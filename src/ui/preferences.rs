//! The preferences dialog.

use std::rc::Rc;

use adw::prelude::*;

use super::theme::{self, Scheme};
use super::window::Window;

pub fn show(win: &Rc<Window>) {
    let dialog = adw::PreferencesDialog::builder().title("Preferences").build();
    let page = adw::PreferencesPage::new();
    dialog.add(&page);

    let look = adw::PreferencesGroup::builder().title("Appearance").build();
    page.add(&look);
    let scheme = adw::ComboRow::builder()
        .title("Colour scheme")
        .subtitle(
            "Light and Dark use libadwaita's own colours; Follow system also takes the desktop's GTK theme.",
        )
        .model(&gtk::StringList::new(&Scheme::ALL.map(Scheme::label)))
        .build();
    let current = Scheme::from_name(&win.config().borrow().color_scheme);
    scheme.set_selected(Scheme::ALL.iter().position(|s| *s == current).unwrap_or(0) as u32);
    scheme.connect_selected_notify({
        let win = win.clone();
        move |row| {
            let chosen = Scheme::ALL[row.selected() as usize];
            {
                let mut cfg = win.config().borrow_mut();
                cfg.color_scheme = chosen.name().to_string();
                cfg.save();
            }
            theme::apply(chosen);
        }
    });
    look.add(&scheme);

    let glosses = adw::PreferencesGroup::builder()
        .title("Glosses")
        .description("Which translation comes first in an entry.")
        .build();
    page.add(&glosses);
    let first = adw::ComboRow::builder()
        .title("Preferred language")
        .model(&gtk::StringList::new(&["English", "German"]))
        .build();
    let langs = win.config().borrow().gloss_languages.clone();
    first.set_selected(if langs.first().map(String::as_str) == Some("eng") {
        0
    } else {
        1
    });
    first.connect_selected_notify({
        let win = win.clone();
        move |row| {
            {
                let mut cfg = win.config().borrow_mut();
                cfg.gloss_languages = if row.selected() == 0 {
                    vec!["eng".into(), "ger".into()]
                } else {
                    vec!["ger".into(), "eng".into()]
                };
                cfg.save();
            }
            win.rerender();
        }
    });
    glosses.add(&first);

    dialog.present(Some(&win.win));
}
