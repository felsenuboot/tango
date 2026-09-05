//! The start screen (issue #80): the name of the app as a dictionary entry, with 単語 in
//! brush calligraphy beside it, the way DenMail introduces itself.
//!
//! The calligraphy is a symbolic SVG in `data/icons` (glyph outlines of Yuji Syuku, OFL).
//! It is painted as a CSS background through `-gtk-recolor`, which fills a symbolic icon
//! with the node's `color`; the stylesheet sets that to the accent colour, so every theme
//! gets its own tint and dark or light needs no second file. A `gtk::Image` would do the
//! recolouring too, but it renders icons into a square, and this one is twice as wide as
//! it is high.

use adw::prelude::*;
use gtk::{gdk, glib};

/// Icon name of the calligraphy; the SVG sits next to the app icon.
const CALLIGRAPHY: &str = "io.github.felsenuboot.Tango-calligraphy-symbolic";
/// Size of the calligraphy in logical pixels, the SVG's own aspect ratio.
const WIDTH: i32 = 312;
const HEIGHT: i32 = 150;

pub struct Start {
    pub widget: gtk::Box,
    /// Calligraphy and entry side by side; the window's narrow breakpoint stacks them.
    pub row: gtk::Box,
}

/// Builds the page. `lookup` runs when the "Look it up" link is activated.
pub fn build(lookup: impl Fn() + 'static) -> Start {
    let row = gtk::Box::builder()
        .spacing(28)
        .halign(gtk::Align::Center)
        .valign(gtk::Align::Center)
        .build();
    row.append(&calligraphy());

    let column = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(6)
        .valign(gtk::Align::Center)
        .build();
    let head = gtk::Label::builder()
        .use_markup(true)
        .label(
            "<span size='x-large' weight='bold'>単語</span>  〔たんご · <i>tango</i>〕  \
             <span size='small' alpha='60%'>noun</span>",
        )
        .xalign(0.0)
        .build();
    column.append(&head);
    column.append(&line("1. word; vocabulary"));
    let german = line("German: Wort; Vokabel; Einzelwort");
    german.add_css_class("caption");
    german.add_css_class("dim-label");
    column.append(&german);
    let link = gtk::Label::builder()
        .use_markup(true)
        .label("<a href='tango:lookup'>Look it up in Tango</a>")
        .xalign(0.0)
        .margin_top(8)
        .css_classes(["caption"])
        .build();
    link.connect_activate_link(move |_, _| {
        lookup();
        glib::Propagation::Stop
    });
    column.append(&link);
    row.append(&column);

    let hint = gtk::Label::builder()
        .label("Look up a word in Japanese, English or German.")
        .justify(gtk::Justification::Center)
        .wrap(true)
        .css_classes(["dim-label"])
        .build();
    let widget = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(36)
        .halign(gtk::Align::Center)
        .valign(gtk::Align::Center)
        .vexpand(true)
        .margin_start(24)
        .margin_end(24)
        .margin_top(24)
        .margin_bottom(24)
        .css_classes(["tango-start"])
        .build();
    widget.append(&row);
    widget.append(&hint);
    Start { widget, row }
}

fn line(text: &str) -> gtk::Label {
    gtk::Label::builder()
        .label(text)
        .xalign(0.0)
        .wrap(true)
        .max_width_chars(36)
        .build()
}

/// The calligraphy, or the two characters in a large font when the SVG is not installed.
fn calligraphy() -> gtk::Widget {
    let Some(display) = gdk::Display::default() else {
        return fallback();
    };
    let theme = gtk::IconTheme::for_display(&display);
    if !theme.has_icon(CALLIGRAPHY) {
        log::warn!("calligraphy icon {CALLIGRAPHY} not found; showing plain text");
        return fallback();
    }
    let icon = theme.lookup_icon(
        CALLIGRAPHY,
        &[],
        HEIGHT,
        1,
        gtk::TextDirection::Ltr,
        gtk::IconLookupFlags::empty(),
    );
    let Some(uri) = icon.file().map(|f| f.uri()) else {
        return fallback();
    };
    // The file's location is only known at run time (checkout or install prefix), so this
    // one rule is added here rather than in style.css.
    let provider = gtk::CssProvider::new();
    provider.load_from_string(&format!(
        ".tango-calligraphy {{ background-image: -gtk-recolor(url(\"{uri}\")); }}"
    ));
    gtk::style_context_add_provider_for_display(
        &display,
        &provider,
        gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
    );
    gtk::Box::builder()
        .width_request(WIDTH)
        .height_request(HEIGHT)
        .halign(gtk::Align::Center)
        .valign(gtk::Align::Center)
        .css_classes(["tango-calligraphy"])
        .accessible_role(gtk::AccessibleRole::Presentation)
        .build()
        .upcast()
}

fn fallback() -> gtk::Widget {
    gtk::Label::builder()
        .label("単語")
        .css_classes(["tango-calligraphy-fallback"])
        .accessible_role(gtk::AccessibleRole::Presentation)
        .build()
        .upcast()
}
