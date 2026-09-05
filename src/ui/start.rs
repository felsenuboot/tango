//! The start screen (issue #80): the name of the app as a dictionary entry, with 単語 in
//! brush calligraphy beside it, the way DenMail introduces itself.
//!
//! The calligraphy is `data/calligraphy.svg`, the glyph outlines of Yuji Syuku (OFL),
//! compiled in and drawn with cairo in a `gtk::DrawingArea`, filled with the widget's CSS
//! `color` (the accent colour, see style.css). Drawing it ourselves keeps the aspect ratio
//! at any size and scale factor: GTK renders symbolic icons into a square before it
//! recolours them, which squashed a 2:1 image (issue #88), and a `gtk::Image` would report a
//! square size too.

use adw::prelude::*;
use gtk::{cairo, glib};

/// The calligraphy: only `<path transform="translate(x y)" d="M … C … L … Z">` elements.
const SVG: &str = include_str!("../../data/calligraphy.svg");
/// Size of the calligraphy in logical pixels, the SVG's own aspect ratio.
const WIDTH: i32 = 312;
const HEIGHT: i32 = 150;

pub struct Start {
    /// The page, scrolling vertically when the window is too short for it.
    pub widget: gtk::ScrolledWindow,
    /// Calligraphy and entry side by side; the window's narrow breakpoints stack them (#91).
    pub row: gtk::Box,
    /// The calligraphy, when the SVG parsed; the breakpoints shrink it through its
    /// `content-width` and `content-height`.
    pub art: Option<gtk::DrawingArea>,
}

/// Builds the page. `lookup` runs when the "Look it up" link is activated.
pub fn build(lookup: impl Fn() + 'static) -> Start {
    let row = gtk::Box::builder()
        .spacing(28)
        .halign(gtk::Align::Center)
        .valign(gtk::Align::Center)
        .build();
    let art = calligraphy();
    row.append(&art);
    let art = art.downcast::<gtk::DrawingArea>().ok();

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
        .wrap(true)
        .max_width_chars(30)
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
        .max_width_chars(40)
        .css_classes(["dim-label"])
        .build();
    let page = gtk::Box::builder()
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
    page.append(&row);
    page.append(&hint);
    let widget = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vscrollbar_policy(gtk::PolicyType::Automatic)
        .child(&page)
        .vexpand(true)
        .build();
    Start { widget, row, art }
}

fn line(text: &str) -> gtk::Label {
    gtk::Label::builder()
        .label(text)
        .xalign(0.0)
        .wrap(true)
        .max_width_chars(36)
        .build()
}

/// The calligraphy, drawn with cairo; the plain characters if the SVG does not parse.
fn calligraphy() -> gtk::Widget {
    let Some(art) = Art::parse(SVG) else {
        log::warn!("calligraphy SVG did not parse; showing plain text");
        return gtk::Label::builder()
            .label("単語")
            .css_classes(["tango-calligraphy-fallback"])
            .accessible_role(gtk::AccessibleRole::Presentation)
            .build()
            .upcast();
    };
    let area = gtk::DrawingArea::builder()
        .content_width(WIDTH)
        .content_height(HEIGHT)
        .halign(gtk::Align::Center)
        .valign(gtk::Align::Center)
        .css_classes(["tango-calligraphy"])
        .accessible_role(gtk::AccessibleRole::Presentation)
        .build();
    area.set_draw_func(move |area, cr, width, height| {
        let scale = (f64::from(width) / art.width).min(f64::from(height) / art.height);
        cr.translate(
            (f64::from(width) - art.width * scale) / 2.0,
            (f64::from(height) - art.height * scale) / 2.0,
        );
        cr.scale(scale, scale);
        cr.translate(-art.x, -art.y);
        let colour = area.color();
        cr.set_source_rgba(
            f64::from(colour.red()),
            f64::from(colour.green()),
            f64::from(colour.blue()),
            f64::from(colour.alpha()),
        );
        art.draw(cr);
        let _ = cr.fill();
    });
    area.upcast()
}

/// One glyph: where it sits, and its outline as absolute path commands.
struct Glyph {
    dx: f64,
    dy: f64,
    commands: Vec<Command>,
}

enum Command {
    MoveTo(f64, f64),
    LineTo(f64, f64),
    CurveTo(f64, f64, f64, f64, f64, f64),
    Close,
}

/// The parsed SVG: its viewBox and glyphs.
struct Art {
    x: f64,
    y: f64,
    width: f64,
    height: f64,
    glyphs: Vec<Glyph>,
}

impl Art {
    /// Reads the small subset of SVG that tools/calligraphy.py writes. `None` on anything
    /// unexpected rather than a panic: the file is ours, but a bad regeneration should not
    /// take the start screen down.
    fn parse(svg: &str) -> Option<Art> {
        let view_box = attribute(svg, "viewBox=\"")?;
        let mut numbers = view_box.split_whitespace().map(str::parse::<f64>);
        let (x, y, width, height) = (
            numbers.next()?.ok()?,
            numbers.next()?.ok()?,
            numbers.next()?.ok()?,
            numbers.next()?.ok()?,
        );
        let mut glyphs = Vec::new();
        let mut rest = svg;
        while let Some(at) = rest.find("<path ") {
            let element = &rest[at..];
            let end = element.find("/>")?;
            let element = &element[..end];
            let translate = attribute(element, "translate(")?;
            let mut offsets = translate.split_whitespace().map(str::parse::<f64>);
            let (dx, dy) = (offsets.next()?.ok()?, offsets.next()?.ok()?);
            let commands = parse_path(attribute(element, "d=\"")?)?;
            glyphs.push(Glyph { dx, dy, commands });
            rest = &rest[at + end..];
        }
        (!glyphs.is_empty()).then_some(Art {
            x,
            y,
            width,
            height,
            glyphs,
        })
    }

    fn draw(&self, cr: &cairo::Context) {
        for glyph in &self.glyphs {
            cr.save().ok();
            cr.translate(glyph.dx, glyph.dy);
            for command in &glyph.commands {
                match *command {
                    Command::MoveTo(x, y) => cr.move_to(x, y),
                    Command::LineTo(x, y) => cr.line_to(x, y),
                    Command::CurveTo(x1, y1, x2, y2, x, y) => cr.curve_to(x1, y1, x2, y2, x, y),
                    Command::Close => cr.close_path(),
                }
            }
            cr.restore().ok();
        }
    }
}

/// The value after `key` up to the closing `"` or `)`.
fn attribute<'a>(text: &'a str, key: &str) -> Option<&'a str> {
    let start = text.find(key)? + key.len();
    let rest = &text[start..];
    let end = rest.find(['"', ')'])?;
    Some(&rest[..end])
}

/// Absolute M, L, C and Z only, as cairo's SVG surface writes them.
fn parse_path(d: &str) -> Option<Vec<Command>> {
    let mut commands = Vec::new();
    let mut tokens = d.split_whitespace().peekable();
    let number = |tokens: &mut std::iter::Peekable<std::str::SplitWhitespace>| -> Option<f64> {
        tokens.next()?.parse().ok()
    };
    while let Some(token) = tokens.next() {
        match token {
            "M" => commands.push(Command::MoveTo(number(&mut tokens)?, number(&mut tokens)?)),
            "L" => commands.push(Command::LineTo(number(&mut tokens)?, number(&mut tokens)?)),
            "C" => commands.push(Command::CurveTo(
                number(&mut tokens)?,
                number(&mut tokens)?,
                number(&mut tokens)?,
                number(&mut tokens)?,
                number(&mut tokens)?,
                number(&mut tokens)?,
            )),
            "Z" => commands.push(Command::Close),
            _ => return None,
        }
    }
    Some(commands)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_shipped_calligraphy_parses() {
        let art = Art::parse(SVG).expect("data/calligraphy.svg parses");
        assert_eq!(art.glyphs.len(), 2, "単 and 語");
        assert!(art.width > art.height, "wider than high");
        assert!(art.glyphs.iter().all(|g| g.commands.len() > 50));
    }

    #[test]
    fn unexpected_commands_are_refused() {
        assert!(parse_path("M 1 2 Q 3 4 5 6").is_none());
        assert_eq!(parse_path("M 1 2 L 3 4 Z").map(|c| c.len()), Some(3));
    }
}
