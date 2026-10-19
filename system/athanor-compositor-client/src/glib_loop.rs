//! GLib's default main context run inside a calloop loop, for a surface drawn without GTK.
//! The launch path awaits GIO's D-Bus calls and the client reads its queue from a GLib
//! source; an application whose loop is calloop's runs GLib's as one of its sources.
//!
//! Each turn of the calloop loop is one iteration of GLib's, split as
//! `g_main_context_iteration` splits it. Before calloop sleeps, `prepare` and `query` name
//! the descriptors and the timeout GLib would wait on. They go into an epoll set built afresh
//! for that iteration, nested in the epoll set calloop polls, beside a timer that stands for
//! the timeout, so calloop's one poll waits on both loops. When the set is ready, or GLib had
//! something ready at once, `check` and `dispatch` run GLib's sources. Nothing but calloop's
//! poll ever blocks.

#![allow(unsafe_code)]

use std::io;
use std::marker::PhantomData;
use std::os::fd::{AsFd, AsRawFd, BorrowedFd, RawFd};
use std::os::raw::{c_int, c_uint};
use std::time::Duration;

use calloop::generic::Generic;
use calloop::{EventSource, Interest, Mode, Poll, PostAction, Readiness, Token, TokenFactory};
use glib::ffi::{
    g_main_context_acquire, g_main_context_check, g_main_context_dispatch, g_main_context_prepare,
    g_main_context_query, g_main_context_release, g_poll, GMainContext, GPollFD, G_IO_IN, G_IO_OUT,
    G_IO_PRI,
};
use glib::translate::ToGlibPtr;
use nix::errno::Errno;
use nix::libc;
use nix::sys::epoll::{Epoll, EpollCreateFlags, EpollEvent, EpollFlags};
use nix::sys::time::TimeSpec;
use nix::sys::timerfd::{ClockId, Expiration, TimerFd, TimerFlags, TimerSetTimeFlags};

/// The epoll set calloop polls, as a descriptor it can register.
struct EpollFd(Epoll);

impl AsFd for EpollFd {
    fn as_fd(&self) -> BorrowedFd<'_> {
        self.0 .0.as_fd()
    }
}

/// GLib's default main context as a calloop event source. It owns the context for as long
/// as it lives, so it is created and dropped on the thread that runs the loop. Its callback
/// runs after each turn that dispatched GLib's sources.
pub struct GlibSource {
    context: glib::MainContext,
    /// The set calloop polls: the timer, and this iteration's set of GLib's descriptors.
    epoll: Generic<EpollFd>,
    timer: TimerFd,
    /// What the last `query` asked to poll, and the set that holds those descriptors.
    fds: Vec<GPollFD>,
    iteration: Option<Epoll>,
    max_priority: c_int,
    /// True between a `prepare` and the `check` that ends its iteration.
    prepared: bool,
    /// The token of the turns `before_sleep` asks for when GLib needs no wait.
    token: Option<Token>,
    /// Acquiring a context binds it to this thread.
    _thread: PhantomData<*const ()>,
}

impl GlibSource {
    /// Fails when another thread owns GLib's default main context.
    pub fn new() -> io::Result<Self> {
        let epoll = Epoll::new(EpollCreateFlags::EPOLL_CLOEXEC)?;
        let timer = TimerFd::new(
            ClockId::CLOCK_MONOTONIC,
            TimerFlags::TFD_CLOEXEC | TimerFlags::TFD_NONBLOCK,
        )?;
        epoll.add(&timer, EpollEvent::new(EpollFlags::EPOLLIN, 0))?;
        // Acquired last, once nothing else can fail: `Drop` releases it.
        let context = glib::MainContext::default();
        // SAFETY: the pointer is the live context `context` holds a reference to.
        if unsafe { g_main_context_acquire(raw(&context)) } == 0 {
            return Err(io::Error::other(
                "GLib's default main context belongs to another thread",
            ));
        }
        Ok(Self {
            context,
            epoll: Generic::new(EpollFd(epoll), Interest::READ, Mode::Level),
            timer,
            fds: Vec::new(),
            iteration: None,
            max_priority: 0,
            prepared: false,
            token: None,
            _thread: PhantomData,
        })
    }

    /// The first half of an iteration: true when GLib has a source ready already, so that
    /// calloop must not wait.
    fn prepare(&mut self) -> io::Result<bool> {
        let context = raw(&self.context);
        // SAFETY: this thread owns the live context; the pointers are to our own fields,
        // and `fds` has room for the count passed with it.
        let ready = unsafe { g_main_context_prepare(context, &mut self.max_priority) } != 0;
        let mut timeout: c_int = -1;
        loop {
            let room = self.fds.len();
            // SAFETY: as for `prepare`; `fds` holds `room` entries.
            let wanted = unsafe {
                g_main_context_query(
                    context,
                    self.max_priority,
                    &mut timeout,
                    self.fds.as_mut_ptr(),
                    c_int::try_from(room).map_err(io::Error::other)?,
                )
            };
            let wanted = usize::try_from(wanted).map_err(io::Error::other)?;
            if wanted <= room {
                self.fds.truncate(wanted);
                break;
            }
            self.fds.resize(
                wanted,
                GPollFD {
                    fd: -1,
                    events: 0,
                    revents: 0,
                },
            );
        }
        self.prepared = true;
        let always_ready = self.watch()?;
        match u64::try_from(timeout) {
            Ok(0) | Err(_) => self.timer.unset()?,
            Ok(ms) => self.timer.set(
                Expiration::OneShot(TimeSpec::from_duration(Duration::from_millis(ms))),
                TimerSetTimeFlags::empty(),
            )?,
        }
        Ok(ready || timeout == 0 || always_ready)
    }

    /// Puts the descriptors of the last query, once each, in a new epoll set that replaces the
    /// last iteration's. Built afresh, the set holds exactly what GLib asked for: a descriptor
    /// closed since, or a number reused by another file, can neither linger nor be missed,
    /// and no descriptor is ever deleted by its number. True when a descriptor cannot be
    /// watched, so that this iteration does not wait and `g_poll` reports it to GLib as
    /// poll would: epoll refuses a regular file with EPERM, which poll calls always ready,
    /// and a closed descriptor with EBADF, which poll answers POLLNVAL. Any other error
    /// (ENOMEM, ENOSPC) ends the loop.
    fn watch(&mut self) -> io::Result<bool> {
        let mut wanted: Vec<(RawFd, EpollFlags)> = Vec::with_capacity(self.fds.len());
        // poll ignores a negative descriptor; so does the set.
        for fd in self.fds.iter().filter(|fd| fd.fd >= 0) {
            let events = c_uint::from(fd.events);
            let mut flags = EpollFlags::empty();
            if events & (G_IO_IN | G_IO_PRI) != 0 {
                flags |= EpollFlags::EPOLLIN | EpollFlags::EPOLLPRI;
            }
            if events & G_IO_OUT != 0 {
                flags |= EpollFlags::EPOLLOUT;
            }
            match wanted.iter_mut().find(|(known, _)| *known == fd.fd) {
                Some((_, known)) => *known |= flags,
                None => wanted.push((fd.fd, flags)),
            }
        }
        let set = Epoll::new(EpollCreateFlags::EPOLL_CLOEXEC)?;
        let mut always_ready = false;
        for (fd, flags) in wanted {
            let mut event = libc::epoll_event {
                events: flags.bits().cast_unsigned(),
                u64: 0,
            };
            // SAFETY: plain system call on a number, which may name a closed descriptor: the
            // kernel answers EBADF then. `set` is open and `event` lives across the call.
            let added = unsafe {
                libc::epoll_ctl(set.0.as_raw_fd(), libc::EPOLL_CTL_ADD, fd, &mut event)
            };
            if added < 0 {
                match Errno::last() {
                    Errno::EPERM | Errno::EBADF => always_ready = true,
                    errno => return Err(errno.into()),
                }
            }
        }
        let outer = &self.epoll.get_ref().0;
        if let Some(last) = self.iteration.take() {
            outer.delete(&last.0)?;
        }
        outer.add(&set.0, EpollEvent::new(EpollFlags::EPOLLIN, 0))?;
        self.iteration = Some(set);
        Ok(always_ready)
    }

    /// The second half: reads which descriptors are ready without waiting, and dispatches
    /// the sources they make ready. True when GLib dispatched.
    fn dispatch(&mut self) -> io::Result<bool> {
        if !std::mem::take(&mut self.prepared) {
            return Ok(false);
        }
        let count = c_uint::try_from(self.fds.len()).map_err(io::Error::other)?;
        // SAFETY: `fds` holds `count` entries the last query filled.
        if unsafe { g_poll(self.fds.as_mut_ptr(), count, 0) } < 0 {
            let err = io::Error::last_os_error();
            if err.kind() != io::ErrorKind::Interrupted {
                return Err(err);
            }
        }
        let context = raw(&self.context);
        // SAFETY: this thread owns the live context, which was prepared and queried with
        // these descriptors.
        let ready = unsafe {
            g_main_context_check(
                context,
                self.max_priority,
                self.fds.as_mut_ptr(),
                c_int::try_from(count).map_err(io::Error::other)?,
            )
        } != 0;
        if ready {
            // SAFETY: as above, after a check that found sources ready.
            unsafe { g_main_context_dispatch(context) };
        }
        Ok(ready)
    }
}

fn raw(context: &glib::MainContext) -> *mut GMainContext {
    context.to_glib_none().0
}

impl Drop for GlibSource {
    fn drop(&mut self) {
        // SAFETY: `new` acquired the context on this thread; the type is not `Send`.
        unsafe { g_main_context_release(raw(&self.context)) };
    }
}

impl EventSource for GlibSource {
    type Event = ();
    type Metadata = ();
    type Ret = ();
    type Error = io::Error;

    const NEEDS_EXTRA_LIFECYCLE_EVENTS: bool = true;

    fn process_events<F>(
        &mut self,
        _: Readiness,
        _: Token,
        mut callback: F,
    ) -> Result<PostAction, Self::Error>
    where
        F: FnMut(Self::Event, &mut Self::Metadata) -> Self::Ret,
    {
        // Every token routed here is ours: the epoll set's, or the turn `before_sleep` asked
        // for. Either way the iteration `prepare` began ends here, once.
        if self.dispatch()? {
            callback((), &mut ());
        }
        Ok(PostAction::Continue)
    }

    fn register(&mut self, poll: &mut Poll, factory: &mut TokenFactory) -> calloop::Result<()> {
        self.token = Some(factory.token());
        self.epoll.register(poll, factory)
    }

    fn reregister(&mut self, poll: &mut Poll, factory: &mut TokenFactory) -> calloop::Result<()> {
        self.token = Some(factory.token());
        self.epoll.reregister(poll, factory)
    }

    fn unregister(&mut self, poll: &mut Poll) -> calloop::Result<()> {
        self.token = None;
        self.epoll.unregister(poll)
    }

    fn before_sleep(&mut self) -> calloop::Result<Option<(Readiness, Token)>> {
        let ready = self.prepare()?;
        Ok(self
            .token
            .filter(|_| ready)
            .map(|token| (Readiness::EMPTY, token)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;
    use std::fs::File;
    use std::io::Write;
    use std::os::unix::net::UnixStream;
    use std::rc::Rc;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;
    use std::time::Instant;

    use calloop::EventLoop;

    /// A source that GLib wakes only when `fd` is readable: its own timeout is an hour.
    fn on_readable(fd: RawFd, fired: &Arc<AtomicBool>) -> glib::Source {
        let fired = fired.clone();
        let source = glib::timeout_source_new(
            Duration::from_secs(3600),
            None,
            glib::Priority::DEFAULT,
            move || {
                fired.store(true, Ordering::SeqCst);
                glib::ControlFlow::Break
            },
        );
        // SAFETY: a live source, not yet attached; GLib only keeps the number to poll it.
        unsafe { glib::ffi::g_source_add_unix_fd(raw_source(&source), fd, G_IO_IN) };
        source.attach(None);
        source
    }

    fn raw_source(source: &glib::Source) -> *mut glib::ffi::GSource {
        source.to_glib_none().0
    }

    /// Runs the loop until `fired`, for at most two seconds, each turn willing to wait five.
    fn until(event_loop: &mut EventLoop<'static, ()>, fired: &AtomicBool) -> bool {
        let deadline = Instant::now() + Duration::from_secs(2);
        while !fired.load(Ordering::SeqCst) && Instant::now() < deadline {
            event_loop
                .dispatch(Duration::from_secs(5), &mut ())
                .expect("dispatch");
        }
        fired.load(Ordering::SeqCst)
    }

    /// One test: GLib's default context has one owner at a time, and the tests of a binary
    /// run on threads of their own.
    #[test]
    fn glib_runs_its_sources_inside_calloop() {
        let mut event_loop: EventLoop<'static, ()> = EventLoop::try_new().expect("event loop");
        event_loop
            .handle()
            .insert_source(GlibSource::new().expect("source"), |_, _, _| {})
            .expect("insert");
        let refused = std::thread::spawn(|| GlibSource::new().is_err())
            .join()
            .expect("thread");
        assert!(refused, "a second thread took the context");

        let idle = Rc::new(Cell::new(false));
        let seen = idle.clone();
        glib::idle_add_local_once(move || seen.set(true));

        let timeout = Rc::new(Cell::new(None));
        let seen = timeout.clone();
        let started = Instant::now();
        glib::timeout_add_local_once(Duration::from_millis(150), move || {
            seen.set(Some(started.elapsed()))
        });

        let future = Rc::new(Cell::new(false));
        let seen = future.clone();
        glib::MainContext::default().spawn_local(async move {
            glib::timeout_future(Duration::from_millis(50)).await;
            seen.set(true);
        });

        let deadline = Instant::now() + Duration::from_secs(5);
        while timeout.get().is_none() && Instant::now() < deadline {
            // A wait longer than the timeout: calloop must wake for GLib's timer.
            event_loop
                .dispatch(Duration::from_secs(5), &mut ())
                .expect("dispatch");
        }
        assert!(idle.get(), "the idle ran");
        assert!(future.get(), "the future completed");
        let elapsed = timeout.get().expect("the timeout ran");
        assert!(
            elapsed >= Duration::from_millis(150) && elapsed < Duration::from_secs(2),
            "{elapsed:?}"
        );

        // A descriptor GLib watched is closed and its number taken by another file, with the
        // same events asked: the new file must wake the loop.
        let (old, old_peer) = UnixStream::pair().expect("pair");
        let number = old.as_raw_fd();
        let first = on_readable(number, &Arc::new(AtomicBool::new(false)));
        event_loop
            .dispatch(Duration::ZERO, &mut ())
            .expect("dispatch");
        first.destroy();
        let (new, mut new_peer) = UnixStream::pair().expect("pair");
        // SAFETY: both descriptors are open; dup2 closes the old file at `number`, which
        // `old` keeps owning, now as the new file.
        let duplicated = unsafe { nix::libc::dup2(new.as_raw_fd(), number) };
        assert_eq!(duplicated, number, "dup2");
        drop((new, old_peer));
        let fired = Arc::new(AtomicBool::new(false));
        let second = on_readable(number, &fired);
        // Attaching and destroying wake GLib's own descriptor, and a turn that wakes for it
        // polls every descriptor: drain it first, so that only the new file can wake the loop.
        for _ in 0..2 {
            event_loop
                .dispatch(Duration::ZERO, &mut ())
                .expect("dispatch");
        }
        new_peer.write_all(b"x").expect("write");
        assert!(until(&mut event_loop, &fired), "the reused number woke the loop");
        second.destroy();
        drop(old);

        // A regular file, which epoll refuses and poll calls always ready.
        let file = File::open(std::env::current_exe().expect("exe")).expect("open");
        let fired = Arc::new(AtomicBool::new(false));
        let file_source = on_readable(file.as_raw_fd(), &fired);
        assert!(until(&mut event_loop, &fired), "a regular file is ready at once");
        file_source.destroy();

        // A closed descriptor, which epoll refuses and poll answers POLLNVAL. A number far
        // above any the tests open, so that no other test takes it meanwhile.
        const CLOSED: RawFd = 900;
        // SAFETY: only asks whether the number names a descriptor.
        let open = unsafe { libc::fcntl(CLOSED, libc::F_GETFD) };
        assert_eq!((open, Errno::last()), (-1, Errno::EBADF), "fd {CLOSED} is closed");
        let fired = Arc::new(AtomicBool::new(false));
        let closed_source = on_readable(CLOSED, &fired);
        assert!(until(&mut event_loop, &fired), "a closed descriptor is reported at once");
        closed_source.destroy();
    }
}
