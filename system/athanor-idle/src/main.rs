//! `athanor-idle`, the idle daemon (doc_session_daemons.md, SD3, SD4, SD5).
//!
//! The main thread runs the GLib main loop, on which GSettings reports changes of
//! `org.athanor.desktop.idle` and of `enable-animations`, and on which the store of
//! cosmic-settings' Power page is watched and followed (SD3). The Wayland connection, the
//! sequence and the buses run on a Tokio thread of their own, which the GLib thread tells of
//! every change.

use std::cell::RefCell;
use std::error::Error as StdError;
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Instant;

use athanor_idle::battery::Battery;
use athanor_idle::confine;
use athanor_idle::wayland::{Display, Notice, Which};
use athanor_idle::{
    bus, choose, follow, screensaver, Action, Config, Event, Hold, Sequence, Timeouts, Timer,
    LOCK_TIMEOUT,
};
use athanor_sulphur_client::idle as power_page;
use athanor_unit::{journal, sandbox};
use futures_util::StreamExt;
use gio::prelude::*;
use tokio::io::Interest;
use tokio::sync::mpsc::{self, UnboundedReceiver, UnboundedSender};

const SCHEMA: &str = "org.athanor.desktop.idle";
const INTERFACE: &str = "org.gnome.desktop.interface";

/// What the GLib thread reads and the sequence needs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Prefs {
    config: Config,
    reduced_motion: bool,
}

fn main() -> ExitCode {
    journal::init();
    let Some(runtime) = std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .filter(|dir| dir.is_absolute())
    else {
        tracing::error!("no absolute XDG_RUNTIME_DIR");
        return ExitCode::FAILURE;
    };
    // Before any GIO object and any thread: GLib's worker threads, zbus and Tokio are confined
    // from their first instruction (SD17).
    let confined = sandbox::ensure_single_threaded().and_then(|()| {
        let (dir, grants) = confine::daemon(&runtime, power_page::user_dir().as_deref(), |name| {
            std::env::var_os(name)
        })?;
        grants.apply()?;
        Ok(dir)
    });
    let dir = match confined {
        Ok(dir) => dir,
        Err(error) => {
            tracing::error!(%error, "refusing to run unconfined");
            return ExitCode::FAILURE;
        }
    };
    let (changes, received) = mpsc::unbounded_channel();
    let Some(_watched) = watch(changes) else {
        return ExitCode::FAILURE;
    };
    std::thread::spawn(move || {
        if let Err(error) = serve(dir, received) {
            tracing::error!(%error, "athanor-idle stopped");
        }
        std::process::exit(1);
    });
    glib::MainLoop::new(None, false).run();
    ExitCode::SUCCESS
}

/// The schema `id`, or `None` when it is not installed (`Settings::new` would abort).
fn settings(id: &str) -> Option<gio::Settings> {
    let schema = gio::SettingsSchemaSource::default()?.lookup(id, true)?;
    Some(gio::Settings::new_full(
        &schema,
        None::<&gio::SettingsBackend>,
        None,
    ))
}

fn read(idle: &gio::Settings, interface: Option<&gio::Settings>) -> Prefs {
    Prefs {
        config: Config {
            blank_after: idle.uint("blank-after"),
            suspend_after_battery: idle.uint("suspend-after-battery"),
            suspend_after_ac: idle.uint("suspend-after-ac"),
        },
        reduced_motion: interface.is_some_and(|s| !s.boolean("enable-animations")),
    }
}

/// Writes each value; true when every write succeeded.
fn write(idle: &gio::Settings, writes: Vec<(follow::Field, u32)>) -> bool {
    let mut all = true;
    for (field, value) in writes {
        match idle.set_uint(field.key(), value) {
            Ok(()) => tracing::info!(
                "{} is {value}, from the Power page's {}",
                field.key(),
                field.store_key()
            ),
            Err(error) => {
                tracing::warn!(%error, "cannot write {}", field.key());
                all = false;
            }
        }
    }
    all
}

/// What the GLib thread keeps alive while the loop runs.
struct Watched {
    _idle: gio::Settings,
    _interface: Option<gio::Settings>,
    _monitor: Option<gio::FileMonitor>,
}

/// Runs SD16's migration once, follows the Power page, and sends the preferences at once and
/// at every change. `None` when the schema is not installed.
fn watch(changes: UnboundedSender<Prefs>) -> Option<Watched> {
    let Some(idle) = settings(SCHEMA) else {
        tracing::error!("the schema {SCHEMA} is not installed");
        return None;
    };
    let interface = settings(INTERFACE).filter(|s| {
        s.settings_schema()
            .is_some_and(|schema| schema.has_key("enable-animations"))
    });
    if interface.is_none() {
        tracing::warn!("{INTERFACE} enable-animations is not installed; the blank fades");
    }

    // A failed write leaves `migrated` false, so the next start copies again.
    if !idle.boolean("migrated") && write(&idle, follow::migrate(&power_page::read_user())) {
        if let Err(error) = idle.set_boolean("migrated", true) {
            tracing::warn!(%error, "cannot set migrated");
        }
    }

    let monitor = match power_page::user_dir() {
        None => {
            tracing::warn!("no home directory; the Power page is not followed");
            None
        }
        Some(dir) => match gio::File::for_path(&dir)
            .monitor_directory(gio::FileMonitorFlags::NONE, gio::Cancellable::NONE)
        {
            Err(error) => {
                tracing::warn!(%error, "cannot watch the Power page's store; it is not followed");
                None
            }
            Ok(monitor) => {
                let follow = RefCell::new(follow::Follow::start(&power_page::read_in(&dir)));
                let idle = idle.clone();
                monitor.connect_changed(move |_, _, _, _| {
                    let writes = follow.borrow_mut().changed(&power_page::read_in(&dir));
                    // A failed write is in the journal; the page's next change writes again.
                    write(&idle, writes);
                });
                Some(monitor)
            }
        },
    };

    let send = {
        let (idle, interface) = (idle.clone(), interface.clone());
        move || {
            if changes.send(read(&idle, interface.as_ref())).is_err() {
                tracing::debug!("the daemon thread has stopped; the process is exiting");
            }
        }
    };
    send();
    let on_idle = send.clone();
    idle.connect_changed(None, move |_, _| on_idle());
    if let Some(interface) = &interface {
        interface.connect_changed(Some("enable-animations"), move |_, _| send());
    }
    Some(Watched {
        _idle: idle,
        _interface: interface,
        _monitor: monitor,
    })
}

fn serve(dir: PathBuf, received: UnboundedReceiver<Prefs>) -> Result<(), Box<dyn StdError>> {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?
        .block_on(daemon(dir, received))
}

/// Which slot of `Driver::deadlines` a timer has.
fn slot(timer: Timer) -> usize {
    match timer {
        Timer::Fade => 0,
        Timer::LockDelay => 1,
        Timer::LockRetry => 2,
        Timer::LockTimeout => 3,
    }
}

const TIMERS: [Timer; 4] = [
    Timer::Fade,
    Timer::LockDelay,
    Timer::LockRetry,
    Timer::LockTimeout,
];

/// The sequence and what it acts on.
struct Driver {
    display: Display,
    sequence: Sequence,
    deadlines: [Option<Instant>; 4],
    /// The blank notification idled and has not resumed.
    blank_idle: bool,
    reduced_motion: bool,
    system: bus::System,
    /// The session bus, for the notice; it serves nothing.
    notices: zbus::Connection,
    answers: UnboundedSender<Event>,
}

impl Driver {
    fn handle(&mut self, event: Event) {
        let actions = self.sequence.handle(event);
        for action in actions {
            self.act(action);
        }
    }

    fn act(&mut self, action: Action) {
        match action {
            Action::ShowOverlay { fade } => self.display.show_overlay(fade),
            Action::RemoveOverlay => self.display.remove_overlay(),
            Action::OutputsOff => self.display.set_outputs_off(true),
            Action::OutputsOn => self.display.set_outputs_off(false),
            Action::Arm { timer, after } => {
                self.deadlines[slot(timer)] = Some(Instant::now() + after);
            }
            Action::Cancel(timer) => self.deadlines[slot(timer)] = None,
            Action::LockSession { attempt } => {
                let (system, answers) = (self.system.clone(), self.answers.clone());
                tokio::spawn(async move {
                    let answer = match system.lock(LOCK_TIMEOUT).await {
                        Ok(()) => Event::LockDone { attempt },
                        Err(error) => {
                            tracing::warn!(%error, attempt, "LockSession failed");
                            Event::LockFailed { attempt }
                        }
                    };
                    if answers.send(answer).is_err() {
                        tracing::debug!("the daemon loop has stopped; the answer is dropped");
                    }
                });
            }
            Action::Suspend => {
                let system = self.system.clone();
                tokio::spawn(async move {
                    if let Err(error) = system.suspend().await {
                        // Not retried until the next idle period (SD4).
                        tracing::info!(%error, "the automatic suspend failed");
                    }
                });
            }
            Action::LogLockFailure => {
                tracing::warn!("the automatic lock failed twice; the session is not locked");
            }
            Action::Notify(summary) => {
                let notices = self.notices.clone();
                tokio::spawn(async move {
                    if let Err(error) = bus::notify(&notices, summary).await {
                        tracing::warn!(%error, "cannot show the notice {summary:?}");
                    }
                });
            }
        }
    }

    /// The timeouts in force; a changed blank timeout that had idled is withdrawn first.
    async fn apply(&mut self, timeouts: Timeouts) {
        if self.display.timeout(Which::Blank) != timeouts.blank && self.blank_idle {
            self.blank_idle = false;
            let hint = self.system.hint().await;
            self.handle(Event::Withdrawn { hint });
        }
        self.display.set_timeout(Which::Blank, timeouts.blank);
        self.display.set_timeout(Which::Suspend, timeouts.suspend);
    }

    /// `hold` is what the inhibitor table holds now, which a queued `idled` must respect.
    async fn notice(&mut self, notice: Notice, hold: Hold) {
        match notice {
            Notice::Idled(Which::Blank) => {
                self.blank_idle = true;
                let hint = self.system.hint().await;
                self.handle(Event::BlankIdled {
                    hint,
                    reduced_motion: self.reduced_motion,
                });
            }
            Notice::Resumed(Which::Blank) => {
                self.blank_idle = false;
                self.handle(Event::Resumed);
            }
            Notice::Idled(Which::Suspend) => self.handle(Event::SuspendIdled { hold }),
            Notice::Resumed(Which::Suspend) => {}
        }
    }

    fn next_deadline(&self) -> Option<Instant> {
        self.deadlines.iter().flatten().min().copied()
    }

    /// The timers that are due, each checked again after the one before it acted.
    async fn expire(&mut self) {
        for timer in TIMERS {
            if !self.deadlines[slot(timer)].is_some_and(|at| at <= Instant::now()) {
                continue;
            }
            self.deadlines[slot(timer)] = None;
            let event = match timer {
                Timer::Fade => Event::FadeDone,
                Timer::LockDelay => Event::LockDue {
                    hint: self.system.hint().await,
                },
                Timer::LockRetry => Event::LockRetryDue {
                    hint: self.system.hint().await,
                },
                Timer::LockTimeout => Event::LockTimeout,
            };
            self.handle(event);
        }
    }
}

async fn daemon(
    dir: PathBuf,
    mut received: UnboundedReceiver<Prefs>,
) -> Result<(), Box<dyn StdError>> {
    let session = zbus::connection::Builder::session()?.build().await?;
    let session_query = zbus::connection::Builder::session()?.build().await?;
    let system_query = zbus::connection::Builder::system()?
        .method_timeout(LOCK_TIMEOUT)
        .build()
        .await?;
    let system_watch = zbus::connection::Builder::system()?.build().await?;
    let system = bus::System::new(system_query);

    let (mut hold, mut ended) =
        screensaver::serve(&session, &session_query, dir.join("inhibitors.json")).await?;

    let mut changes = bus::upower_changes(&system_watch).await?;
    let mut owners = bus::upower_owners(&system_watch).await?;
    let mut battery = Battery::new();
    let mut retry = read_battery(&system, &mut battery).await;

    let display = Display::connect()?;
    // A duplicate of the display's socket, which only tells when it is readable; it shares the
    // open file, so the display's own reads are non-blocking too.
    let socket = std::os::unix::net::UnixStream::from(display.fd()?);
    socket.set_nonblocking(true)?;
    let socket = tokio::net::UnixStream::from_std(socket)?;
    let mut prefs = received.recv().await.ok_or("the settings thread stopped")?;
    let (answers, mut answered) = mpsc::unbounded_channel();
    let mut driver = Driver {
        display,
        sequence: Sequence::new(),
        deadlines: [None; 4],
        blank_idle: false,
        reduced_motion: prefs.reduced_motion,
        system: system.clone(),
        notices: session_query,
        answers,
    };
    driver
        .apply(choose(&prefs.config, battery.on_battery(), *hold.borrow()))
        .await;
    driver.display.flush()?;
    tracing::info!("athanor-idle is running");

    loop {
        let deadline = driver.next_deadline();
        tokio::select! {
            changed = received.recv() => {
                prefs = changed.ok_or("the settings thread stopped")?;
                driver.reduced_motion = prefs.reduced_motion;
            }
            changed = hold.changed() => changed?,
            changed = changes.next() => {
                changed.ok_or("UPower's property stream ended")?;
                retry = read_battery(&system, &mut battery).await;
            }
            gained = owners.next() => {
                if gained.ok_or("UPower's owner stream ended")? {
                    retry = read_battery(&system, &mut battery).await;
                }
            }
            () = sleep_until(retry), if retry.is_some() => {
                retry = read_battery(&system, &mut battery).await;
            }
            ready = socket.readable() => {
                ready?;
                // A `WouldBlock` read clears the readiness until the next event.
                match socket.try_io(Interest::READABLE, || driver.display.read()) {
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {}
                    read => read?,
                }
                for notice in driver.display.dispatch()? {
                    driver.notice(notice, *hold.borrow()).await;
                }
            }
            () = sleep_until(deadline), if deadline.is_some() => driver.expire().await,
            Some(answer) = answered.recv() => driver.handle(answer),
            _ = &mut ended => return Err("the bus is no longer followed".into()),
        }
        driver
            .apply(choose(&prefs.config, battery.on_battery(), *hold.borrow()))
            .await;
        driver.display.flush()?;
    }
}

/// Reads `OnBattery` into `battery`: when to read again after a failure, `None` after a
/// success. A failure keeps what was known, mains before the first success.
async fn read_battery(system: &bus::System, battery: &mut Battery) -> Option<Instant> {
    let read = match system.on_battery().await {
        Ok(now) => Some(now),
        Err(error) => {
            tracing::warn!(%error, "cannot read UPower's OnBattery; kept as it was, read again later");
            None
        }
    };
    battery.answer(read).map(|wait| Instant::now() + wait)
}

async fn sleep_until(deadline: Option<Instant>) {
    if let Some(at) = deadline {
        tokio::time::sleep_until(at.into()).await;
    }
}
