//! GLib's default main context run inside a calloop loop, for a surface drawn without GTK.
//! The launch path awaits GIO's D-Bus calls and the client reads its queue from a GLib
//! source; an application whose loop is calloop's runs GLib's as one of its sources.
//!
//! Each turn of the calloop loop is one iteration of GLib's, split as
//! `g_main_context_iteration` splits it. Before calloop sleeps, `prepare` and `query` name
//! the descriptors and the timeout GLib would wait on; an epoll set of our own holds those
//! descriptors and a timer stands for the timeout, so calloop's one poll waits on both
//! loops. When the set is ready, or GLib had something ready at once, `check` and
//! `dispatch` run GLib's sources. Nothing but calloop's poll ever blocks.

#![allow(unsafe_code)]

use std::io;
use std::marker::PhantomData;
use std::os::fd::{AsFd, BorrowedFd, RawFd};
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
    epoll: Generic<EpollFd>,
    timer: TimerFd,
    /// What the last `query` asked to poll, and the descriptors the epoll set holds for it.
    fds: Vec<GPollFD>,
    watched: Vec<(RawFd, EpollFlags)>,
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
        let context = glib::MainContext::default();
        // SAFETY: the pointer is the live context `context` holds a reference to.
        if unsafe { g_main_context_acquire(raw(&context)) } == 0 {
            return Err(io::Error::other(
                "GLib's default main context belongs to another thread",
            ));
        }
        let epoll = Epoll::new(EpollCreateFlags::EPOLL_CLOEXEC)?;
        let timer = TimerFd::new(
            ClockId::CLOCK_MONOTONIC,
            TimerFlags::TFD_CLOEXEC | TimerFlags::TFD_NONBLOCK,
        )?;
        epoll.add(&timer, EpollEvent::new(EpollFlags::EPOLLIN, 0))?;
        Ok(Self {
            context,
            epoll: Generic::new(EpollFd(epoll), Interest::READ, Mode::Level),
            timer,
            fds: Vec::new(),
            watched: Vec::new(),
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
        self.watch()?;
        match u64::try_from(timeout) {
            Ok(0) | Err(_) => self.timer.unset()?,
            Ok(ms) => self.timer.set(
                Expiration::OneShot(TimeSpec::from_duration(Duration::from_millis(ms))),
                TimerSetTimeFlags::empty(),
            )?,
        }
        Ok(ready || timeout == 0)
    }

    /// Makes the epoll set hold the descriptors of the last query, once each.
    fn watch(&mut self) -> io::Result<()> {
        let mut wanted: Vec<(RawFd, EpollFlags)> = Vec::with_capacity(self.fds.len());
        for fd in &self.fds {
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
        wanted.sort_unstable_by_key(|&(fd, _)| fd);
        if wanted == self.watched {
            return Ok(());
        }
        let epoll = &self.epoll.get_ref().0;
        for &(fd, _) in &self.watched {
            // SAFETY: only names the descriptor to the kernel; one its owner closed since is
            // gone from the set already and answers ENOENT or EBADF.
            match epoll.delete(unsafe { BorrowedFd::borrow_raw(fd) }) {
                Ok(()) | Err(Errno::ENOENT | Errno::EBADF) => {}
                Err(err) => return Err(err.into()),
            }
        }
        self.watched.clear();
        for &(fd, flags) in &wanted {
            // SAFETY: GLib's query names descriptors its sources hold open.
            let borrowed = unsafe { BorrowedFd::borrow_raw(fd) };
            epoll.add(borrowed, EpollEvent::new(flags, 0))?;
            self.watched.push((fd, flags));
        }
        Ok(())
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
    use std::rc::Rc;
    use std::time::Instant;

    use calloop::EventLoop;

    /// One test: GLib's default context has one owner at a time, and the tests of a binary
    /// run on threads of their own.
    #[test]
    fn glib_runs_its_idles_timeouts_and_futures_inside_calloop() {
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
    }
}
