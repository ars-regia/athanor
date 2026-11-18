//! A row of the list. Every string was cleaned by its source (athanor_unit::text) and is set
//! as plain text; the accessible name is the title and the kind (LA5).

use athanor_launcher::list::Line;
use athanor_preview::themed;
use athanor_sal::{Kind, TextStyle, Ui, WidgetId, Wrap};
use athanor_search::command::Refusal;
use athanor_search::item::{Group, Hit};
use athanor_style::sal::size;

use super::preview::Icons;
use crate::i18n::tr;

fn kind(group: Group) -> String {
    match group {
        Group::Command => tr("Command"),
        Group::Apps => tr("Application"),
        Group::Windows => tr("Window"),
        Group::Calc => tr("Calculation"),
        Group::Settings => tr("Settings page"),
        Group::Files => tr("File"),
        Group::Providers => tr("Search result"),
        Group::Web => tr("Web search"),
    }
}

fn header(group: Group, title: &str) -> String {
    match group {
        Group::Providers => title.to_owned(),
        Group::Command => tr("Command"),
        Group::Apps => tr("Applications"),
        Group::Windows => tr("Windows"),
        Group::Calc => tr("Calculator"),
        Group::Settings => tr("Settings"),
        Group::Files => tr("Files"),
        Group::Web => tr("Web"),
    }
}

fn refusal(why: Refusal) -> String {
    match why {
        Refusal::Empty => tr("Type a command after >"),
        Refusal::TooLong => tr("This command is too long to run"),
        Refusal::Hidden => tr("This command holds hidden characters and is not run"),
        Refusal::Quoting => tr("This command has an unclosed quote or escape and is not run"),
    }
}

/// The size of the top hit's icon, as the GTK launcher's; the other rows take the toolkit's
/// launcher row icon (doc_visual_language.md VL10).
const TOP_ICON: f32 = 40.0;
const ROW_ICON: f32 = size::icon::LAUNCHER_ROW;

/// A label that keeps to one line and ends in an ellipsis when its place is narrower, as
/// the GTK rows' labels.
pub(super) fn label(ui: &mut Ui, parent: WidgetId, text: &str) -> WidgetId {
    let label = ui.add(parent, Kind::Label(text.to_owned()));
    let mut wrap = Wrap::default();
    wrap.lines = std::num::NonZeroU32::new(1);
    ui.set_wrap(label, Some(wrap));
    label
}

/// The type step of the subtitles and the section titles: the GTK launcher's 0.85 em.
pub(super) fn caption() -> TextStyle {
    TextStyle {
        scale: size::CAPTION.scale,
        weight: size::CAPTION.weight,
        ..TextStyle::default()
    }
}

fn hit_row(
    ui: &mut Ui,
    list: WidgetId,
    icons: &mut Icons,
    scale: u16,
    hit: &Hit,
    top: bool,
) -> WidgetId {
    let row = ui.list_row(list, &format!("{}, {}", hit.title, kind(hit.group)));
    let side = if top { TOP_ICON } else { ROW_ICON };
    // LA9: only a themed icon is drawn; any other icon is a file this process would decode.
    // No icon, or a refused one, is the preview's generic icon.
    if let Some(image) = icons.image(&themed(hit.icon.clone()), side as u16, scale) {
        ui.image(row, image, side, "");
    }
    let text = ui.add(row, Kind::Column);
    label(ui, text, &hit.title);
    if !hit.subtitle.is_empty() {
        let subtitle = label(ui, text, &hit.subtitle);
        ui.set_text_style(subtitle, caption());
    }
    row
}

fn passive(ui: &mut Ui, list: WidgetId, text: &str, title: bool) -> WidgetId {
    let heading = ui.list_heading(list, text);
    if title {
        ui.set_text_style(heading, caption());
    }
    heading
}

/// Adds `line` at the end of `list`: a row that the keyboard and the pointer choose for a
/// hit, a heading they pass over for the rest.
pub fn row(ui: &mut Ui, list: WidgetId, icons: &mut Icons, scale: u16, line: &Line) -> WidgetId {
    match line {
        Line::Top(hit) => hit_row(ui, list, icons, scale, hit, true),
        Line::Hit(hit) => hit_row(ui, list, icons, scale, hit, false),
        Line::Header { group, title } => passive(ui, list, &header(*group, title), true),
        Line::Indexing => passive(ui, list, &tr("Indexing files…"), true),
        Line::Refused(why) => passive(ui, list, &refusal(*why), false),
    }
}
