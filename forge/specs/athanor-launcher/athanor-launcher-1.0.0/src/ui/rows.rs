//! A row of the list. Every string was cleaned by its source (athanor_unit::text) and is set
//! as plain text; the accessible name is the title and the kind (LA5).

use athanor_launcher::list::Line;
use athanor_search::item::{Group, Hit};
use gtk4::prelude::*;

use athanor_preview::themed;

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

fn label(text: &str, class: &str) -> gtk4::Label {
    let label = gtk4::Label::builder().label(text).xalign(0.0).ellipsize(gtk4::pango::EllipsizeMode::End).build();
    label.add_css_class(class);
    label
}

fn passive(child: &impl IsA<gtk4::Widget>) -> gtk4::ListBoxRow {
    let row = gtk4::ListBoxRow::builder().child(child).selectable(false).activatable(false).build();
    row.set_focusable(false);
    row
}

fn hit_row(hit: &Hit, top: bool) -> gtk4::ListBoxRow {
    let line = gtk4::Box::new(gtk4::Orientation::Horizontal, 10);
    let icon = gtk4::Image::builder().pixel_size(if top { 40 } else { 28 }).build();
    // LA9: only a themed icon is drawn; any other icon is a file this process would decode.
    if let Some(gicon) = &hit.icon {
        icon.set_from_gicon(&themed(Some(gicon.clone())));
    }
    line.append(&icon);
    let text = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
    text.append(&label(&hit.title, "row-title"));
    if !hit.subtitle.is_empty() {
        text.append(&label(&hit.subtitle, "row-subtitle"));
    }
    line.append(&text);
    let row = gtk4::ListBoxRow::builder().child(&line).build();
    row.set_focusable(false);
    row.update_property(&[gtk4::accessible::Property::Label(&format!("{}, {}", hit.title, kind(hit.group)))]);
    if top {
        row.add_css_class("top-hit");
    }
    row
}

pub fn row(line: &Line) -> gtk4::ListBoxRow {
    match line {
        Line::Top(hit) => hit_row(hit, true),
        Line::Hit(hit) => hit_row(hit, false),
        Line::Header { group, title } => passive(&label(&header(*group, title), "section-title")),
        Line::Indexing => passive(&label(&tr("Indexing files…"), "section-title")),
    }
}
