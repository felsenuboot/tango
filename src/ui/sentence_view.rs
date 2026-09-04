//! The page for one Tatoeba sentence: the Japanese, its translations, and the words the corpus
//! index found in it, each a button that opens the dictionary entry.

use std::cell::RefCell;
use std::rc::Rc;

use adw::prelude::*;

use super::entry_view::{highlight, lang_chip};
use crate::model::{Sentence, SentenceWord};

type WordCallback = Rc<RefCell<Option<Box<dyn Fn(&SentenceWord)>>>>;

pub struct SentenceView {
    root: gtk::ScrolledWindow,
    body: gtk::Box,
    on_word: WordCallback,
}

impl SentenceView {
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
            on_word: Default::default(),
        }
    }

    pub fn widget(&self) -> &gtk::ScrolledWindow {
        &self.root
    }

    pub fn connect_word(&self, f: impl Fn(&SentenceWord) + 'static) {
        *self.on_word.borrow_mut() = Some(Box::new(f));
    }

    pub fn show(&self, sentence: &Sentence, words: &[SentenceWord]) {
        while let Some(child) = self.body.first_child() {
            self.body.remove(&child);
        }
        let japanese = gtk::Label::builder()
            .use_markup(true)
            .label(highlight(&sentence.text, None))
            .xalign(0.0)
            .wrap(true)
            .selectable(true)
            .css_classes(["tango-sentence-large"])
            .build();
        self.body.append(&japanese);
        if !sentence.translations.is_empty() {
            let column = gtk::Box::new(gtk::Orientation::Vertical, 8);
            for (lang, text) in &sentence.translations {
                let line = gtk::Box::new(gtk::Orientation::Horizontal, 8);
                line.append(&lang_chip(lang));
                let label = gtk::Label::builder()
                    .label(text)
                    .xalign(0.0)
                    .wrap(true)
                    .selectable(true)
                    .hexpand(true)
                    .build();
                line.append(&label);
                column.append(&line);
            }
            self.body.append(&column);
        }
        if !words.is_empty() {
            let column = gtk::Box::new(gtk::Orientation::Vertical, 8);
            let title = gtk::Label::builder()
                .label("Words")
                .xalign(0.0)
                .css_classes(["heading"])
                .build();
            column.append(&title);
            let flow = gtk::FlowBox::builder()
                .selection_mode(gtk::SelectionMode::None)
                .homogeneous(false)
                .row_spacing(6)
                .column_spacing(6)
                .max_children_per_line(20)
                .halign(gtk::Align::Start)
                .build();
            for word in words {
                let text = match &word.reading {
                    Some(r) => format!("{} {r}", word.headword),
                    None => word.headword.clone(),
                };
                let button = gtk::Button::builder()
                    .label(text)
                    .tooltip_text(word.surface.as_deref().unwrap_or(&word.headword))
                    .css_classes(["flat"])
                    .build();
                let on_word = self.on_word.clone();
                let word = word.clone();
                button.connect_clicked(move |_| {
                    if let Some(f) = on_word.borrow().as_ref() {
                        f(&word);
                    }
                });
                flow.insert(&button, -1);
            }
            column.append(&flow);
            self.body.append(&column);
        }
        let source = gtk::LinkButton::builder()
            .label(format!("Tatoeba sentence #{}", sentence.id))
            .uri(format!("https://tatoeba.org/en/sentences/show/{}", sentence.id))
            .halign(gtk::Align::Start)
            .css_classes(["dim-label", "caption"])
            .build();
        self.body.append(&source);
        self.root.vadjustment().set_value(0.0);
    }
}
