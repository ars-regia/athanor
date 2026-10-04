//! The wall clock as the do-not-disturb schedule needs it: local time, and a signal whenever
//! it may have jumped (the clock set, a resume from sleep, a new timezone).
use std::collections::HashMap;
use std::os::fd::{AsFd, AsRawFd, RawFd};
use std::sync::Once;

use futures_util::StreamExt;
use nix::errno::Errno;
use nix::sys::time::TimeSpec;
use nix::sys::timerfd::{ClockId, Expiration, TimerFd, TimerFlags, TimerSetTimeFlags};
use tokio::io::unix::AsyncFd;
use tokio::sync::{mpsc::Sender, watch};
use tracing::warn;

use crate::dnd::Local;

/// Unix time and the local minute and weekday. Reads `/etc/localtime` again on each call
/// (`tzset`), so a timezone change is seen without a restart. Should `localtime_r` fail, the
/// UTC fields stand in, with one warning.
#[must_use]
pub fn local_now() -> (i64, Local) {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or_else(
            |err| -(err.duration().as_secs() as i64),
            |d| d.as_secs() as i64,
        );
    let tm = localtime(now);
    let local = match tm {
        Some(tm) => Local {
            minute: (tm.tm_hour * 60 + tm.tm_min) as u16,
            weekday: ((tm.tm_wday + 6) % 7) as u8, // tm_wday: 0 = Sunday
        },
        None => {
            static ONCE: Once = Once::new();
            ONCE.call_once(|| warn!("localtime_r failed, using UTC for the schedule"));
            utc_fields(now)
        }
    };
    (now, local)
}

extern "C" {
    /// POSIX; glibc reads `/etc/localtime` again on each call. Not in the `libc` crate.
    fn tzset();
}

/// `localtime_r` after `tzset`; `None` when the conversion fails.
#[allow(unsafe_code)] // the only FFI of the daemon: libc has no safe local-time call
fn localtime(now: i64) -> Option<libc::tm> {
    let time: libc::time_t = now;
    // SAFETY: `tzset` takes no arguments and only refreshes libc's own timezone state.
    unsafe { tzset() };
    // SAFETY: `libc::tm` is plain integers and a nullable pointer, for which all-zero is valid.
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    // SAFETY: both pointers are valid for the call; `localtime_r` writes only to `tm`.
    let done = unsafe { libc::localtime_r(&time, &mut tm) };
    (!done.is_null()).then_some(tm)
}

fn utc_fields(now: i64) -> Local {
    Local {
        minute: (now.rem_euclid(86_400) / 60) as u16,
        weekday: ((now.div_euclid(86_400) + 3).rem_euclid(7)) as u8, // 1970-01-01 was a Thursday
    }
}

/// nix's `TimerFd` hands out its descriptor as `AsFd` only; `AsyncFd` wants `AsRawFd`.
struct Timer(TimerFd);

impl AsRawFd for Timer {
    fn as_raw_fd(&self) -> RawFd {
        self.0.as_fd().as_raw_fd()
    }
}

fn arm(timer: &TimerFd, seconds: u64) -> Result<(), Errno> {
    let at = nix::time::clock_gettime(nix::time::ClockId::CLOCK_REALTIME)?
        + TimeSpec::new(seconds.max(1).min(i64::MAX as u64 / 2) as i64, 0);
    timer.set(
        Expiration::OneShot(at),
        TimerSetTimeFlags::TFD_TIMER_ABSTIME | TimerSetTimeFlags::TFD_TIMER_CANCEL_ON_SET,
    )
}

/// `()` at each wake (seconds from now, read from `wake` each time it is moved or fires), and
/// whenever the real-time clock is set: the read then fails with `ECANCELED`.
async fn timer(tx: Sender<()>, mut wake: watch::Receiver<u64>) {
    let fd = match TimerFd::new(
        ClockId::CLOCK_REALTIME,
        TimerFlags::TFD_NONBLOCK | TimerFlags::TFD_CLOEXEC,
    )
    .map_err(io_error)
    .and_then(|timer| AsyncFd::new(Timer(timer)))
    {
        Ok(fd) => fd,
        Err(err) => {
            warn!(%err, "no timer for do not disturb: the schedule will not wake by itself");
            return;
        }
    };
    loop {
        let seconds = *wake.borrow_and_update();
        if let Err(err) = arm(&fd.get_ref().0, seconds) {
            warn!(%err, "cannot arm the do-not-disturb timer");
            return;
        }
        tokio::select! {
            changed = wake.changed() => if changed.is_err() { return },
            ready = fd.readable() => {
                let Ok(mut ready) = ready else { return };
                let read = fd.get_ref().0.wait();
                ready.clear_ready();
                match read {
                    Ok(()) | Err(Errno::ECANCELED) => {}
                    Err(Errno::EAGAIN) => continue,
                    Err(err) => { warn!(%err, "do-not-disturb timer failed"); return }
                }
                if tx.send(()).await.is_err() { return }
            }
        }
    }
}

fn io_error(err: Errno) -> std::io::Error {
    std::io::Error::from_raw_os_error(err as i32)
}

/// `()` for each reason to evaluate again: the wake of the timer, the clock set, a resume from
/// sleep (logind `PrepareForSleep(false)`), a new timezone (timedated `Timezone`). The first
/// two need no bus; without the system bus this logs once and the timer alone remains.
pub async fn changes(tx: Sender<()>, wake: watch::Receiver<u64>) {
    tokio::join!(timer(tx.clone(), wake), system_changes(tx));
}

/// The resume and timezone signals of the system bus; returns at once, with one warning, when
/// the bus is absent, and when it goes away.
pub async fn system_changes(tx: Sender<()>) {
    match zbus::Connection::system().await {
        Ok(conn) => bus_changes(conn, tx).await,
        Err(err) => {
            warn!(%err, "no system bus: do not disturb will not notice sleep or timezone changes")
        }
    }
}

/// [`system_changes`] on a given connection.
pub async fn bus_changes(conn: zbus::Connection, tx: Sender<()>) {
    let (resume, timezone) = match tokio::try_join!(resumes(&conn), timezones(&conn)) {
        Ok(streams) => streams,
        Err(err) => {
            warn!(%err, "cannot follow logind and timedated: sleep and timezone changes are not noticed");
            return;
        }
    };
    let mut both = futures_util::stream::select(resume, timezone);
    while both.next().await.is_some() {
        if tx.send(()).await.is_err() {
            return;
        }
    }
    warn!("the system bus went away: sleep and timezone changes are no longer noticed");
}

type Changes = std::pin::Pin<Box<dyn futures_util::Stream<Item = ()> + Send>>;

async fn resumes(conn: &zbus::Connection) -> zbus::Result<Changes> {
    let login = zbus::Proxy::new(
        conn,
        "org.freedesktop.login1",
        "/org/freedesktop/login1",
        "org.freedesktop.login1.Manager",
    )
    .await?;
    let signals = login.receive_signal("PrepareForSleep").await?;
    Ok(Box::pin(signals.filter_map(|message| async move {
        // `true` is the machine going down; the wake-up is the `false` after it.
        (message.body().deserialize::<bool>() == Ok(false)).then_some(())
    })))
}

async fn timezones(conn: &zbus::Connection) -> zbus::Result<Changes> {
    let timedate = zbus::Proxy::new(
        conn,
        "org.freedesktop.timedate1",
        "/org/freedesktop/timedate1",
        "org.freedesktop.DBus.Properties",
    )
    .await?;
    let signals = timedate.receive_signal("PropertiesChanged").await?;
    Ok(Box::pin(signals.filter_map(|message| async move {
        let (interface, changed, invalidated): (
            String,
            HashMap<String, zvariant::OwnedValue>,
            Vec<String>,
        ) = message.body().deserialize().ok()?;
        (interface == "org.freedesktop.timedate1"
            && (changed.contains_key("Timezone")
                || invalidated.iter().any(|key| key == "Timezone")))
        .then_some(())
    })))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dnd::Local;

    #[test]
    fn local_time_agrees_with_chrono() {
        use chrono::{Datelike, Timelike};
        let reading = |t: chrono::DateTime<chrono::Local>| Local {
            minute: (t.hour() * 60 + t.minute()) as u16,
            weekday: t.weekday().num_days_from_monday() as u8,
        };
        let before = reading(chrono::Local::now());
        let (now, ours) = local_now();
        let after = reading(chrono::Local::now());
        assert!(
            [
                (before.minute, before.weekday),
                (after.minute, after.weekday)
            ]
            .contains(&(ours.minute, ours.weekday)),
            "a minute boundary may fall between the readings, never outside them"
        );
        assert!((now - chrono::Local::now().timestamp()).abs() <= 1);
    }

    #[test]
    fn utc_fields_stand_in_when_localtime_fails() {
        // 1970-01-01 was a Thursday; 2026-10-05 12:34:56 UTC is a Monday.
        assert_eq!(
            utc_fields(0),
            Local {
                minute: 0,
                weekday: 3
            }
        );
        assert_eq!(
            utc_fields(1_791_203_696),
            Local {
                minute: 12 * 60 + 34,
                weekday: 0
            }
        );
        assert_eq!(
            utc_fields(-1),
            Local {
                minute: 23 * 60 + 59,
                weekday: 2
            }
        );
    }

    #[tokio::test]
    async fn the_timer_fires_at_the_wake_and_again_when_it_is_moved() {
        let (tx, mut rx) = tokio::sync::mpsc::channel(4);
        let (wake_tx, wake) = tokio::sync::watch::channel(3_600_u64);
        let task = tokio::spawn(timer(tx, wake));
        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(300), rx.recv())
                .await
                .is_err(),
            "an hour away does not fire"
        );
        wake_tx.send(1).expect("move the wake");
        let fired = tokio::time::timeout(std::time::Duration::from_secs(3), rx.recv()).await;
        assert_eq!(fired, Ok(Some(())));
        task.abort();
    }
}
