//! The pages of doc_settings.md SE3, one file each. A page joins the build with its file and one
//! line of [`PAGES`]: the window lists, opens and draws only the pages named there, and takes
//! each page's title and sections from the registry of athanor-settings-core.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use athanor_controls::sal::widgets::Look as ControlsLook;
use athanor_controls::sal::Turn;
use athanor_controls::Services;
use athanor_sal::{Event, Image, Ui, WidgetId};
use athanor_settings_core::registry::Registry;

use crate::info::Info;
use crate::look::Look;

pub(crate) mod about;

/// Dirty flag shared with a page's asynchronous callbacks (GLib main context, D-Bus replies) so
/// the window redraws or refreshes the active page on the next turn.
#[derive(Clone, Default)]
pub struct Notifier(Rc<Cell<bool>>);

impl Notifier {
    /// Marks the active page dirty for the next window turn.
    pub fn notify(&self) {
        self.0.set(true);
    }

    /// Reads and clears the dirty flag.
    pub fn take(&self) -> bool {
        self.0.replace(false)
    }
}

/// What the window lends a page while it checks presence, draws, handles an event or runs a turn.
pub struct Ctx<'a> {
    pub look: &'a Look,
    pub registry: &'a Registry,
    pub info: &'a RefCell<Info>,
    /// The system's name and version, from os-release.
    pub system: Option<&'a str>,
    /// Shared service models from `athanor-services`, when the Tokio runtime started.
    pub services: Option<&'a Services>,
    /// Marks the active page dirty from an asynchronous callback.
    pub dirty: &'a Notifier,
}

impl<'a> Ctx<'a> {
    /// Adapts Settings' [`Look`] to the look expected by embedded `athanor-controls` Sal pages.
    pub fn controls_look(
        &self,
        icon: impl FnMut(&'static str) -> Option<Image> + 'a,
        icon_size: f32,
    ) -> ControlsLook<'a> {
        ControlsLook {
            icon: Box::new(icon),
            icon_size,
            caption: self.look.caption(),
            secondary: self.look.secondary(),
            radio_gap: None,
        }
    }
}

/// A page of the window. The window makes one of each at start and keeps it for the life of
/// the process (SE5), so what a page has read survives the window's close.
pub trait Page {
    /// Whether this page should be listed in the sidebar and landing groups (SE3).
    fn present(&self, _ctx: &Ctx) -> bool {
        true
    }

    /// Draws the page into `page`, the column beside the sidebar, which the window has emptied.
    fn draw(&mut self, ctx: &Ctx, ui: &mut Ui, page: WidgetId);

    /// An event of the page's own widgets; the window keeps the search and the links. A
    /// read-only page has none.
    fn event(&mut self, _ctx: &Ctx, _ui: &mut Ui, _event: &Event) {}

    /// Applies any asynchronous model updates that arrived since the last frame.
    fn turn(&mut self, _ctx: &Ctx, _ui: &mut Ui) -> Turn {
        Turn::Unchanged
    }
}

/// A page of this build: its id in the registry (SE4) and how it is made.
pub struct PageDef {
    pub id: &'static str,
    pub new: fn() -> Box<dyn Page>,
}

/// The pages this build draws (SE26), in no particular order: the registry orders the sidebar.
pub const PAGES: &[PageDef] = &[PageDef {
    id: "about",
    new: about::new,
}];

/// Whether this build draws the page `id`.
pub fn built(id: &str) -> bool {
    PAGES.iter().any(|page| page.id == id)
}

#[cfg(test)]
pub(crate) mod test_harness {
    use athanor_sal::Kind;
    use athanor_style::appearance::Appearance;

    use super::*;

    /// In-memory harness for driving a [`Page`] without a display (SE25).
    pub struct Harness {
        pub look: Look,
        pub registry: Registry,
        pub info: RefCell<Info>,
        pub dirty: Notifier,
        pub ui: Ui,
        pub column: WidgetId,
    }

    impl Harness {
        pub fn new() -> Self {
            let look = crate::look::look(&Appearance::default());
            let mut ui = Ui::new("Settings", look.theme.clone(), false);
            let column = ui.add(Ui::ROOT, Kind::Column);
            Self {
                look,
                registry: Registry::new(),
                info: RefCell::default(),
                dirty: Notifier::default(),
                ui,
                column,
            }
        }

        pub fn ctx(&self) -> Ctx<'_> {
            Ctx {
                look: &self.look,
                registry: &self.registry,
                info: &self.info,
                system: Some("Athanor 1.0"),
                services: None,
                dirty: &self.dirty,
            }
        }

        pub fn draw(&mut self, page: &mut dyn Page) {
            self.ui.remove_children(self.column);
            let ctx = Ctx {
                look: &self.look,
                registry: &self.registry,
                info: &self.info,
                system: Some("Athanor 1.0"),
                services: None,
                dirty: &self.dirty,
            };
            page.draw(&ctx, &mut self.ui, self.column);
            self.ui.layout();
        }

        pub fn event(&mut self, page: &mut dyn Page, event: &Event) {
            let ctx = Ctx {
                look: &self.look,
                registry: &self.registry,
                info: &self.info,
                system: Some("Athanor 1.0"),
                services: None,
                dirty: &self.dirty,
            };
            page.event(&ctx, &mut self.ui, event);
        }

        pub fn turn(&mut self, page: &mut dyn Page) -> Turn {
            let turn = {
                let ctx = Ctx {
                    look: &self.look,
                    registry: &self.registry,
                    info: &self.info,
                    system: Some("Athanor 1.0"),
                    services: None,
                    dirty: &self.dirty,
                };
                page.turn(&ctx, &mut self.ui)
            };
            match turn {
                Turn::Rebuild => self.draw(page),
                Turn::Updated => {
                    self.ui.layout();
                }
                Turn::Unchanged => {
                    if self.dirty.take() {
                        self.draw(page);
                    }
                }
            }
            turn
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_page_of_the_build_is_a_page_of_the_registry_once() {
        let registry = Registry::new();
        for (index, page) in PAGES.iter().enumerate() {
            assert!(
                registry.page(page.id).is_some(),
                "{} is not in the registry",
                page.id
            );
            assert!(
                !PAGES.iter().skip(index + 1).any(|other| other.id == page.id),
                "{} is built twice",
                page.id
            );
        }
    }

    #[test]
    fn notifier_sets_and_clears_dirty_flag() {
        let notifier = Notifier::default();
        assert!(!notifier.take());
        notifier.notify();
        assert!(notifier.take());
        assert!(!notifier.take());
    }

    #[test]
    fn controls_look_carries_settings_caption_and_secondary_ink() {
        let harness = test_harness::Harness::new();
        let ctx = harness.ctx();
        let clook = ctx.controls_look(|_| None, 16.0);
        assert_eq!(clook.icon_size, 16.0);
        assert_eq!(clook.caption, harness.look.caption());
        assert_eq!(clook.secondary, harness.look.secondary());
    }

    #[test]
    fn harness_draws_about_and_tracks_turns() {
        let mut harness = test_harness::Harness::new();
        let mut about = about::About {
            hardware: Some(crate::info::Hardware {
                processor: None,
                memory: None,
                graphics: Vec::new(),
                disk: None,
            }),
        };
        assert!(about.present(&harness.ctx()));
        harness.draw(&mut about);
        assert!(!harness.ui.widget(harness.column).children.is_empty());
        harness.event(&mut about, &Event::Clicked(harness.column));
        harness.dirty.notify();
        assert_eq!(harness.turn(&mut about), Turn::Unchanged);
        assert!(!harness.dirty.take(), "turn cleared the dirty flag");
    }
}
