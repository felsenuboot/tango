//! One dictionary entry: headword, readings, then the senses with their glosses per language.

use adw::prelude::*;

use crate::model::{Entry, Sense};

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
        let available = entry.languages();
        let mut langs: Vec<&str> = preferred
            .iter()
            .map(String::as_str)
            .filter(|l| available.contains(l))
            .collect();
        let extra: Vec<&str> = available.iter().copied().filter(|l| !langs.contains(l)).collect();
        langs.extend(extra);

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
        for (i, sense) in entry.senses.iter().enumerate() {
            self.body.append(&sense_row(i + 1, sense, &langs));
        }
        self.root.vadjustment().set_value(0.0);
    }
}

fn sense_row(number: usize, sense: &Sense, langs: &[&str]) -> gtk::Box {
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
    for lang in langs {
        let text = sense.gloss_text(lang);
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
        let gloss = label(&text, &[]);
        gloss.set_hexpand(true);
        line.append(&gloss);
        body.append(&line);
    }
    row.append(&body);
    row
}
