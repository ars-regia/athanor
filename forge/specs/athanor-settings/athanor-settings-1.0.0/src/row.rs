//! Preference-row helpers built from Sal's existing widgets (doc_settings.md SE3), shared by
//! pages until Sal adds a dedicated general preference row.

use athanor_sal::{Kind, Ui, WidgetId, Wrap};

use crate::look::Look;

/// A column that fills its parent's width, so switches, sliders and rows span the page.
pub fn column(ui: &mut Ui, parent: WidgetId) -> WidgetId {
    let col = ui.add(parent, Kind::Column);
    ui.set_fill(col, true);
    col
}

/// A group's heading styled with `look`.
pub fn heading(ui: &mut Ui, parent: WidgetId, look: &Look, text: &str) -> WidgetId {
    let label = ui.add(parent, Kind::Label(text.to_owned()));
    look.heading(ui, label);
    label
}

/// A horizontal row holding a `title` and an optional secondary `note` in a filling column,
/// returning the row id so the caller can append the trailing control at its end edge.
pub fn row_frame(
    ui: &mut Ui,
    parent: WidgetId,
    look: &Look,
    title: &str,
    note: Option<&str>,
) -> WidgetId {
    let row = ui.add(parent, Kind::Row);
    let text = column(ui, row);
    let label = ui.add(text, Kind::Label(title.to_owned()));
    ui.set_wrap(label, Some(Wrap::default()));
    if let Some(note) = note.filter(|text| !text.trim().is_empty()) {
        let secondary = ui.add(text, Kind::Label(note.to_owned()));
        look.note(ui, secondary);
        ui.set_wrap(secondary, Some(Wrap::default()));
    }
    row
}

/// A labelled switch row, using Sal's `Ui::switch_row` so assistive technologies see one
/// switch named by `title` and described by `note`.
pub fn switch(
    ui: &mut Ui,
    parent: WidgetId,
    title: &str,
    note: Option<&str>,
    active: bool,
) -> WidgetId {
    ui.switch_row(parent, title, note.unwrap_or(""), active)
}

/// A preference row with a trailing button labelled `button_label`, returning the button's id.
pub fn button(
    ui: &mut Ui,
    parent: WidgetId,
    look: &Look,
    title: &str,
    note: Option<&str>,
    button_label: &str,
) -> WidgetId {
    let row = row_frame(ui, parent, look, title, note);
    ui.add(row, Kind::Button(button_label.to_owned()))
}

/// A preference row with a trailing drop-down, returning the drop-down's id.
pub fn drop_down(
    ui: &mut Ui,
    parent: WidgetId,
    look: &Look,
    title: &str,
    note: Option<&str>,
    items: &[&str],
    selected: usize,
) -> WidgetId {
    let row = row_frame(ui, parent, look, title, note);
    ui.drop_down(row, title, items, selected)
}

/// A preference row with a trailing slider, returning the slider's id when its bounds are valid.
#[allow(clippy::too_many_arguments)]
pub fn slider(
    ui: &mut Ui,
    parent: WidgetId,
    look: &Look,
    title: &str,
    note: Option<&str>,
    min: f64,
    max: f64,
    step: f64,
    value: f64,
) -> Option<WidgetId> {
    let row = row_frame(ui, parent, look, title, note);
    ui.slider(row, title, min, max, step, value)
}

/// A preference row with a trailing single-line text field, returning the field's id.
pub fn field(
    ui: &mut Ui,
    parent: WidgetId,
    look: &Look,
    title: &str,
    note: Option<&str>,
    text: &str,
) -> WidgetId {
    let row = row_frame(ui, parent, look, title, note);
    ui.field(row, title, text)
}

#[cfg(test)]
mod tests {
    use athanor_sal::Ui;
    use athanor_style::appearance::Appearance;

    use super::*;

    fn test_ui() -> (Ui, Look) {
        let look = crate::look::look(&Appearance::default());
        let ui = Ui::new("Settings", look.theme.clone(), false);
        (ui, look)
    }

    #[test]
    fn row_frame_omits_blank_note_and_styles_non_empty_note() {
        let (mut ui, look) = test_ui();
        let plain = row_frame(&mut ui, Ui::ROOT, &look, "Title", Some("   "));
        let text_col = ui.widget(plain).children[0];
        assert_eq!(ui.widget(text_col).children.len(), 1);

        let noted = row_frame(&mut ui, Ui::ROOT, &look, "Title", Some("Detail"));
        let noted_col = ui.widget(noted).children[0];
        assert_eq!(ui.widget(noted_col).children.len(), 2);
    }

    #[test]
    fn trailing_controls_attach_to_the_row() {
        let (mut ui, look) = test_ui();
        let sw = switch(&mut ui, Ui::ROOT, "Wi-Fi", Some("Enabled"), true);
        assert!(ui.is_active(sw));

        let btn = button(&mut ui, Ui::ROOT, &look, "Reset", None, "Reset");
        assert!(matches!(&ui.widget(btn).kind, Kind::Button(t) if t == "Reset"));

        let dd = drop_down(&mut ui, Ui::ROOT, &look, "Scale", None, &["100%", "150%"], 1);
        assert_eq!(ui.chosen(dd), Some(1));

        let sl = slider(&mut ui, Ui::ROOT, &look, "Volume", None, 0.0, 100.0, 5.0, 50.0)
            .expect("valid slider bounds");
        assert!(matches!(ui.widget(sl).kind, Kind::Slider(_)));

        let fld = field(&mut ui, Ui::ROOT, &look, "Name", None, "athanor");
        assert_eq!(ui.field_text(fld).as_deref(), Some("athanor"));
    }

    #[test]
    fn column_fills_its_parent_width() {
        let (mut ui, look) = test_ui();
        ui.min_width = 500.0;
        let col = column(&mut ui, Ui::ROOT);
        ui.layout();
        let expected = ui.widget(Ui::ROOT).rect.w - 2.0 * look.theme.margin;
        assert_eq!(ui.widget(col).rect.w, expected);
    }
}
