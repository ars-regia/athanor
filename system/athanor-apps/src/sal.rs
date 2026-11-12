//! The row drawn with Athanor's own toolkit (doc_bar.md, BR3, BR7), as `row` draws it with
//! GTK: an icon button per entry, its menu of windows, launch and pin, and what a press or a
//! menu row does. The surface that shows it (the dock's island, the bar's module) owns the
//! placement, the menu's popup and its own buttons; this module owns the entries' widgets,
//! so the bar and the dock show one row (CC1).

use std::collections::HashMap;
use std::rc::Rc;

use athanor_layout::favorites;
use athanor_sal::{IconTheme, Image, Kind, Rgba, Side, Ui, WidgetId};
use athanor_sulphur_client::{Client, Opener, WindowId};
use gio::glib;
use gio_unix::DesktopAppInfo;

use crate::entries::{self, app_name, entry_label, State, FALLBACK_ICON};
use crate::favorites::Store;
use crate::i18n::{tr, tr_with};
use crate::model::{self, Open, Primary};

/// The size of an entry's icon, as the GTK row draws it.
pub const ENTRY_ICON: u16 = 24;
/// The size of an opener's icon and of a menu row's.
pub const SMALL_ICON: u16 = 16;

/// The images by icon, size and scale, tinted with one ink when symbolic; `None` for one
/// that could not be read, whose button shows its name.
pub struct Icons {
    theme: IconTheme,
    ink: Rgba,
    images: HashMap<(String, u16, u16), Option<Image>>,
}

impl Icons {
    pub fn new(theme: &str, ink: Rgba) -> Icons {
        Icons {
            theme: IconTheme::system(theme),
            ink,
            images: HashMap::new(),
        }
    }

    /// Drops the cached images, and tints the next ones with `ink`: a new look tints them
    /// anew, and an application installed or removed can bring the icon an entry fell back
    /// from, as the GTK row follows the theme.
    pub fn reset(&mut self, ink: Rgba) {
        self.ink = ink;
        self.images.clear();
    }

    pub fn image(&mut self, icon: &str, size: u16, scale: u16) -> Option<Image> {
        let (theme, ink) = (&self.theme, self.ink);
        self.images
            .entry((icon.to_owned(), size, scale))
            .or_insert_with(|| match theme.image(icon, size, scale) {
                Ok(image) if icon.ends_with("-symbolic") => Some(image.tinted(ink)),
                Ok(image) => Some(image),
                Err(err) => {
                    tracing::error!(error = %err, icon, "cannot draw the icon; the button shows its name");
                    None
                }
            })
            .clone()
    }

    /// A button with the icon `icon` and the name and tooltip `label`; a plain button with
    /// the label when the icon cannot be read.
    #[allow(clippy::too_many_arguments)]
    pub fn button(
        &mut self,
        ui: &mut Ui,
        parent: WidgetId,
        icon: &str,
        size: u16,
        scale: u16,
        label: &str,
        indicator: Option<Side>,
    ) -> WidgetId {
        let button = match self.image(icon, size, scale) {
            Some(image) => ui.icon_button(parent, label, image, f32::from(size), indicator),
            None => ui.add(parent, Kind::Button(label.to_owned())),
        };
        ui.set_tooltip(button, Some(label));
        button
    }
}

/// The icon an entry is drawn with: its desktop entry's, or [`FALLBACK_ICON`].
pub fn entry_icon(info: Option<&DesktopAppInfo>) -> String {
    info.and_then(|info| info.string("Icon"))
        .map_or_else(|| FALLBACK_ICON.to_owned(), |icon| icon.to_string())
}

/// Whether a row drawn from `before` must be drawn again for `now`: an entry changed, or
/// what its button shows of the desktop entry, its icon or its name, as when the application
/// of a pinned entry is installed or its desktop entry edited.
pub fn redraws(before: &[State], now: &[State]) -> bool {
    let drawn = |states: &[State]| {
        states
            .iter()
            .map(|(entry, info)| {
                (
                    entry.clone(),
                    entry_icon(info.as_ref()),
                    app_name(entry, info.as_ref()),
                )
            })
            .collect::<Vec<_>>()
    };
    drawn(before) != drawn(now)
}

/// The entries' buttons of one surface, with the states they were drawn from.
#[derive(Default)]
pub struct Row {
    buttons: Vec<WidgetId>,
    states: Vec<State>,
}

impl Row {
    /// Appends a button per entry of `states` to `parent`. An active entry carries its
    /// indicator on `indicator`'s side.
    pub fn build(
        ui: &mut Ui,
        parent: WidgetId,
        states: &[State],
        icons: &mut Icons,
        scale: u16,
        indicator: Side,
    ) -> Row {
        let mut buttons = Vec::with_capacity(states.len());
        for (entry, info) in states {
            let label = entry_label(&app_name(entry, info.as_ref()), &entry.windows);
            let icon = entry_icon(info.as_ref());
            let shown = entries::active(entry).then_some(indicator);
            let button = icons.button(ui, parent, &icon, ENTRY_ICON, scale, &label, shown);
            if !entry.windows.is_empty() {
                ui.set_description(button, Some(&tr("Running")));
            }
            ui.set_has_popup(button, true);
            buttons.push(button);
        }
        Row {
            buttons,
            states: states.to_vec(),
        }
    }

    pub fn states(&self) -> &[State] {
        &self.states
    }

    /// The entry whose button is `widget`.
    pub fn entry(&self, widget: WidgetId) -> Option<&State> {
        let index = self.buttons.iter().position(|b| *b == widget)?;
        self.states.get(index)
    }

    /// The desktop id of a pinned entry's button.
    pub fn pinned(&self, widget: WidgetId) -> Option<String> {
        let (entry, _) = self.entry(widget)?;
        entry.pinned.then(|| entry.desktop_id.clone())?
    }
}

/// What a press on an entry's button asks for.
pub enum Press {
    /// Done: the entry was launched, activated or minimised.
    Done,
    /// The entry has several windows: its menu opens to choose one.
    Menu,
}

/// Does what a press on `state`'s button does (`model::primary`). `app` names the program in
/// a failed start's notification.
pub fn press(client: &Rc<Client>, state: &State, app: &'static str) -> Press {
    let (entry, info) = state;
    match model::primary(entry) {
        Primary::Launch => {
            if let Some(info) = info {
                launch(client, info, app);
            }
        }
        Primary::Activate(id) => {
            if let Some(window) = entry.windows.iter().find(|window| window.id == id) {
                entries::activate(client, window);
            }
        }
        Primary::Minimize(id) => entries::minimize(client, id),
        Primary::Choose => return Press::Menu,
    }
    Press::Done
}

/// BR2: every start goes through the compositor client, behind a security context
/// (`entries::launch`).
pub fn launch(client: &Rc<Client>, info: &DesktopAppInfo, app: &'static str) {
    let (client, info) = (client.clone(), info.clone());
    glib::spawn_future_local(async move { entries::launch(&client, &info, app).await });
}

pub fn open(client: &Rc<Client>, opener: Opener) {
    let client = client.clone();
    glib::spawn_future_local(async move { entries::open(&client, opener).await });
}

/// What a row of an entry's menu does.
#[derive(Clone)]
pub enum MenuAct {
    Activate(Open<WindowId>),
    Minimize(WindowId),
    Close(WindowId),
    Launch(DesktopAppInfo),
    Pin(String),
    Unpin(String),
}

/// The pin row of the menu of the entry with the desktop id `id`, from the favourites file
/// read as the menu opens rather than from the row as it was last drawn: a menu holds the
/// row, so a change another writer made to the file shows in the next menu all the same.
/// Pinning needs the favourites and a desktop id; without either there is no row.
pub fn pin_act(favourites: Option<Vec<String>>, id: Option<String>) -> Option<MenuAct> {
    let (ids, id) = (favourites?, id?);
    Some(if ids.contains(&id) {
        MenuAct::Unpin(id)
    } else {
        MenuAct::Pin(id)
    })
}

/// An entry's menu: its rows' acts, and whether the favourites file changed since the row
/// was drawn, so that the row is drawn again once the menu closes.
pub struct Menu {
    pub acts: Vec<(WidgetId, MenuAct)>,
    pub stale: bool,
}

impl Menu {
    pub fn act(&self, row: WidgetId) -> Option<MenuAct> {
        self.acts
            .iter()
            .find(|(w, _)| *w == row)
            .map(|(_, act)| act.clone())
    }
}

/// The title of `state`'s menu: the application's name.
pub fn menu_title(state: &State) -> String {
    app_name(&state.0, state.1.as_ref())
}

/// Fills `ui` with `state`'s menu: per window, its row, minimise and close; then a
/// separator, open or a new window, and the pin row labelled by `pin_label(pinned)`.
pub fn menu(
    ui: &mut Ui,
    icons: &mut Icons,
    scale: u16,
    state: &State,
    favorites: &Store,
    pin_label: impl Fn(bool) -> String,
) -> Menu {
    let (entry, info) = state;
    let list = ui.add(Ui::ROOT, Kind::Menu);
    let mut acts = Vec::new();
    let mut stale = false;
    for window in &entry.windows {
        let title = entries::window_title(window);
        let row = ui.add(list, Kind::Button(title.clone()));
        acts.push((row, MenuAct::Activate(window.clone())));
        let minimise = tr_with("Minimise {title}", "title", &title);
        let row = icons.button(
            ui,
            list,
            "window-minimize-symbolic",
            SMALL_ICON,
            scale,
            &minimise,
            None,
        );
        acts.push((row, MenuAct::Minimize(window.id)));
        let close = tr_with("Close {title}", "title", &title);
        let row = icons.button(
            ui,
            list,
            "window-close-symbolic",
            SMALL_ICON,
            scale,
            &close,
            None,
        );
        acts.push((row, MenuAct::Close(window.id)));
    }
    if let Some(info) = info {
        if !entry.windows.is_empty() {
            ui.add(list, Kind::Separator);
        }
        let open = if entry.windows.is_empty() {
            tr("Open")
        } else {
            tr("New Window")
        };
        let row = ui.add(list, Kind::Button(open));
        acts.push((row, MenuAct::Launch(info.clone())));
        // The file as it is now, not as the watch's debounce last read it: one small read,
        // bounded by favorites::MAX_FILE_BYTES, per menu. A change the watch has not brought
        // yet redraws the row too, once the menu closes.
        stale = favorites.reload();
        if let Some(act) = pin_act(favorites.ids(), entry.desktop_id.clone()) {
            let label = pin_label(matches!(act, MenuAct::Unpin(_)));
            let row = ui.add(list, Kind::Button(label));
            acts.push((row, act));
        }
    }
    Menu { acts, stale }
}

/// Does what a menu row does.
pub fn menu_act(client: &Rc<Client>, favorites: &Store, act: MenuAct, app: &'static str) {
    match act {
        MenuAct::Activate(window) => entries::activate(client, &window),
        MenuAct::Minimize(id) => entries::minimize(client, id),
        MenuAct::Close(id) => entries::close(client, id),
        MenuAct::Launch(info) => launch(client, &info, app),
        MenuAct::Pin(id) => {
            favorites.change(|ids| favorites::pinned(ids, &id));
        }
        MenuAct::Unpin(id) => {
            favorites.change(|ids| Ok(favorites::unpinned(ids, &id)));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_pin_row_follows_the_favourites_as_they_are_now() {
        let id = "org.athanor.CcWindow1.desktop".to_owned();
        let pinned = Some(vec![id.clone()]);
        assert!(matches!(pin_act(pinned, Some(id.clone())), Some(MenuAct::Unpin(i)) if i == id));
        assert!(
            matches!(pin_act(Some(Vec::new()), Some(id.clone())), Some(MenuAct::Pin(i)) if i == id)
        );
        // Unavailable favourites, or an entry without a desktop id: no row.
        assert!(pin_act(None, Some(id)).is_none());
        assert!(pin_act(Some(Vec::new()), None).is_none());
    }

    fn info(icon: &str, name: &str) -> Option<DesktopAppInfo> {
        let file = glib::KeyFile::new();
        let data =
            format!("[Desktop Entry]\nType=Application\nName={name}\nIcon={icon}\nExec=true\n");
        file.load_from_data(&data, glib::KeyFileFlags::NONE).ok()?;
        DesktopAppInfo::from_keyfile(&file)
    }

    #[test]
    fn a_pinned_entry_is_drawn_again_when_its_application_comes_or_its_desktop_entry_changes() {
        let entry = model::Entry {
            desktop_id: Some("org.example.Mail.desktop".to_owned()),
            app_id: "org.example.Mail".to_owned(),
            pinned: true,
            windows: Vec::new(),
        };
        let missing: Vec<State> = vec![(entry.clone(), None)];
        let installed: Vec<State> = vec![(entry.clone(), info("mail-client", "Mail"))];
        assert!(installed[0].1.is_some(), "the fixture is a desktop entry");
        assert!(!redraws(&missing, &missing));
        assert!(!redraws(&installed, &installed));
        // The same entry: the fallback icon and the app id give way to the desktop entry's.
        assert!(redraws(&missing, &installed));
        let renamed: Vec<State> = vec![(entry.clone(), info("mail-client", "Post"))];
        let new_icon: Vec<State> = vec![(entry, info("mail-new", "Mail"))];
        assert!(redraws(&installed, &renamed));
        assert!(redraws(&installed, &new_icon));
    }

    #[test]
    fn a_button_names_the_entry_it_was_drawn_for() {
        let entry = |id: &str, pinned| model::Entry {
            desktop_id: Some(format!("{id}.desktop")),
            app_id: id.to_owned(),
            pinned,
            windows: Vec::new(),
        };
        let states: Vec<State> = vec![
            (entry("org.a.A", true), None),
            (entry("org.b.B", false), None),
        ];
        let mut ui = Ui::new("Row", athanor_sal::Theme::default(), false);
        let parent = ui.add(Ui::ROOT, Kind::Column);
        let mut icons = Icons::new("Adwaita", Rgba([0, 0, 0, 255]));
        let row = Row::build(&mut ui, parent, &states, &mut icons, 1, Side::Bottom);
        let buttons = ui.widget(parent).children.clone();
        assert_eq!(buttons.len(), 2);
        assert_eq!(
            row.entry(buttons[1]).map(|(e, _)| e.app_id.as_str()),
            Some("org.b.B")
        );
        assert_eq!(row.pinned(buttons[0]).as_deref(), Some("org.a.A.desktop"));
        assert_eq!(
            row.pinned(buttons[1]),
            None,
            "an entry not pinned does not drag"
        );
        assert!(row.entry(parent).is_none());
    }
}
