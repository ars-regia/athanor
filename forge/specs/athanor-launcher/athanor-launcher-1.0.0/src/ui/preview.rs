//! The preview pane's GTK widgets (LA6): they draw the card athanor-preview hands over at each
//! change. Every text is set as plain text, never as markup.

use std::rc::Rc;

use athanor_preview::{Card, Picture, Preview};
use gtk4::prelude::*;
use gtk4::{gdk, glib};

struct Widgets {
    picture: gtk4::Picture,
    icon: gtk4::Image,
    title: gtk4::Label,
    body: gtk4::Label,
    facts: gtk4::Label,
}

fn label(class: &str) -> gtk4::Label {
    let label = gtk4::Label::builder()
        .xalign(0.0)
        .wrap(true)
        .selectable(false)
        .use_markup(false)
        .build();
    label.add_css_class(class);
    label
}

fn set(label: &gtk4::Label, text: &str) {
    label.set_text(text);
    label.set_visible(!text.is_empty());
}

impl Widgets {
    fn draw(&self, card: &Card) {
        match &card.picture {
            Some(picture) => self.picture.set_paintable(Some(&texture(picture))),
            None => self.picture.set_paintable(None::<&gdk::Paintable>),
        }
        self.picture.set_visible(card.picture.is_some());
        if let Some(icon) = &card.icon {
            self.icon.set_from_gicon(icon);
        }
        self.icon
            .set_visible(card.icon.is_some() && card.picture.is_none());
        set(&self.title, &card.title);
        set(&self.body, &card.body);
        set(&self.facts, &card.facts);
    }
}

/// A picture of athanor-preview as a GDK texture.
fn texture(picture: &Picture) -> gdk::MemoryTexture {
    // A decoded picture is at most 1024 pixels a side, and a window's at most 8192.
    gdk::MemoryTexture::new(
        picture.width as i32,
        picture.height as i32,
        gdk::MemoryFormat::R8g8b8a8Premultiplied,
        &glib::Bytes::from(&picture.rgba),
        picture.width as usize * 4,
    )
}

/// The pane and the preview that fills it.
pub(super) fn new() -> (gtk4::Box, Rc<Preview>) {
    let root = gtk4::Box::new(gtk4::Orientation::Vertical, 8);
    root.add_css_class("preview");
    root.set_accessible_role(gtk4::AccessibleRole::Region);
    let picture = gtk4::Picture::builder()
        .can_shrink(true)
        .content_fit(gtk4::ContentFit::Contain)
        .height_request(220)
        .build();
    let icon = gtk4::Image::builder().pixel_size(96).build();
    let (title, body, facts) = (
        label("preview-title"),
        label("preview-body"),
        label("preview-facts"),
    );
    body.set_wrap_mode(gtk4::pango::WrapMode::WordChar);
    for widget in [
        picture.upcast_ref::<gtk4::Widget>(),
        icon.upcast_ref(),
        title.upcast_ref(),
        body.upcast_ref(),
        facts.upcast_ref(),
    ] {
        root.append(widget);
    }
    let widgets = Widgets {
        picture,
        icon,
        title,
        body,
        facts,
    };
    widgets.draw(&Card::default());
    let preview = Preview::new(move |card| widgets.draw(card));
    // The scale sizes the pictures decoded from now on.
    let weak = Rc::downgrade(&preview);
    root.connect_scale_factor_notify(move |root| {
        if let Some(preview) = weak.upgrade() {
            preview.set_scale(f64::from(root.scale_factor()));
        }
    });
    preview.set_scale(f64::from(root.scale_factor()));
    (root, preview)
}
