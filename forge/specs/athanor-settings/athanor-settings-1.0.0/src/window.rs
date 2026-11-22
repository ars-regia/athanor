//! The window of doc_settings.md SE3: the sidebar, its search, and the page beside it, which is
//! the landing page until a page is opened. Only the pages this build draws are listed and
//! opened ([`BUILT`]); the rest of the registry joins as SE26 builds it.

use std::cell::RefCell;
use std::collections::VecDeque;
use std::path::PathBuf;
use std::rc::Rc;
use std::time::{SystemTime, UNIX_EPOCH};

use athanor_sal::driver::Role;
use athanor_sal::{Application, Event, Kind, Shell, SurfaceId, Ui, WidgetId, WindowButton, Wrap};
use athanor_settings_core::registry::{Group, Page, Registry};
use athanor_settings_core::target::{self, Target};
use athanor_settings_core::window_state;
use athanor_style::appearance::Appearance;
use athanor_trust_state::{Badge, ReadError, Reason, State};

use crate::i18n::tr;
use crate::info::{self, Hardware, Info};
use crate::look::{self, Look};

/// The application id, as the name of Settings' `.desktop` file.
pub const APP_ID: &str = "os.athanor.Settings";

/// The pages this build draws (SE26, step 1).
pub const BUILT: &[&str] = &["about"];

/// A row of a read-only page: its label, and its value when one was read.
type Line = (String, Option<String>);

/// The target of the oldest queued `Show`, which leaves the queue.
pub fn take_show(registry: &Registry, shows: &RefCell<VecDeque<String>>) -> Option<Target> {
    let text = shows.borrow_mut().pop_front()?;
    Some(resolve(registry, &text))
}

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
    /// Read when About is first drawn: pci.ids alone is over a megabyte.
    hardware: Option<Hardware>,
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
            hardware: None,
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
                    (tr("System integrity"), Some(trust_text(info.trust.as_ref(), now()))),
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

/// The trust state as About words it (SE22): the shield's own classification
/// (`athanor_trust_state::badge`, at `now`), so "Verified" only where the shield shows the
/// system as verified. A refused update is the shield's to say, not About's, so that badge
/// reads plain "Not verified", as every state that was not read or not trusted does.
fn trust_text(read: Option<&Result<State, ReadError>>, now: i64) -> String {
    let Some(Ok(state)) = read else {
        return tr("Not verified");
    };
    match athanor_trust_state::badge(state, now) {
        Badge::Check => tr("Verified"),
        Badge::Cross => tr("Not verified"),
        Badge::Attention if state.verified.value => tr("Not verified yet"),
        Badge::Attention => match state.verified.reason {
            Reason::Media => tr("Not verified: installed from media"),
            Reason::NoSignature => tr("Not verified: no signature"),
            Reason::KeyNotInPolicy => tr("Not verified: its key is not in the policy"),
            Reason::PolicyNotInForce => tr("Not verified: fetched under a permissive policy"),
            Reason::ReferenceOutOfScope => {
                tr("Not verified: the image is outside the policy's scope")
            }
            Reason::LocalChanges => tr("Not verified: the system was changed on this machine"),
            Reason::ChannelAbsent => {
                tr("Not verified: waiting for the stable channel to be published")
            }
            Reason::OriginNotEnforcing => {
                tr("Not verified: this machine does not verify its updates")
            }
            Reason::PinnedBuild => tr("Signed, but pinned to one build: it receives no updates"),
            Reason::Signature => tr("Not verified"),
        },
    }
}

/// Seconds since the epoch, as the shield counts the age of the last check.
fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| i64::try_from(elapsed.as_secs()).unwrap_or(i64::MAX))
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
            self.close_window();
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

    /// Opens the next window at the first `Show` since the last one closed, then applies the
    /// `Show` calls and the answers of the services that came since.
    fn turn(&mut self, shell: &mut Shell) {
        if self.closed {
            let Some(target) = take_show(&self.registry, &self.shows) else {
                return;
            };
            // Read again, as each start read them: they may have changed while no window was open.
            self.look = look::look(&Appearance::read());
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
        if changed && shows.is_empty() {
            self.draw_page(ui);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use athanor_style::appearance::Appearance;
    use athanor_trust_state::{
        state_reply, Deployment, ErrorCode, Policy, SecureBoot, UpdateState, Verified, SCHEMA,
    };

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

    const NOW: i64 = 1_800_000_000;

    /// A state the shield shows as verified, with the update service at rest.
    fn state(verified: Verified) -> State {
        let booted = Deployment {
            image: "registry.example/athanor-system:stable".into(),
            digest: format!("sha256:{}", "1".repeat(64)),
            version: "43.20260915.2".into(),
            build_time: NOW - 86_400,
        };
        State {
            schema: SCHEMA,
            booted,
            downloaded: None,
            previous: None,
            verified,
            update: UpdateState::None,
            policy: Policy {
                path: "/etc/containers/policy.json".into(),
                sha256: "ab".repeat(32),
                shipped: true,
            },
            secure_boot: SecureBoot {
                secure_boot: Some(1),
                setup_mode: Some(0),
                mok_sb_state: None,
                lockdown: Some("integrity".into()),
            },
            newest_booted_build_time: NOW - 86_400,
            last_successful_check: Some(NOW - 3_600),
            last_error: ErrorCode::None,
            last_error_host: None,
            automatic_security_updates: true,
            postpone: None,
        }
    }

    #[test]
    fn only_a_verified_state_from_root_reads_verified() {
        let json = serde_json::to_string(&state(Reason::Signature.into())).expect("serialises");
        let not_verified = tr("Not verified");
        let read = state_reply(Ok(&json), Some(0));
        assert_eq!(trust_text(Some(&read), NOW), tr("Verified"));
        for read in [
            state_reply(Ok(&json), Some(1000)),
            state_reply(Ok(&json), None),
            state_reply(Err(None), Some(0)),
            state_reply(Err(Some("os.athanor.Update1.Error.NoState")), Some(0)),
            state_reply(Err(Some("os.athanor.Update1.Error.Untrusted")), Some(0)),
            state_reply(Err(Some("os.athanor.Update1.Error.Unreadable")), Some(0)),
            state_reply(Ok("{"), Some(0)),
        ] {
            assert_eq!(trust_text(Some(&read), NOW), not_verified, "{read:?}");
        }
        assert_eq!(trust_text(None, NOW), not_verified, "not yet read");
    }

    #[test]
    fn verified_reads_verified_only_where_the_shield_shows_it() {
        let words = |state: State| trust_text(Some(&Ok(state)), NOW);
        let mut stale = state(Reason::Signature.into());
        stale.last_successful_check = Some(NOW - 15 * 86_400);
        assert_eq!(words(stale), tr("Not verified yet"));
        let mut older = state(Reason::Signature.into());
        older.newest_booted_build_time = NOW;
        assert_eq!(words(older), tr("Not verified yet"));
        // Through the parser, as a real read goes: it refuses a pair that disagrees.
        let json = serde_json::to_string(&state(Reason::PinnedBuild.into())).expect("serialises");
        assert_eq!(
            trust_text(Some(&state_reply(Ok(&json), Some(0))), NOW),
            tr("Signed, but pinned to one build: it receives no updates")
        );
        // The update verdict is the shield's, not About's.
        let mut refused = state(Reason::Signature.into());
        refused.update = UpdateState::Refused;
        assert_eq!(words(refused), tr("Not verified"));
    }

    #[test]
    fn a_state_that_is_not_verified_gives_its_reason() {
        let read = Ok(state(Reason::Media.into()));
        assert_eq!(trust_text(Some(&read), NOW), tr("Not verified: installed from media"));
    }
}
