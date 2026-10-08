//! What a page asks of the surface that holds it.

use std::rc::Rc;

/// A page that needs the person (a password, a pairing to confirm) asks its surface to show
/// it, and to hide it when the person has answered; it picks the surface where it will be seen.
#[derive(Clone)]
pub struct Host {
    open: Rc<dyn Fn()>,
    close: Rc<dyn Fn()>,
    usable: Rc<dyn Fn() -> bool>,
}

impl Host {
    /// `open` shows the page, and does nothing when it is shown already; `close` hides it;
    /// `usable` tells whether this surface is on screen, so a request goes to one that the
    /// person can see.
    pub fn new(
        open: impl Fn() + 'static,
        close: impl Fn() + 'static,
        usable: impl Fn() -> bool + 'static,
    ) -> Host {
        Host {
            open: Rc::new(open),
            close: Rc::new(close),
            usable: Rc::new(usable),
        }
    }

    pub(crate) fn open(&self) {
        (self.open)();
    }

    pub(crate) fn close(&self) {
        (self.close)();
    }

    pub(crate) fn usable(&self) -> bool {
        (self.usable)()
    }
}
