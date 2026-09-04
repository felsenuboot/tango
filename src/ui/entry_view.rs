//! One dictionary entry: headword, readings, then the senses with their glosses per language.

use adw::prelude::*;

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

fn lang_label(lang: &str) -> String {
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

pub struct EntryView {
    root: gtk::ScrolledWindow,
    body: gtk::Box,
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
        Self { root, body }
    }

    pub fn widget(&self) -> &gtk::ScrolledWindow {
        &self.root
    }

    /// `preferred` is the configured language order; languages the entry has beyond that follow.
    pub fn show(&self, entry: &Entry, preferred: &[String]) {
        while let Some(child) = self.body.first_child() {
            self.body.remove(&child);
        }
        let head = gtk::Box::new(gtk::Orientation::Horizontal, 12);
        head.append(&label(entry.headword(), &["tango-headword"]));
        if entry.common {
            let tag = gtk::Label::builder()
                .label("common")
                .valign(gtk::Align::End)
                .margin_bottom(10)
                .css_classes(["tango-common"])
                .build();
            head.append(&tag);
        }
        self.body.append(&head);
        let readings: &[String] = if entry.kanji.is_empty() {
            &entry.readings[1..]
        } else {
            &entry.readings
        };
        if !readings.is_empty() {
            self.body.append(&label(&readings.join("、"), &["tango-reading"]));
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
