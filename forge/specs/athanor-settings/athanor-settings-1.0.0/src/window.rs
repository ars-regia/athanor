//! The window of doc_settings.md SE3: the sidebar, its search, and the page beside it, which is
//! the landing page until a page is opened. Only the pages this build draws are listed and
//! opened ([`pages::PAGES`]); the rest of the registry joins as SE26 builds it.

use std::cell::RefCell;
use std::collections::VecDeque;
use std::path::PathBuf;
use std::rc::Rc;

use athanor_controls::sal::Turn;
use athanor_controls::Services;
use athanor_sal::driver::Role;
use athanor_sal::{Application, Event, Kind, Shell, SurfaceId, Ui, WidgetId, WindowButton};
use athanor_settings_core::registry::{Group, Page, Registry};
use athanor_settings_core::target::{self, Target};
use athanor_settings_core::window_state;
use athanor_style::appearance::Appearance;

use crate::i18n::tr;
use crate::info::{self, Info};
use crate::look::{self, Look};
use crate::pages::{self, Ctx, Notifier};

/// The application id, as the name of Settings' `.desktop` file.
pub const APP_ID: &str = "os.athanor.Settings";

/// The target of the oldest queued `Show`, which leaves the queue.
pub fn take_show(registry: &Registry, shows: &RefCell<VecDeque<String>>) -> Option<Target> {
    let text = shows.borrow_mut().pop_front()?;
    Some(resolve(registry, &text))
}

/// What a `Show` of `text` opens: the target, when it is a page this build draws; otherwise
/// the landing page, with one journal line (SE4).
pub fn resolve(registry: &Registry, text: &str) -> Target {
    match target::parse(registry, text) {
        Ok(target) if target.page().is_none_or(pages::built) => target,
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

/// The first page the window shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum First {
    /// The target of the `Show` that started the run, built at once.
    Show(Target),
    /// The target a crashed run left (SE5), built only once the compositor has answered, with
    /// window_state's one-shot marker around its first frame.
    Reopen(Target),
}

/// The window's frame: the sidebar, with its search and its list of pages, and the page beside
/// it, returned as the window, the search, the list and the page.
fn frame(look: &Look, rtl: bool) -> (Ui, WidgetId, WidgetId, WidgetId) {
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
    (ui, search, nav, page)
}

/// Settings' window and what it shows. The process outlives the window (SE5): a `Show` while
/// none is open builds the next one.
pub struct Settings {
    registry: Registry,
    look: Look,
    rtl: bool,
    /// The window's widgets until the driver opens them.
    ui: Option<Ui>,
    window: Option<SurfaceId>,
    /// The window was closed, and the next `Show` opens a new one.
    closed: bool,
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
    services: Option<Services>,
    dirty: Notifier,
    /// Every page of the build, made once and kept while the process lives (SE5).
    pages: Vec<(&'static str, Box<dyn pages::Page>)>,
    /// The targets `Show` was called with since the last turn.
    shows: Rc<RefCell<VecDeque<String>>>,
    /// Where the current target is written (SE5); `None` writes nothing.
    state_dir: Option<PathBuf>,
    /// The reopened page has not drawn its first frame yet.
    reopening: bool,
}

impl Settings {
    pub fn new(
        registry: Registry,
        look: Look,
        rtl: bool,
        first: First,
        info: Rc<RefCell<Info>>,
        shows: Rc<RefCell<VecDeque<String>>>,
        state_dir: Option<PathBuf>,
    ) -> Self {
        let (mut ui, search, nav, page) = frame(&look, rtl);
        let mut settings = Self {
            registry,
            look,
            rtl,
            ui: None,
            window: None,
            closed: false,
            search,
            nav,
            nav_rows: Vec::new(),
            page,
            page_links: Vec::new(),
            current: Target::Landing,
            info,
            system: info::system(),
            services: None,
            dirty: Notifier::default(),
            pages: pages::PAGES.iter().map(|page| (page.id, (page.new)())).collect(),
            shows,
            state_dir,
            reopening: false,
        };
        settings.fill_nav(&mut ui, "");
        match first {
            First::Show(target) => settings.navigate(&mut ui, target),
            First::Reopen(target) => {
                settings.current = target;
                settings.reopening = true;
            }
        }
        settings.ui = Some(ui);
        settings
    }

    /// Attaches the shared `athanor-services` handle for pages that read or control system models.
    pub fn with_services(mut self, services: Option<Services>) -> Self {
        self.services = services;
        self
    }

    fn listed(&self, page: &Page) -> bool {
        if !pages::built(page.id) {
            return false;
        }
        let ctx = self.ctx();
        self.pages
            .iter()
            .find(|(id, _)| *id == page.id)
            .is_some_and(|(_, built)| built.present(&ctx))
    }

    fn ctx(&self) -> Ctx<'_> {
        Ctx {
            look: &self.look,
            registry: &self.registry,
            info: &self.info,
            system: self.system.as_deref(),
            services: self.services.as_ref(),
            dirty: &self.dirty,
        }
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
                    .filter(|page| page.group == group && self.listed(page))
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
                if !self.listed(hit.page) {
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
        let column = self.page;
        if !self.on_page(|page, ctx| page.draw(ctx, ui, column)) {
            self.landing(ui);
        }
        ui.layout();
    }

    /// Runs `f` on the open page with what the window lends it; `false` when the window shows
    /// the landing page.
    fn on_page(&mut self, f: impl FnOnce(&mut dyn pages::Page, &Ctx)) -> bool {
        let Some(id) = self.current.page() else {
            return false;
        };
        let ctx = Ctx {
            look: &self.look,
            registry: &self.registry,
            info: &self.info,
            system: self.system.as_deref(),
            services: self.services.as_ref(),
            dirty: &self.dirty,
        };
        let Some((_, page)) = self.pages.iter_mut().find(|(page, _)| *page == id) else {
            return false;
        };
        if !page.present(&ctx) {
            return false;
        }
        f(page.as_mut(), &ctx);
        true
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
                .find(|page| page.group == group && self.listed(page));
            let Some(first) = first else { continue };
            let label = group.heading().unwrap_or_else(|| first.title.clone());
            let button = ui.add(groups, Kind::Button(label));
            self.page_links.push((button, Target::Page(first.id)));
        }
    }

    /// Builds the reopened page, called once the compositor has answered: the marker first, so
    /// that a page that aborts while it draws is not reopened by the next start (SE5).
    fn begin_reopen(&mut self, ui: &mut Ui) {
        if !self.reopening {
            return;
        }
        // Accepted: a compositor that restarts between the marker and the first frame leaves
        // the marker, so the next start opens the landing page, not this one.
        if let Some(dir) = &self.state_dir {
            if let Err(error) = window_state::mark(dir) {
                tracing::warn!(%error, "cannot mark the reopened page; a crash drawing it reopens it again");
            }
        }
        self.navigate(ui, self.current);
    }

    /// The reopened page drew its first frame: the marker goes.
    fn end_reopen(&mut self) {
        if !std::mem::take(&mut self.reopening) {
            return;
        }
        if let Some(dir) = &self.state_dir {
            if let Err(error) = window_state::unmark(dir) {
                tracing::warn!(%error, "cannot remove the reopen marker; the next start opens the landing page");
            }
        }
    }

    /// The window closed and the process stays (SE5): its state goes, so that a crash with no
    /// window open reopens none, and with it the marker of a reopened page closed before its
    /// first frame.
    fn close_window(&mut self) {
        self.window = None;
        self.closed = true;
        self.reopening = false;
        if let Some(dir) = &self.state_dir {
            if let Err(error) = window_state::clear(dir) {
                tracing::warn!(%error, "cannot clear the window state; a restart reopens it");
            }
        }
    }

    /// The next window, at `target`: its widgets built anew, the registry and what was read of
    /// the hardware kept from the first (SE5).
    fn window_at(&mut self, target: Target) -> Ui {
        let (mut ui, search, nav, page) = frame(&self.look, self.rtl);
        (self.search, self.nav, self.page) = (search, nav, page);
        self.closed = false;
        self.fill_nav(&mut ui, "");
        self.navigate(&mut ui, target);
        ui
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

/// Whether a Close of `surface` closes Settings' window (SE5): the window has no id before its
/// first frame, so any surface is it then; afterwards only the one it drew on. Generic, as
/// `SurfaceId` has no constructor outside Sal.
fn closes_window<T: PartialEq>(window: Option<T>, surface: T) -> bool {
    window.is_none_or(|window| window == surface)
}

impl Application for Settings {
    fn start(&mut self) -> Vec<(Ui, Role)> {
        let role = Role::Window {
            app_id: APP_ID.into(),
        };
        let Some(mut ui) = self.ui.take() else {
            return Vec::new();
        };
        self.begin_reopen(&mut ui);
        vec![(ui, role)]
    }

    /// Resident (SE5): the window's Close closes the window, not the run.
    fn close_ends_run(&self) -> bool {
        false
    }

    fn drawn(&mut self, _: &mut Shell, surface: SurfaceId) {
        self.window.get_or_insert(surface);
        self.end_reopen();
    }

    fn event(&mut self, shell: &mut Shell, surface: SurfaceId, event: Event) {
        if event == Event::Window(WindowButton::Close) {
            shell.close(surface);
            if closes_window(self.window, surface) {
                self.close_window();
            }
            return;
        }
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
            Event::Selected(id) | Event::Clicked(id) if self.link(id).is_some() => {
                if let Some(target) = self.link(id) {
                    if target != self.current {
                        self.navigate(ui, target);
                    }
                }
            }
            event => {
                self.on_page(|page, ctx| page.event(ctx, ui, &event));
            }
        }
    }

    /// Opens the next window at the first `Show` since the last one closed, then applies the
    /// `Show` calls and the answers of the services that came since.
    fn turn(&mut self, shell: &mut Shell) {
        if self.closed {
            let Some(target) = take_show(&self.registry, &self.shows) else {
                return;
            };
            // Read again, as each start read them: they may have changed while no window was open.
            // The last window's answers go first, so the new one never draws a trust state or a
            // name read before it, and shows "Not verified" until the service answers again.
            self.look = look::look(&Appearance::read());
            *self.info.borrow_mut() = Info::default();
            info::fetch(&self.info);
            let ui = self.window_at(target);
            match shell.open_window(ui, APP_ID) {
                Ok(window) => self.window = Some(window),
                Err(error) => return shell.fail(error),
            }
        }
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
        let dirty = self.dirty.take();
        let mut page_turn = Turn::Unchanged;
        self.on_page(|page, ctx| {
            page_turn = page.turn(ctx, ui);
        });
        if shows.is_empty() {
            match page_turn {
                Turn::Rebuild => self.draw_page(ui),
                Turn::Updated => {
                    if changed || dirty {
                        self.draw_page(ui);
                    } else {
                        ui.layout();
                    }
                }
                Turn::Unchanged => {
                    if changed || dirty {
                        self.draw_page(ui);
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use athanor_style::appearance::Appearance;

    impl Settings {
        /// About as if it had read `hardware`, so that it reads nothing of the host.
        fn about(&mut self, hardware: info::Hardware) {
            let about = pages::about::About {
                hardware: Some(hardware),
            };
            if let Some((_, page)) = self.pages.iter_mut().find(|(id, _)| *id == "about") {
                *page = Box::new(about);
            }
        }
    }

    fn settings(first: Target) -> Settings {
        Settings::new(
            Registry::new(),
            crate::look::look(&Appearance::default()),
            false,
            First::Show(first),
            Rc::default(),
            Rc::default(),
            None,
        )
    }

    #[test]
    fn a_show_queued_while_the_window_closes_is_taken_once_in_order() {
        let registry = Registry::new();
        let shows = RefCell::new(VecDeque::from(["about".to_owned(), String::new()]));
        assert_eq!(take_show(&registry, &shows), Some(Target::Page("about")));
        assert_eq!(take_show(&registry, &shows), Some(Target::Landing));
        assert_eq!(take_show(&registry, &shows), None);
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
        settings.about(info::Hardware {
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

    /// The widgets under `id`, one line each, indented by depth: what each is and shows, and
    /// its text's weight and scale. Rects are left out: they follow the fonts installed.
    fn outline(ui: &Ui, id: WidgetId, depth: usize, out: &mut String) {
        let widget = ui.widget(id);
        let style = widget.text_style();
        let kind = match &widget.kind {
            Kind::Label(text) => format!("Label({text:?})"),
            Kind::Group(name) => format!("Group({name:?})"),
            Kind::Row => "Row".to_owned(),
            _ => "another kind".to_owned(),
        };
        out.push_str(&format!(
            "{}{kind} {}@{:.3}\n",
            "  ".repeat(depth),
            style.weight,
            style.scale
        ));
        for &child in &widget.children {
            outline(ui, child, depth + 1, out);
        }
    }

    const ABOUT: &str = "\
Label(\"About\") 700@1.360
Group(\"Device\") 400@1.000
  Label(\"Device\") 600@0.885
  Row 400@1.000
    Label(\"Device name\") 400@0.885
    Label(\"Desk\") 400@1.000
  Row 400@1.000
    Label(\"Manufacturer\") 400@0.885
    Label(\"Maker\") 400@1.000
  Row 400@1.000
    Label(\"Model\") 400@0.885
    Label(\"Model 7\") 400@1.000
Group(\"System\") 400@1.000
  Label(\"System\") 600@0.885
  Row 400@1.000
    Label(\"System\") 400@0.885
    Label(\"Athanor 1.0\") 400@1.000
  Row 400@1.000
    Label(\"System integrity\") 400@0.885
    Label(\"Not verified\") 400@1.000
Group(\"Hardware\") 400@1.000
  Label(\"Hardware\") 600@0.885
  Row 400@1.000
    Label(\"Processor\") 400@0.885
    Label(\"CPU 8 cores\") 400@1.000
  Row 400@1.000
    Label(\"Graphics\") 400@0.885
    Label(\"GPU A\") 400@1.000
  Row 400@1.000
    Label(\"Graphics\") 400@0.885
    Label(\"GPU B\") 400@1.000
  Row 400@1.000
    Label(\"Disk capacity\") 400@0.885
    Label(\"512 GB\") 400@1.000
";

    #[test]
    fn about_draws_its_rows_in_order_and_skips_the_values_not_read() {
        let mut settings = settings(Target::Landing);
        settings.system = Some("Athanor 1.0".into());
        settings.about(info::Hardware {
            processor: Some("CPU 8 cores".into()),
            memory: None,
            graphics: vec!["GPU A".into(), "GPU B".into()],
            disk: Some("512 GB".into()),
        });
        {
            let mut info = settings.info.borrow_mut();
            info.pretty_hostname = Some("Desk".into());
            info.vendor = Some("Maker".into());
            info.model = Some("Model 7".into());
        }
        let mut ui = settings.ui.take().expect("not started");
        settings.navigate(&mut ui, Target::Page("about"));
        let mut drawn = String::new();
        for &child in &ui.widget(settings.page).children {
            outline(&ui, child, 0, &mut drawn);
        }
        assert_eq!(drawn, ABOUT);
    }

    #[test]
    fn a_reopened_page_is_built_after_the_connection_inside_the_marker() {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("a clock after 1970")
            .as_nanos();
        let dir = std::env::temp_dir().join(format!(
            "athanor-settings-reopen-{}-{nanos}",
            std::process::id()
        ));
        std::fs::create_dir(&dir).expect("a scratch directory");
        let registry = Registry::new();
        window_state::save(&dir, Target::Page("about")).expect("a crashed run's state");
        let mut settings = Settings::new(
            Registry::new(),
            crate::look::look(&Appearance::default()),
            false,
            First::Reopen(Target::Page("about")),
            Rc::default(),
            Rc::default(),
            Some(dir.clone()),
        );
        let marker = dir.join(window_state::MARKER);
        // Before the compositor answers, nothing is built and the state is kept.
        assert!(!marker.exists());
        assert!(settings.page_links.is_empty());
        assert_eq!(
            window_state::load(&registry, &dir).expect("readable"),
            Some(Target::Page("about"))
        );
        let mut started = settings.start();
        assert_eq!(started.len(), 1);
        assert!(marker.exists(), "the reopened page is drawn inside the marker");
        let (ui, _) = started.first_mut().expect("the window");
        assert!(ui.selected(settings.nav).is_some(), "About is built and selected");
        settings.end_reopen();
        assert!(!marker.exists(), "its first frame removes the marker");
        std::fs::remove_dir_all(&dir).expect("cleaned up");
    }

    #[test]
    fn a_closed_window_keeps_the_process_and_the_next_show_builds_a_new_one() {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("a clock after 1970")
            .as_nanos();
        let dir = std::env::temp_dir().join(format!(
            "athanor-settings-resident-{}-{nanos}",
            std::process::id()
        ));
        std::fs::create_dir(&dir).expect("a scratch directory");
        let registry = Registry::new();
        let state = || window_state::load(&registry, &dir).expect("readable");
        window_state::save(&dir, Target::Landing).expect("a crashed run's state");
        let mut settings = Settings::new(
            Registry::new(),
            crate::look::look(&Appearance::default()),
            false,
            First::Reopen(Target::Landing),
            Rc::default(),
            Rc::default(),
            Some(dir.clone()),
        );
        assert!(!settings.close_ends_run(), "closing the window does not end the run");
        let first = settings.start();
        let (ui, _) = first.first().expect("the window");
        let first_nav = settings.nav;
        assert!(ui.selected(first_nav).is_none(), "the landing page selects no row");
        let marker = dir.join(window_state::MARKER);
        assert!(marker.exists(), "the reopened page is drawn inside the marker");

        settings.close_window();
        assert_eq!(state(), None, "a crash with no window open reopens none");
        assert!(!marker.exists(), "a page closed before its first frame leaves no marker");

        let next = settings.window_at(Target::Page("about"));
        assert!(
            next.selected(settings.nav).is_some(),
            "the new window's own sidebar selects About"
        );
        assert_eq!(state(), Some(Target::Page("about")));
        assert!(!marker.exists(), "only a page reopened after a crash is marked");
        std::fs::remove_dir_all(&dir).expect("cleaned up");
    }

    #[test]
    fn a_close_ends_the_window_before_its_first_frame_and_on_its_own_surface_only() {
        assert!(closes_window(None, 7), "before the first frame any surface is the window");
        assert!(closes_window(Some(7), 7));
        assert!(!closes_window(Some(7), 8), "a dialog's Close leaves the window open");
    }
}
