//! Grid views (issue #83): a word list, or a search result, as cards in the content pane and
//! as tiles in the sidebar. Both are preferences (`Config::list_cards`, `Config::list_tiles`),
//! and `Config::grid_search` extends them to search results.
//!
//! Cards and tiles are built from the same rows the sidebar shows (`ListEntry` plus what
//! WaniKani knows about the item), so a WaniKani level is a wall of stage-tinted cards and
//! Favourites a wall of words with the day they were added.

use std::rc::{Rc, Weak};

use adw::prelude::*;

use super::window::Window;
use crate::accounts::{self, Kind, Learned};
use crate::store::user::ListEntry;

/// "Show N more" after the last card or tile, for the paged WaniKani list.
pub struct More {
    pub remaining: usize,
    pub load: Rc<dyn Fn()>,
}

/// A translucent wash of the stage colour for a card or tile; the chips keep the solid fill.
fn stage_wash(l: &Learned) -> &'static str {
    match l.stage {
        0 => "tango-wash-locked",
        1..=4 => "tango-wash-apprentice",
        5 | 6 => "tango-wash-guru",
        7 => "tango-wash-master",
        8 => "tango-wash-enlightened",
        _ => "tango-wash-burned",
    }
}

fn label(text: &str, classes: &[&str], xalign: f32) -> gtk::Label {
    gtk::Label::builder()
        .label(text)
        .xalign(xalign)
        .max_width_chars(18)
        .ellipsize(gtk::pango::EllipsizeMode::End)
        .css_classes(classes)
        .build()
}

/// One chip per SRS stage with its count, when the items carry stages.
fn stage_strip(learned: &[Option<Learned>]) -> Option<gtk::Box> {
    if learned.iter().all(Option::is_none) {
        return None;
    }
    let mut counts = [0usize; 6];
    for l in learned.iter().flatten() {
        counts[match l.stage {
            0 => 0,
            1..=4 => 1,
            5 | 6 => 2,
            7 => 3,
            8 => 4,
            _ => 5,
        }] += 1;
    }
    let strip = gtk::Box::builder().spacing(6).margin_top(6).build();
    let names = ["Locked", "Apprentice", "Guru", "Master", "Enlightened", "Burned"];
    let stages = [0u8, 1, 5, 7, 8, 9];
    for (i, name) in names.iter().enumerate() {
        if counts[i] > 0 {
            strip.append(
                &gtk::Label::builder()
                    .label(format!("{name} {}", counts[i]))
                    .css_classes(["tango-learned", accounts::stage_class(stages[i])])
                    .build(),
            );
        }
    }
    Some(strip)
}

fn activate(win: &Weak<Window>, entries: &[ListEntry], more: &Option<More>, index: usize) {
    let Some(win) = win.upgrade() else { return };
    if let Some(e) = entries.get(index) {
        win.open_list_entry(e);
    } else if let Some(more) = more {
        (more.load)();
    }
}

/// Cards in the content pane, under a title, a line of context and the stage counts.
pub fn cards(
    win: &Weak<Window>,
    title: &str,
    subtitle: &str,
    entries: &[ListEntry],
    learned: &[Option<Learned>],
    more: Option<More>,
) -> gtk::Widget {
    let page = gtk::Box::new(gtk::Orientation::Vertical, 0);
    let head = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(6)
        .margin_bottom(12)
        .build();
    let heading = label(title, &["title-1"], 0.0);
    heading.set_max_width_chars(-1);
    head.append(&heading);
    let context = label(subtitle, &["dim-label"], 0.0);
    context.set_max_width_chars(-1);
    head.append(&context);
    if let Some(strip) = stage_strip(learned) {
        head.append(&strip);
    }
    page.append(&head);

    let flow = gtk::FlowBox::builder()
        .selection_mode(gtk::SelectionMode::None)
        .homogeneous(true)
        .min_children_per_line(2)
        .max_children_per_line(5)
        .column_spacing(12)
        .row_spacing(12)
        .valign(gtk::Align::Start)
        .build();
    for (e, l) in entries.iter().zip(learned) {
        let card = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(2)
            .width_request(180)
            .css_classes(["tango-card"])
            .build();
        if let Some(l) = l {
            card.add_css_class(stage_wash(l));
        }
        card.append(&label(&e.headword, &["tango-card-word"], 0.0));
        // A zero-width space keeps the line, so cards line up whether or not there is a reading.
        let reading = if e.reading.is_empty() {
            "\u{200b}"
        } else {
            &e.reading
        };
        card.append(&label(reading, &["dim-label"], 0.0));
        let gloss = label(&e.gloss, &["caption"], 0.0);
        gloss.set_margin_top(4);
        card.append(&gloss);
        let foot = gtk::Box::builder().margin_top(8).spacing(6).build();
        match l {
            Some(l) => foot.append(&super::entry_view::learned_chip(l, "")),
            None => foot.append(&label(
                e.added.get(..10).unwrap_or(""),
                &["caption", "dim-label"],
                0.0,
            )),
        }
        card.append(&foot);
        flow.insert(&card, -1);
    }
    if let Some(more) = &more {
        let card = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .valign(gtk::Align::Center)
            .css_classes(["tango-card", "tango-card-more"])
            .build();
        card.append(&label(
            &format!("Show {} more", more.remaining.min(super::lists::PAGE)),
            &["heading"],
            0.5,
        ));
        card.append(&label(
            &format!("{} not shown", more.remaining),
            &["caption", "dim-label"],
            0.5,
        ));
        flow.insert(&card, -1);
    }
    let entries: Vec<ListEntry> = entries.to_vec();
    let win = win.clone();
    flow.connect_child_activated(move |_, child| activate(&win, &entries, &more, child.index() as usize));
    page.append(&flow);

    let clamp = adw::Clamp::builder()
        .maximum_size(1100)
        .tightening_threshold(900)
        .margin_start(24)
        .margin_end(24)
        .margin_top(24)
        .margin_bottom(24)
        .child(&page)
        .build();
    gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .child(&clamp)
        .vexpand(true)
        .build()
        .upcast()
}

/// Tiles in the sidebar: the word, and its stage or its reading; the meaning is the tooltip.
pub fn tiles(
    win: &Weak<Window>,
    entries: &[ListEntry],
    learned: &[Option<Learned>],
    more: Option<More>,
) -> gtk::FlowBox {
    let flow = gtk::FlowBox::builder()
        .selection_mode(gtk::SelectionMode::None)
        .homogeneous(true)
        .min_children_per_line(3)
        .max_children_per_line(4)
        .column_spacing(6)
        .row_spacing(6)
        .margin_start(12)
        .margin_end(12)
        .margin_bottom(12)
        .valign(gtk::Align::Start)
        .build();
    for (e, l) in entries.iter().zip(learned) {
        let tile = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .css_classes(["tango-tile"])
            .tooltip_text(format!("{} {}\n{}", e.headword, e.reading, e.gloss).trim())
            .build();
        if let Some(l) = l {
            tile.add_css_class(stage_wash(l));
            if l.kind == Kind::Kanji {
                tile.add_css_class("tango-tile-kanji");
            }
        }
        tile.append(&label(&e.headword, &["tango-tile-word"], 0.5));
        let sub = match l {
            Some(l) => format!("{} · {}", l.level, accounts::stage_name(l.stage)),
            None => e.reading.clone(),
        };
        tile.append(&label(&sub, &["caption", "dim-label"], 0.5));
        flow.insert(&tile, -1);
    }
    if let Some(more) = &more {
        let tile = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .valign(gtk::Align::Center)
            .css_classes(["tango-tile", "tango-card-more"])
            .build();
        tile.append(&label(&format!("{} more", more.remaining), &["heading"], 0.5));
        tile.append(&label("Show", &["caption", "dim-label"], 0.5));
        flow.insert(&tile, -1);
    }
    let entries: Vec<ListEntry> = entries.to_vec();
    let win = win.clone();
    flow.connect_child_activated(move |_, child| activate(&win, &entries, &more, child.index() as usize));
    flow
}
