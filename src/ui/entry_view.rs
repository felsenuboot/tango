//! One dictionary entry: headword, readings, then the senses with their glosses per language.

use adw::prelude::*;

use crate::dict::sources;
use crate::model::{Entry, LanguageBlock, Sense};

fn lang_name(lang: &str) -> String {
    match lang {
        "eng" => "English",
        "ger" => "German",
        "dut" => "Dutch",
        "fre" => "French",
        "rus" => "Russian",
        "spa" => "Spanish",
        "hun" => "Hungarian",
        "slv" => "Slovenian",
        "swe" => "Swedish",
        other => return other.to_uppercase(),
    }
    .to_string()
}

pub fn lang_label(lang: &str) -> String {
    match lang {
        "eng" => "EN",
        "ger" => "DE",
        "dut" => "NL",
        "fre" => "FR",
        "rus" => "RU",
        "spa" => "ES",
        "hun" => "HU",
        "slv" => "SL",
        "swe" => "SV",
        other => return other.to_uppercase(),
    }
    .to_string()
}

fn label(text: &str, css: &[&str]) -> gtk::Label {
    gtk::Label::builder()
        .label(text)
        .xalign(0.0)
        .wrap(true)
        .selectable(true)
        .css_classes(css)
        .build()
}

type KanjiCallback = std::rc::Rc<std::cell::RefCell<Option<Box<dyn Fn(char)>>>>;

pub struct EntryView {
    root: gtk::ScrolledWindow,
    body: gtk::Box,
    /// What happens when a kanji in the headword is clicked.
    on_kanji: KanjiCallback,
}

pub fn is_kanji(c: char) -> bool {
    matches!(c, '\u{4E00}'..='\u{9FFF}' | '\u{3400}'..='\u{4DBF}' | '々')
}

impl EntryView {
    pub fn new() -> Self {
        let body = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(18)
            .margin_top(24)
            .margin_bottom(24)
            .margin_start(18)
            .margin_end(18)
            .build();
        let clamp = adw::Clamp::builder().child(&body).maximum_size(760).build();
        let root = gtk::ScrolledWindow::builder()
            .child(&clamp)
            .hscrollbar_policy(gtk::PolicyType::Never)
            .vexpand(true)
            .build();
        Self {
            root,
            body,
            on_kanji: Default::default(),
        }
    }

    pub fn widget(&self) -> &gtk::ScrolledWindow {
        &self.root
    }

    pub fn connect_kanji(&self, f: impl Fn(char) + 'static) {
        *self.on_kanji.borrow_mut() = Some(Box::new(f));
    }

    /// The headword with every kanji as a button, the rest as text.
    fn headword(&self, text: &str) -> gtk::Box {
        let row = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        let mut run = String::new();
        let flush = |run: &mut String, row: &gtk::Box| {
            if !run.is_empty() {
                row.append(&label(run, &["tango-headword"]));
                run.clear();
            }
        };
        for c in text.chars() {
            if is_kanji(c) {
                flush(&mut run, &row);
                let button = gtk::Button::builder()
                    .label(c.to_string())
                    .tooltip_text("Show this kanji")
                    .css_classes(["flat", "tango-headword-kanji"])
                    .build();
                let on_kanji = self.on_kanji.clone();
                button.connect_clicked(move |_| {
                    if let Some(f) = on_kanji.borrow().as_ref() {
                        f(c);
                    }
                });
                row.append(&button);
            } else {
                run.push(c);
            }
        }
        flush(&mut run, &row);
        row
    }

    /// `preferred` is the configured language order; languages the entry has beyond that follow.
    pub fn show(&self, entry: &Entry, preferred: &[String]) {
        while let Some(child) = self.body.first_child() {
            self.body.remove(&child);
        }
        let head = gtk::Box::new(gtk::Orientation::Horizontal, 12);
        head.append(&self.headword(entry.headword()));
        if entry.common {
            let tag = gtk::Label::builder()
                .label("common")
                .valign(gtk::Align::End)
                .margin_bottom(10)
                .css_classes(["tango-common"])
                .build();
            head.append(&tag);
        }
        if entry.source != "jmdict"
            && let Some(source) = sources::by_id(&entry.source)
        {
            let tag = gtk::Label::builder()
                .label(source.name)
                .valign(gtk::Align::End)
                .margin_bottom(10)
                .tooltip_text(source.licence)
                .css_classes(["tango-lang"])
                .build();
            head.append(&tag);
        }
        self.body.append(&head);
        let readings: &[String] = if entry.kanji.is_empty() {
            &entry.readings[1..]
        } else {
            &entry.readings
        };
        if !readings.is_empty() || !entry.pitch.is_empty() {
            let line = gtk::Box::new(gtk::Orientation::Horizontal, 8);
            if !readings.is_empty() {
                line.append(&label(&readings.join("、"), &["tango-reading"]));
            }
            for pitch in &entry.pitch {
                line.append(&pitch_chip(*pitch));
            }
            self.body.append(&line);
        }
        if entry.kanji.len() > 1 {
            let also = format!("Also written {}", entry.kanji[1..].join("、"));
            self.body.append(&label(&also, &["dim-label"]));
        }
        let grouped = entry.grouped(preferred);
        for (i, meaning) in grouped.meanings.iter().enumerate() {
            self.body
                .append(&sense_row(i + 1, meaning.sense, &meaning.glosses));
        }
        if !grouped.blocks.is_empty() {
            self.body.append(&label(
                "JMdict splits the following languages into senses of their own, so they are listed \
                 separately from the meanings above.",
                &["dim-label", "caption"],
            ));
            for block in &grouped.blocks {
                self.body.append(&language_block(block));
            }
        }
        self.root.vadjustment().set_value(0.0);
    }
}

/// ⓪ ① ② … for the accent number, with the meaning in the tooltip.
fn pitch_chip(pitch: u8) -> gtk::Label {
    let symbol = match pitch {
        0 => '⓪',
        1..=20 => char::from_u32(0x2460 + u32::from(pitch) - 1).unwrap_or('?'),
        _ => '?',
    };
    let tooltip = if pitch == 0 {
        "Pitch accent: flat (heiban), no drop".to_string()
    } else {
        format!("Pitch accent: the pitch drops after mora {pitch}")
    };
    gtk::Label::builder()
        .label(symbol.to_string())
        .valign(gtk::Align::Center)
        .tooltip_text(tooltip)
        .css_classes(["tango-pitch"])
        .build()
}

fn language_block(block: &LanguageBlock) -> gtk::Box {
    let column = gtk::Box::new(gtk::Orientation::Vertical, 10);
    let title = gtk::Label::builder()
        .label(lang_name(block.lang))
        .xalign(0.0)
        .css_classes(["heading"])
        .build();
    column.append(&title);
    for (i, sense) in block.senses.iter().enumerate() {
        let glosses = [(block.lang, sense.gloss_text(block.lang))];
        column.append(&sense_row(i + 1, sense, &glosses));
    }
    column
}

/// One numbered meaning: its parts of speech and notes, then a line per language.
fn sense_row(number: usize, sense: &Sense, glosses: &[(&str, String)]) -> gtk::Box {
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    let num = gtk::Label::builder()
        .label(format!("{number}."))
        .xalign(0.0)
        .valign(gtk::Align::Start)
        .css_classes(["tango-sense-number"])
        .build();
    row.append(&num);
    let body = gtk::Box::builder()
        .orientation(gtk::Orientation::Vertical)
        .spacing(4)
        .hexpand(true)
        .build();
    let meta: Vec<&str> = sense
        .pos
        .iter()
        .chain(&sense.fields)
        .chain(&sense.misc)
        .map(String::as_str)
        .collect();
    if !meta.is_empty() {
        let meta_label = gtk::Label::builder()
            .label(meta.join(", "))
            .xalign(0.0)
            .wrap(true)
            .css_classes(["dim-label"])
            .build();
        body.append(&meta_label);
    }
    for (lang, text) in glosses {
        if text.is_empty() {
            continue;
        }
        let line = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        let tag = gtk::Label::builder()
            .label(lang_label(lang))
            .valign(gtk::Align::Start)
            .margin_top(3)
            .css_classes(["tango-lang"])
            .build();
        line.append(&tag);
        let gloss = label(text, &[]);
        gloss.set_hexpand(true);
        line.append(&gloss);
        body.append(&line);
    }
    row.append(&body);
    row
}
