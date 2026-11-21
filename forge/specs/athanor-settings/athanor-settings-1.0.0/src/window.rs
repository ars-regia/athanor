//! The window of doc_settings.md SE3: the sidebar, its search, and the page beside it, which is
//! the landing page until a page is opened. Only the pages this build draws are listed and
//! opened ([`BUILT`]); the rest of the registry joins as SE26 builds it.

use std::cell::RefCell;
use std::collections::VecDeque;
use std::path::PathBuf;
use std::rc::Rc;

use athanor_sal::driver::Role;
use athanor_sal::{Application, Event, Kind, Shell, SurfaceId, Ui, WidgetId, Wrap};
use athanor_settings_core::registry::{Group, Page, Registry};
use athanor_settings_core::target::{self, Target};
use athanor_settings_core::window_state;
use athanor_trust_state::Badge;

use crate::i18n::tr;
use crate::info::{self, Hardware, Info};
use crate::look::Look;

/// The application id, as the name of Settings' `.desktop` file.
pub const APP_ID: &str = "os.athanor.Settings";

/// The pages this build draws (SE26, step 1).
pub const BUILT: &[&str] = &["about"];

/// A row of a read-only page: its label, and its value when one was read.
type Line = (String, Option<String>);

/// What a `Show` of `text` opens: the target, when it is a page this build draws; otherwise
/// the landing page, with one journal line (SE4).
pub fn resolve(registry: &Registry, text: &str) -> Target {
    match target::parse(registry, text) {
        Ok(target) if target.page().is_none_or(|page| BUILT.contains(&page)) => target,
        Ok(target) => {
            tracing::warn!(%target, "this page is not built yet; the landing page opens");
            Target::Landing
        }
        Err(unknown) => {
            tracing::warn!("{unknown}");
            Target::Landing
        }
    }
}

fn listed(page: &Page) -> bool {
    BUILT.contains(&page.id)
}

/// Settings' window and what it shows.
pub struct Settings {
    registry: Registry,
    look: Look,
    /// The window's widgets until the driver opens them.
    ui: Option<Ui>,
    window: Option<SurfaceId>,
    search: WidgetId,
    nav: WidgetId,
    /// The sidebar's rows and where each leads.
    nav_rows: Vec<(WidgetId, Target)>,
    page: WidgetId,
    /// The page's buttons that lead elsewhere.
    page_links: Vec<(WidgetId, Target)>,
    current: Target,
    info: Rc<RefCell<Info>>,
    system: Option<String>,
    /// Read when About is first drawn: pci.ids alone is over a megabyte.
    hardware: Option<Hardware>,
    /// The targets `Show` was called with since the last turn.
    shows: Rc<RefCell<VecDeque<String>>>,
    /// Where the current target is written (SE5); `None` writes nothing.
    state_dir: Option<PathBuf>,
}

impl Settings {
    pub fn new(
        registry: Registry,
        look: Look,
        rtl: bool,
        first: Target,
        info: Rc<RefCell<Info>>,
        shows: Rc<RefCell<VecDeque<String>>>,
        state_dir: Option<PathBuf>,
    ) -> Self {
        let mut ui = Ui::new(&tr("Settings"), look.theme.clone(), rtl);
        ui.min_width = 860.0;
        ui.min_height = 560.0;
        // ponytail: equal halves until Sal's split view (SL3) gives the sidebar its own width
        // and collapses the two below 600 px.
        let body = ui.add(Ui::ROOT, Kind::Row);
        let side = ui.add(body, Kind::Column);
        let search = ui.field(side, &tr("Search settings"), "");
        let nav = ui.list(side, &tr("Pages"));
        let page = ui.add(body, Kind::Column);
        let mut settings = Self {
            registry,
            look,
            ui: None,
            window: None,
            search,
            nav,
            nav_rows: Vec::new(),
            page,
            page_links: Vec::new(),
            current: first,
            info,
            system: info::system(),
            hardware: None,
            shows,
            state_dir,
        };
        settings.fill_nav(&mut ui, "");
        settings.navigate(&mut ui, first);
        settings.ui = Some(ui);
        settings
    }

    /// The sidebar: every listed page under its group's heading, or the hits of `query`.
    fn fill_nav(&mut self, ui: &mut Ui, query: &str) {
        ui.remove_children(self.nav);
        self.nav_rows.clear();
        if query.trim().is_empty() {
            for group in Group::ALL {
                let pages: Vec<&Page> = self
                    .registry
                    .pages()
                    .iter()
                    .filter(|page| page.group == group && listed(page))
                    .collect();
                if pages.is_empty() {
                    continue;
                }
                if let Some(heading) = group.heading() {
                    ui.list_heading(self.nav, &heading);
                }
                for page in pages {
                    let row = ui.list_row(self.nav, &page.title);
                    self.nav_rows.push((row, Target::Page(page.id)));
                }
            }
        } else {
            for hit in self.registry.search(query) {
                if !listed(hit.page) {
                    continue;
                }
                let (text, target) = match hit.section {
                    Some(section) => (
                        format!("{} \u{203a} {}", hit.page.title, section.title),
                        Target::Section(hit.page.id, section.id),
                    ),
                    None => (hit.page.title.clone(), Target::Page(hit.page.id)),
                };
                let row = ui.list_row(self.nav, &text);
                self.nav_rows.push((row, target));
            }
        }
        ui.layout();
    }

    /// Shows `target` and writes it as the current target (SE5), once its page has drawn: a
    /// page that aborts while it draws is not reopened at every restart.
    fn navigate(&mut self, ui: &mut Ui, target: Target) {
        self.current = target;
        self.draw_page(ui);
        if let Some(dir) = &self.state_dir {
            if let Err(error) = window_state::save(dir, target) {
                tracing::warn!(%error, "cannot write the current target; a restart opens the landing page");
            }
        }
        let page = target.page();
        let row = self
            .nav_rows
            .iter()
            .find(|(_, to)| to.page() == page && page.is_some())
            .map(|(row, _)| *row);
        match row {
            Some(row) => {
                let _changed = ui.select_row(row);
            }
            // Sal clears no selection on its own; a list filled anew has none.
            None => {
                let query = ui.field_text(self.search).unwrap_or_default();
                self.fill_nav(ui, &query);
            }
        }
    }

    fn draw_page(&mut self, ui: &mut Ui) {
        ui.remove_children(self.page);
        self.page_links.clear();
        match self.current.page() {
            Some("about") => self.about(ui),
            _ => self.landing(ui),
        }
        ui.layout();
    }

    fn title(&self, ui: &mut Ui, text: &str) {
        let title = ui.add(self.page, Kind::Label(text.to_owned()));
        self.look.title(ui, title);
    }

    /// The landing page (F-settings-32): the user's card, the device card, and the groups.
    fn landing(&mut self, ui: &mut Ui) {
        let info = self.info.borrow();
        self.title(ui, &tr("Settings"));
        // The card opens Users once that page is built (SE26, step 3); the avatar waits for an
        // image Sal reads from AccountsService's extensionless icon files.
        let user = ui.add(self.page, Kind::Group(tr("Your account")));
        let heading = ui.add(user, Kind::Label(tr("Your account")));
        self.look.heading(ui, heading);
        if let Some(name) = &info.real_name {
            ui.add(user, Kind::Label(name.clone()));
        }

        let device = ui.add(self.page, Kind::Group(tr("This device")));
        let heading = ui.add(device, Kind::Label(tr("This device")));
        self.look.heading(ui, heading);
        let name = info
            .device_name()
            .map_or_else(|| tr("This device"), str::to_owned);
        let card = ui.labelled_button(device, &name, None, 0.0, &name);
        self.page_links.push((card, Target::Page("about")));
        let line: Vec<&str> = [info.model.as_deref(), self.system.as_deref()]
            .into_iter()
            .flatten()
            .collect();
        if !line.is_empty() {
            let note = ui.add(device, Kind::Label(line.join(" \u{b7} ")));
            self.look.note(ui, note);
        }
        drop(info);

        let groups = ui.add(self.page, Kind::Group(tr("Groups")));
        for group in Group::ALL {
            let first = self
                .registry
                .pages()
                .iter()
                .find(|page| page.group == group && listed(page));
            let Some(first) = first else { continue };
            let label = group.heading().unwrap_or_else(|| first.title.clone());
            let button = ui.add(groups, Kind::Button(label));
            self.page_links.push((button, Target::Page(first.id)));
        }
    }

    /// About (SE22), read-only: what the system says of itself with no privilege.
    fn about(&mut self, ui: &mut Ui) {
        let hardware = self.hardware.get_or_insert_with(info::hardware);
        let info = self.info.borrow();
        let title = self
            .registry
            .page("about")
            .map_or_else(|| tr("About"), |page| page.title.clone());
        let mut rows: Vec<(String, Vec<Line>)> = vec![
            (
                tr("Device"),
                vec![
                    (tr("Device name"), info.device_name().map(str::to_owned)),
                    (tr("Manufacturer"), info.vendor.clone()),
                    (tr("Model"), info.model.clone()),
                ],
            ),
            (
                tr("System"),
                vec![
                    (tr("System"), self.system.clone()),
                    (tr("Updates"), Some(trust_text(info.badge()))),
                ],
            ),
        ];
        let mut parts = vec![
            (tr("Processor"), hardware.processor.clone()),
            (tr("Memory"), hardware.memory.clone()),
        ];
        parts.extend(
            hardware
                .graphics
                .iter()
                .map(|name| (tr("Graphics"), Some(name.clone()))),
        );
        parts.push((tr("Disk capacity"), hardware.disk.clone()));
        rows.push((tr("Hardware"), parts));
        drop(info);

        self.title(ui, &title);
        for (name, lines) in rows {
            let group = ui.add(self.page, Kind::Group(name.clone()));
            let heading = ui.add(group, Kind::Label(name));
            self.look.heading(ui, heading);
            // ponytail: two labels side by side until Sal's preference row (SL4) reads them as
            // one control named by its title.
            for (label, value) in lines {
                let Some(value) = value else { continue };
                let row = ui.add(group, Kind::Row);
                let label = ui.add(row, Kind::Label(label));
                self.look.note(ui, label);
                let value = ui.add(row, Kind::Label(value));
                ui.set_wrap(value, Some(Wrap::default()));
            }
        }
    }

    /// The target the sidebar's row or the page's button `id` leads to.
    fn link(&self, id: WidgetId) -> Option<Target> {
        self.nav_rows
            .iter()
            .chain(&self.page_links)
            .find(|(widget, _)| *widget == id)
            .map(|(_, target)| *target)
    }

    /// The window's widgets before the driver opens them, for the tests.
    #[cfg(test)]
    fn ui(&self) -> Option<&Ui> {
        self.ui.as_ref()
    }
}

/// The update trust state as About words it (SH12's badge). One not yet read is not verified.
fn trust_text(badge: Option<Badge>) -> String {
    match badge {
        Some(Badge::Check) => tr("Verified"),
        Some(Badge::Cross) => tr("The last update was refused by the policy"),
        Some(Badge::Attention) | None => tr("Not verified"),
    }
}

impl Application for Settings {
    fn start(&mut self) -> Vec<(Ui, Role)> {
        let role = Role::Window {
            app_id: APP_ID.into(),
        };
        self.ui.take().map(|ui| (ui, role)).into_iter().collect()
    }

    fn drawn(&mut self, _: &mut Shell, surface: SurfaceId) {
        self.window.get_or_insert(surface);
    }

    fn event(&mut self, shell: &mut Shell, surface: SurfaceId, event: Event) {
        let Some(ui) = shell.ui_mut(surface) else {
            return;
        };
        match event {
            Event::Edited(id) if id == self.search => {
                let query = ui.field_text(id).unwrap_or_default();
                self.fill_nav(ui, &query);
            }
            Event::Activated(id) if id == self.search => {
                if let Some(&(_, target)) = self.nav_rows.first() {
                    self.navigate(ui, target);
                }
            }
            Event::Selected(id) | Event::Clicked(id) => {
                if let Some(target) = self.link(id) {
                    if target != self.current {
                        self.navigate(ui, target);
                    }
                }
            }
            _ => {}
        }
    }

    /// Applies the `Show` calls and the answers of the services that came since.
    fn turn(&mut self, shell: &mut Shell) {
        let Some(window) = self.window else {
            return;
        };
        let Some(ui) = shell.ui_mut(window) else {
            return;
        };
        // ponytail: a Show on the open window navigates without raising it, until Sal raises a
        // window with the caller's activation token (SL2).
        let shows: Vec<String> = self.shows.borrow_mut().drain(..).collect();
        for text in &shows {
            let target = resolve(&self.registry, text);
            self.navigate(ui, target);
        }
        let changed = std::mem::take(&mut self.info.borrow_mut().changed);
        if changed && shows.is_empty() {
            self.draw_page(ui);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use athanor_style::appearance::Appearance;

    fn settings(first: Target) -> Settings {
        Settings::new(
            Registry::new(),
            crate::look::look(&Appearance::default()),
            false,
            first,
            Rc::default(),
            Rc::default(),
            None,
        )
    }

    #[test]
    fn a_target_that_is_unknown_or_not_built_opens_the_landing_page() {
        let registry = Registry::new();
        assert_eq!(resolve(&registry, ""), Target::Landing);
        assert_eq!(resolve(&registry, "about"), Target::Page("about"));
        assert_eq!(
            resolve(&registry, "network"),
            Target::Landing,
            "not built yet"
        );
        assert_eq!(
            resolve(&registry, "notifications/schedule"),
            Target::Landing
        );
        assert_eq!(resolve(&registry, "no-such-page"), Target::Landing);
        assert_eq!(resolve(&registry, "about/no-such-section"), Target::Landing);
        assert_eq!(
            resolve(&registry, &Target::Page("network").to_string()),
            Target::Landing,
            "a state file left by a build that drew Network reopens the landing page"
        );
    }

    #[test]
    fn the_sidebar_lists_the_built_pages_and_the_search_finds_them() {
        let mut settings = settings(Target::Landing);
        assert_eq!(
            settings
                .nav_rows
                .iter()
                .map(|(_, t)| *t)
                .collect::<Vec<_>>(),
            vec![Target::Page("about")]
        );
        let mut ui = settings.ui.take().expect("not started");
        settings.fill_nav(&mut ui, "network");
        assert!(settings.nav_rows.is_empty(), "Network is not built yet");
        settings.fill_nav(&mut ui, "ABOUT");
        assert_eq!(settings.nav_rows.len(), 1);
        settings.fill_nav(&mut ui, "");
        assert_eq!(settings.nav_rows.len(), 1);
    }

    #[test]
    fn the_landing_page_leads_to_about_and_about_leads_nowhere() {
        let mut settings = settings(Target::Landing);
        assert!(settings.ui().is_some());
        assert!(settings
            .page_links
            .iter()
            .any(|(_, target)| *target == Target::Page("about")));
        let mut ui = settings.ui.take().expect("not started");
        settings.navigate(&mut ui, Target::Page("about"));
        assert_eq!(settings.current, Target::Page("about"));
        assert!(settings.page_links.is_empty(), "About is read-only");
    }

    #[test]
    fn every_navigation_writes_the_target_a_restart_reopens() {
        // A new directory of this run alone: `create_dir` refuses a path that already exists.
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("a clock after 1970")
            .as_nanos();
        let dir = std::env::temp_dir().join(format!(
            "athanor-settings-state-{}-{nanos}",
            std::process::id()
        ));
        std::fs::create_dir(&dir).expect("a scratch directory");
        let mut settings = settings(Target::Landing);
        settings.state_dir = Some(dir.clone());
        let mut ui = settings.ui.take().expect("not started");
        settings.navigate(&mut ui, Target::Page("about"));
        let registry = Registry::new();
        assert_eq!(
            window_state::load(&registry, &dir).expect("readable"),
            Some(Target::Page("about"))
        );
        settings.navigate(&mut ui, Target::Landing);
        assert_eq!(
            window_state::load(&registry, &dir).expect("readable"),
            Some(Target::Landing)
        );
        std::fs::remove_dir_all(&dir).expect("cleaned up");
    }

    #[test]
    fn the_landing_page_selects_no_sidebar_row() {
        let mut settings = settings(Target::Page("about"));
        let mut ui = settings.ui.take().expect("not started");
        assert!(ui.selected(settings.nav).is_some(), "About is selected");
        settings.navigate(&mut ui, Target::Landing);
        assert_eq!(ui.selected(settings.nav), None);
    }

    #[test]
    fn a_long_hardware_name_wraps_inside_the_window() {
        let mut settings = settings(Target::Landing);
        settings.hardware = Some(info::Hardware {
            processor: None,
            memory: None,
            graphics: vec!["NVIDIA Corporation AD103 [GeForce RTX 4080 SUPER] ".repeat(4)],
            disk: None,
        });
        let mut ui = settings.ui.take().expect("not started");
        let (width, _) = ui.layout();
        settings.navigate(&mut ui, Target::Page("about"));
        assert_eq!(ui.layout().0, width, "the window keeps its width");
    }

    #[test]
    fn a_trust_state_not_read_is_not_verified() {
        assert_eq!(trust_text(None), trust_text(Some(Badge::Attention)));
        assert_ne!(trust_text(Some(Badge::Check)), trust_text(None));
    }
}
