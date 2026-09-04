//! One kanji: the character with its stroke order diagram, readings, meanings, the numbers a
//! learner cares about (strokes, grade, JLPT, frequency), its radicals, and words that
//! use it.

use crate::accounts::{self, Learned};
use std::cell::RefCell;
use std::rc::Rc;

use adw::prelude::*;
use gtk::glib;

use super::strokes::Diagram;
use crate::model::{Entry, Kanji};

type WordCallback = RefCell<Option<Box<dyn Fn(&Entry)>>>;

pub struct KanjiView {
    root: gtk::ScrolledWindow,
    body: gtk::Box,
    diagram: Diagram,
    /// The word rows' entries, by row index.
    words: RefCell<Vec<Entry>>,
    on_word: WordCallback,
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

fn chip(text: &str, tooltip: &str) -> gtk::Label {
    gtk::Label::builder()
        .label(text)
        .tooltip_text(tooltip)
        .css_classes(["tango-lang"])
        .build()
}

impl KanjiView {
    pub fn new() -> Rc<Self> {
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
        Rc::new(Self {
            root,
            body,
            diagram: Diagram::new(180),
            words: RefCell::new(Vec::new()),
            on_word: RefCell::new(None),
        })
    }

    pub fn widget(&self) -> &gtk::ScrolledWindow {
        &self.root
    }

    /// What to do when a word below the kanji is activated.
    pub fn connect_word(&self, f: impl Fn(&Entry) + 'static) {
        *self.on_word.borrow_mut() = Some(Box::new(f));
    }

    /// `kanji` may be missing (no KANJIDIC2) while strokes exist, or the other way round.
    // One page, one call: every piece of data the window gathered for it.
    #[allow(clippy::too_many_arguments)]
    pub fn show(
        self: &Rc<Self>,
        literal: char,
        kanji: Option<&Kanji>,
        strokes: Option<&[String]>,
        radicals: &[String],
        words: Vec<Entry>,
        preferred: &[String],
        learned: Option<&Learned>,
    ) {
        while let Some(child) = self.body.first_child() {
            self.body.remove(&child);
        }

        // Head: the character, the diagram with its play button, and the numbers.
        let head = gtk::Box::new(gtk::Orientation::Horizontal, 24);
        head.append(&label(&literal.to_string(), &["tango-kanji-literal"]));
        if let Some(paths) = strokes {
            self.diagram.set_paths(paths);
            let column = gtk::Box::new(gtk::Orientation::Vertical, 6);
            column.append(&self.diagram.area);
            let play = gtk::Button::builder()
                .icon_name("media-playback-start-symbolic")
                .tooltip_text("Write it stroke by stroke")
                .halign(gtk::Align::Start)
                .css_classes(["flat"])
                .build();
            play.connect_clicked({
                let this = self.clone();
                move |_| this.diagram.play()
            });
            column.append(&play);
            head.append(&column);
        }
        self.body.append(&head);

        let facts = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .spacing(8)
            .build();
        if let Some(l) = learned {
            facts.append(&super::entry_view::learned_chip(
                l,
                accounts::provider_name(&l.provider),
            ));
        }
        if let Some(k) = kanji {
            facts.append(&chip(&format!("{} strokes", k.strokes), "Stroke count"));
            if let Some(grade) = k.grade {
                let text = match grade {
                    1..=6 => format!("Grade {grade}"),
                    8 => "Jōyō (secondary school)".to_string(),
                    9 | 10 => "Jinmeiyō (names)".to_string(),
                    g => format!("Grade {g}"),
                };
                facts.append(&chip(&text, "School grade in which the kanji is taught"));
            }
            if let Some(jlpt) = k.jlpt {
                facts.append(&chip(
                    &format!("JLPT {jlpt}"),
                    "Old four-level JLPT rating (1 hardest)",
                ));
            }
            if let Some(freq) = k.freq {
                facts.append(&chip(
                    &format!("Frequency {freq}"),
                    "Rank among the 2,500 most used kanji in newspapers",
                ));
            }
            facts.append(&chip(
                &format!("Radical {}", k.radical),
                "Classical (Kangxi) radical number",
            ));
        } else if let Some(paths) = strokes {
            facts.append(&chip(&format!("{} strokes", paths.len()), "Stroke count"));
        }
        if facts.first_child().is_some() {
            self.body.append(&facts);
        }

        if let Some(k) = kanji {
            let grid = gtk::Grid::builder().column_spacing(12).row_spacing(6).build();
            let mut row = 0;
            let mut add_row = |name: &str, value: String| {
                if value.is_empty() {
                    return;
                }
                let name = gtk::Label::builder()
                    .label(name)
                    .xalign(0.0)
                    .valign(gtk::Align::Start)
                    .css_classes(["dim-label"])
                    .build();
                let value = label(&value, &["tango-reading"]);
                value.set_hexpand(true);
                grid.attach(&name, 0, row, 1, 1);
                grid.attach(&value, 1, row, 1, 1);
                row += 1;
            };
            add_row("On", k.on.join("、"));
            add_row("Kun", k.kun.join("、"));
            add_row("Nanori", k.nanori.join("、"));
            self.body.append(&grid);

            // Meanings in the preferred languages that exist, English always.
            let mut langs: Vec<&str> = preferred.iter().map(String::as_str).collect();
            if !langs.contains(&"eng") {
                langs.push("eng");
            }
            for lang in langs {
                let meanings = k.meanings_in(lang);
                if meanings.is_empty() {
                    continue;
                }
                let line = gtk::Box::new(gtk::Orientation::Horizontal, 8);
                let tag = chip(&super::entry_view::lang_label(lang), "");
                tag.set_valign(gtk::Align::Start);
                tag.set_margin_top(3);
                line.append(&tag);
                let text = label(&meanings.join("; "), &[]);
                text.set_hexpand(true);
                line.append(&text);
                self.body.append(&line);
            }
        }

        if !radicals.is_empty() {
            let line = gtk::Box::new(gtk::Orientation::Horizontal, 6);
            line.append(
                &gtk::Label::builder()
                    .label("Parts")
                    .css_classes(["dim-label"])
                    .build(),
            );
            for r in radicals {
                line.append(&chip(r, "Radical (RADKFILE)"));
            }
            self.body.append(&line);
        }

        if kanji.is_none() && strokes.is_none() {
            self.body.append(&label(
                "No kanji data yet. Install KANJIDIC2 and KanjiVG from the Dictionaries page in Preferences.",
                &["dim-label"],
            ));
        }

        if !words.is_empty() {
            self.body.append(
                &gtk::Label::builder()
                    .label("Words with this kanji")
                    .xalign(0.0)
                    .css_classes(["heading"])
                    .build(),
            );
            let list = gtk::ListBox::builder()
                .selection_mode(gtk::SelectionMode::None)
                .css_classes(["boxed-list"])
                .build();
            for e in &words {
                let mut title = glib::markup_escape_text(e.headword()).to_string();
                if !e.kanji.is_empty() && !e.reading().is_empty() {
                    title.push_str(&format!(
                        "  <span alpha='70%'>{}</span>",
                        glib::markup_escape_text(e.reading())
                    ));
                }
                let row = adw::ActionRow::builder()
                    .activatable(true)
                    .use_markup(true)
                    .title(&title)
                    .subtitle(glib::markup_escape_text(e.summary(preferred)).as_str())
                    .title_lines(1)
                    .subtitle_lines(1)
                    .build();
                list.append(&row);
            }
            list.connect_row_activated({
                let this = self.clone();
                move |_, row| {
                    let entry = this.words.borrow().get(row.index() as usize).cloned();
                    if let (Some(entry), Some(f)) = (entry, this.on_word.borrow().as_ref()) {
                        f(&entry);
                    }
                }
            });
            self.body.append(&list);
        }
        *self.words.borrow_mut() = words;
        self.root.vadjustment().set_value(0.0);
    }
}
