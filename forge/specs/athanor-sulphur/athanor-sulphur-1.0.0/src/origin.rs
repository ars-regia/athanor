//! The origin check of the allow-list (doc_own_stack.md OS4, #1560). A client granted a line
//! outside `identity::TRANSIENT` gets that line's interfaces only once the user's systemd says
//! it is the main process of the line's unit, whose one `ExecStart` runs its executable and
//! whose files lie in the image (`athanor_unit::bus_caller::admit_pidfd`, asked with the pidfd
//! of `SO_PEERPIDFD`). Until then the client is not inserted: nothing of it is served.
//!
//! The event loop never waits on the bus: it hands the client to the `origin` thread through a
//! queue of [`JOBS`], with `try_send`, and the thread asks on connections of its own, the whole
//! check within `eis::ASK`, then sends the verdict back through a calloop channel, where the
//! loop inserts the client. A refusal, a timeout, a full queue or a thread that is gone grant
//! the client its transient lines alone.

use crate::eis::{within_ask, ASK};
use crate::identity::Peer;
use crate::Error;
use athanor_unit::bus_caller::{self, OnBus, Questions, Service};
use smithay::reexports::calloop::{channel, LoopHandle};
use std::os::fd::AsFd;
use std::os::unix::net::UnixStream;
use std::sync::mpsc::{sync_channel, SyncSender};
use zbus::connection::Builder;
use zbus::fdo::DBusProxy;

/// The clients the queue holds while one is checked: the shell's clients connect together when
/// the session starts, and a few more wait behind them.
const JOBS: usize = 16;

/// A client waiting for its check: its connection, and what `identify` read of it.
#[derive(Debug)]
pub struct Job {
    pub stream: UnixStream,
    pub peer: Peer,
}

/// A checked job: `Ok` admits it, `Err` says why not.
pub struct Verdict {
    pub job: Job,
    pub admitted: Result<(), String>,
}

/// Starts the `origin` thread, which runs `check` on each job, and returns its queue; `done`
/// receives each verdict on the loop.
pub fn start<D: 'static>(
    handle: &LoopHandle<'static, D>,
    mut check: impl FnMut(&Job) -> Result<(), String> + Send + 'static,
    mut done: impl FnMut(&mut D, Verdict) + 'static,
) -> Result<SyncSender<Job>, Error> {
    let (verdicts, received) = channel::channel();
    handle
        .insert_source(received, move |event, _, data| {
            if let channel::Event::Msg(verdict) = event {
                done(data, verdict);
            }
        })
        .map_err(|e| e.error)?;
    let (jobs, queued) = sync_channel::<Job>(JOBS);
    std::thread::Builder::new()
        .name("origin".into())
        .spawn(move || {
            for job in queued {
                let admitted = check(&job);
                if verdicts.send(Verdict { job, admitted }).is_err() {
                    // The loop has ended, and the compositor with it.
                    return;
                }
            }
        })?;
    Ok(jobs)
}

/// Asks the buses whether the job's process is the main process of its unit of the image, on
/// connections made for this one check.
// ponytail: two connections per check, for the few clients that name a line; keep them across
// checks if a session ever starts many.
pub fn ask(job: &Job) -> Result<(), String> {
    ask_on(Builder::session(), bus_caller::system_bus(), job)
}

/// `ask` on the buses `session` and `system` reach. The whole check, the handshakes that zbus
/// does not bound included, ends within `eis::ASK`: a bus or a systemd that does not answer
/// refuses the client in a few seconds, and the thread takes the next job.
fn ask_on(session: Builder<'_>, system: Builder<'_>, job: &Job) -> Result<(), String> {
    zbus::block_on(async {
        let check = async {
            let session = within_ask(session)
                .build()
                .await
                .map_err(|e| format!("the session bus: {e}"))?;
            let system = within_ask(system)
                .build()
                .await
                .map_err(|e| format!("the system bus: {e}"))?;
            let bus = DBusProxy::new(&session)
                .await
                .map_err(|e| format!("the session bus: {e}"))?;
            let questions = OnBus {
                bus: &bus,
                session: &session,
                system: &system,
            };
            admit(&questions, job).await
        };
        tokio::time::timeout(ASK, check)
            .await
            .unwrap_or_else(|_| Err(format!("the buses did not answer within {ASK:?}")))
    })
}

/// Whether `questions` place the job's process as the main process of the unit its identity
/// names, run by its executable, as `ask` asks the buses.
async fn admit(questions: &impl Questions, job: &Job) -> Result<(), String> {
    let identity = &job.peer.identity;
    let unit = identity.unit.as_deref().ok_or("the client is in no unit")?;
    let exec = identity
        .executable
        .to_str()
        .ok_or("the executable's path is not UTF-8")?;
    let pid = u32::try_from(job.peer.pid).map_err(|e| e.to_string())?;
    bus_caller::admit_pidfd(questions, job.peer.pidfd.as_fd(), pid, Service { unit, exec }).await
}

// Test-only code comes last: `verify.py panics` stops reading a file where it begins.
#[cfg(test)]
mod tests {
    use super::*;
    use crate::identity::{Grants, Identity, Privilege};
    use smithay::reexports::calloop::EventLoop;
    use std::sync::mpsc::TrySendError;
    use std::sync::{Arc, Barrier};
    use std::time::{Duration, Instant};
    use zbus::OwnedObjectPath;

    /// A job for the test's own process at the other end of a socket pair.
    fn job(by_origin: Vec<Privilege>) -> (Job, UnixStream) {
        let (server, client) = UnixStream::pair().expect("socket pair");
        let peer = crate::identity::identify(
            server.as_fd(),
            &crate::identity::Proc,
            &crate::identity::AllowList::default(),
        )
        .expect("identity");
        let peer = Peer {
            identity: Identity {
                unit: Some("athanor-bar.service".into()),
                ..peer.identity
            },
            grants: Grants {
                exempt: Vec::new(),
                by_origin,
            },
            ..peer
        };
        (
            Job {
                stream: server,
                peer,
            },
            client,
        )
    }

    /// Dispatches `event_loop` until `verdicts` holds `count`, for ten seconds at most.
    fn wait(
        event_loop: &mut EventLoop<'static, Vec<Verdict>>,
        verdicts: &mut Vec<Verdict>,
        count: usize,
    ) {
        let deadline = Instant::now() + Duration::from_secs(10);
        while verdicts.len() < count && Instant::now() < deadline {
            event_loop
                .dispatch(Some(Duration::from_millis(50)), verdicts)
                .expect("dispatch");
        }
    }

    /// Each job comes back to the loop with the verdict of its check, its connection and grants
    /// with it; the loop never runs the check.
    #[test]
    fn each_job_comes_back_to_the_loop_with_its_verdict() {
        let mut event_loop = EventLoop::try_new().expect("loop");
        let looping = std::thread::current().id();
        let jobs = start(
            &event_loop.handle(),
            move |job: &Job| {
                assert_ne!(std::thread::current().id(), looping);
                if job.peer.grants.by_origin.contains(&Privilege::TopLayers) {
                    Ok(())
                } else {
                    Err("refused".into())
                }
            },
            |verdicts: &mut Vec<Verdict>, verdict| verdicts.push(verdict),
        )
        .expect("origin thread");
        let (admitted, _a) = job(vec![Privilege::TopLayers]);
        let (refused, _r) = job(vec![Privilege::ToplevelList]);
        jobs.try_send(admitted).expect("queued");
        jobs.try_send(refused).expect("queued");
        let mut verdicts = Vec::new();
        wait(&mut event_loop, &mut verdicts, 2);
        assert_eq!(verdicts.len(), 2);
        assert_eq!(verdicts[0].admitted, Ok(()));
        assert_eq!(
            verdicts[0].job.peer.grants.by_origin,
            [Privilege::TopLayers]
        );
        assert_eq!(verdicts[1].admitted, Err("refused".into()));
    }

    /// While a check hangs, the queue takes `JOBS` clients and gives the next one back at once:
    /// the loop never waits for it.
    #[test]
    fn a_full_queue_gives_the_job_back_at_once() {
        let mut event_loop = EventLoop::try_new().expect("loop");
        let hold = Arc::new(Barrier::new(2));
        let held = hold.clone();
        let mut first = true;
        let jobs = start(
            &event_loop.handle(),
            move |_: &Job| {
                if std::mem::take(&mut first) {
                    held.wait();
                }
                Ok(())
            },
            |verdicts: &mut Vec<Verdict>, verdict| verdicts.push(verdict),
        )
        .expect("origin thread");
        let mut clients = Vec::new();
        // The first is taken by the thread, which then holds it; `JOBS` more fill the queue.
        let mut queued = 0;
        let started = Instant::now();
        let overflow = loop {
            let (job, client) = job(vec![Privilege::TopLayers]);
            clients.push(client);
            match jobs.try_send(job) {
                Ok(()) => queued += 1,
                Err(TrySendError::Full(job)) => break job,
                Err(TrySendError::Disconnected(_)) => panic!("the thread is gone"),
            }
            assert!(queued <= JOBS + 1, "the queue is not bounded");
        };
        assert!(started.elapsed() < Duration::from_secs(1));
        assert!(queued >= JOBS);
        assert!(overflow.peer.pid > 0);
        hold.wait();
        let mut verdicts = Vec::new();
        wait(&mut event_loop, &mut verdicts, queued);
        assert_eq!(verdicts.len(), queued);
    }

    /// A user's systemd that answers for the test's own process as the main process of
    /// athanor-bar.service, its manager the main process of user@<uid>.service, with the unit
    /// file at `fragment`.
    struct Systemd {
        fragment: String,
        exec: String,
        pid: u32,
        pidfd: std::os::fd::OwnedFd,
    }

    impl Questions for Systemd {
        fn user_manager(&self) -> impl std::future::Future<Output = Result<String, String>> + Send {
            std::future::ready(Ok(":1.1".to_owned()))
        }

        fn credentials(
            &self,
            _peer: &str,
        ) -> impl std::future::Future<Output = Result<bus_caller::Credentials, String>> + Send {
            std::future::ready(Ok(bus_caller::Credentials {
                uid: Some(nix::unistd::getuid().as_raw()),
                pid: Some(self.pid),
                pidfd: Some(self.pidfd.try_clone().expect("pidfd")),
            }))
        }

        fn unit_by_pidfd(
            &self,
            manager: bus_caller::Manager<'_>,
            _pidfd: std::os::fd::BorrowedFd<'_>,
        ) -> impl std::future::Future<Output = Result<(OwnedObjectPath, String), String>> + Send
        {
            let unit = match manager {
                bus_caller::Manager::System => format!("user@{}.service", nix::unistd::getuid()),
                bus_caller::Manager::User(_) => "athanor-bar.service".to_owned(),
            };
            let path = OwnedObjectPath::try_from("/org/freedesktop/systemd1/unit/u").expect("path");
            std::future::ready(Ok((path, unit)))
        }

        fn main_pid(
            &self,
            _manager: bus_caller::Manager<'_>,
            _path: &OwnedObjectPath,
        ) -> impl std::future::Future<Output = Result<u32, String>> + Send {
            std::future::ready(Ok(self.pid))
        }

        fn unit(
            &self,
            _manager: bus_caller::Manager<'_>,
            _path: &OwnedObjectPath,
        ) -> impl std::future::Future<Output = Result<bus_caller::Unit, String>> + Send {
            std::future::ready(Ok(bus_caller::Unit {
                main_pid: self.pid,
                exec_start: vec![self.exec.clone()],
                fragment_path: self.fragment.clone(),
                drop_in_paths: Vec::new(),
            }))
        }
    }

    /// The check admits the main process of a unit whose file lies in the image, and refuses
    /// the same process when its unit is one the user defined under ~/.config/systemd/user, a
    /// unit any process of the user can write (OS4, #1560).
    #[test]
    fn a_unit_the_user_defined_is_refused_and_one_of_the_image_admitted() {
        let (job, _client) = job(vec![Privilege::TopLayers]);
        let systemd = |fragment: &str| Systemd {
            fragment: fragment.to_owned(),
            exec: job.peer.identity.executable.to_str().expect("UTF-8").to_owned(),
            pid: u32::try_from(job.peer.pid).expect("pid"),
            pidfd: job.peer.pidfd.try_clone().expect("pidfd"),
        };
        let image = systemd("/usr/lib/systemd/user/athanor-bar.service");
        assert_eq!(zbus::block_on(admit(&image, &job)), Ok(()));
        let home = systemd("/home/user/.config/systemd/user/athanor-bar.service");
        let refused = zbus::block_on(admit(&home, &job));
        assert!(
            refused
                .as_ref()
                .is_err_and(|why| why.contains("is defined by") && why.contains("/.config/systemd/user/")),
            "{refused:?}"
        );
    }

    /// The production check refuses a process that no systemd places in the unit: here, wherever
    /// the test runs, no user manager answers for the test's process as the bar's main process.
    #[test]
    fn the_check_on_the_buses_refuses_a_process_outside_its_unit() {
        let (job, _client) = job(vec![Privilege::TopLayers]);
        let asked = ask(&job);
        eprintln!("the origin check of the test's process: {asked:?}");
        assert!(asked.is_err());
    }

    /// A bus that takes the connection and never answers, not even the handshake, ends the
    /// check within `eis::ASK` with a refusal, and the thread then takes the next job: a silent
    /// bus never holds the queue (OS4, #1560).
    #[test]
    fn a_bus_that_never_answers_refuses_within_the_bound_and_frees_the_thread() {
        let mut event_loop = EventLoop::try_new().expect("loop");
        // The far ends stay open and silent for as long as the thread runs.
        let mut silent = Vec::new();
        let jobs = start(
            &event_loop.handle(),
            move |job: &Job| {
                let (session, session_end) = UnixStream::pair().expect("socket pair");
                let (system, system_end) = UnixStream::pair().expect("socket pair");
                silent.extend([session_end, system_end]);
                ask_on(
                    Builder::unix_stream(session),
                    Builder::unix_stream(system),
                    job,
                )
            },
            |verdicts: &mut Vec<Verdict>, verdict| verdicts.push(verdict),
        )
        .expect("origin thread");
        let (first, _a) = job(vec![Privilege::TopLayers]);
        let (second, _b) = job(vec![Privilege::TopLayers]);
        let started = Instant::now();
        jobs.try_send(first).expect("queued");
        jobs.try_send(second).expect("queued");
        let mut verdicts = Vec::new();
        wait(&mut event_loop, &mut verdicts, 2);
        assert_eq!(verdicts.len(), 2, "a silent bus held the thread");
        assert!(started.elapsed() < ASK * 3);
        for verdict in &verdicts {
            assert!(
                verdict
                    .admitted
                    .as_ref()
                    .is_err_and(|why| why.contains("did not answer")),
                "{:?}",
                verdict.admitted
            );
        }
    }
}
