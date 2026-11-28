//! The pages of doc_settings.md SE3, one file each. A page joins the build with its file and one
//! line of [`PAGES`]: the window lists, opens and draws only the pages named there, and takes
//! each page's title and sections from the registry of athanor-settings-core.

use std::cell::RefCell;

use athanor_sal::{Event, Ui, WidgetId};
use athanor_settings_core::registry::Registry;

use crate::info::Info;
use crate::look::Look;

pub(crate) mod about;

/// What the window lends a page while it draws or handles an event.
pub struct Ctx<'a> {
    pub look: &'a Look,
    pub registry: &'a Registry,
    pub info: &'a RefCell<Info>,
    /// The system's name and version, from os-release.
    pub system: Option<&'a str>,
}

/// A page of the window. The window makes one of each at start and keeps it for the life of
/// the process (SE5), so what a page has read survives the window's close.
pub trait Page {
    /// Draws the page into `page`, the column beside the sidebar, which the window has emptied.
    fn draw(&mut self, ctx: &Ctx, ui: &mut Ui, page: WidgetId);

    /// An event of the page's own widgets; the window keeps the search and the links. A
    /// read-only page has none.
    fn event(&mut self, _ctx: &Ctx, _ui: &mut Ui, _event: &Event) {}
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
mod tests {
    use super::*;

    #[test]
    fn every_page_of_the_build_is_a_page_of_the_registry_once() {
        let registry = Registry::new();
        for (index, page) in PAGES.iter().enumerate() {
            assert!(registry.page(page.id).is_some(), "{} is not in the registry", page.id);
            assert!(
                !PAGES.iter().skip(index + 1).any(|other| other.id == page.id),
                "{} is built twice",
                page.id
            );
        }
    }
}
