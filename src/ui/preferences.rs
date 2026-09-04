use std::rc::Rc;

use adw::prelude::*;

use super::window::Window;
use crate::config::Config;

const SCHEMES: [(&str, adw::ColorScheme); 3] = [
    ("system", adw::ColorScheme::Default),
    ("light", adw::ColorScheme::ForceLight),
    ("dark", adw::ColorScheme::ForceDark),
];

pub fn apply_color_scheme(config: &Config) {
    let scheme = SCHEMES
        .iter()
        .find(|(name, _)| *name == config.color_scheme)
        .map_or(adw::ColorScheme::Default, |(_, s)| *s);
    adw::StyleManager::default().set_color_scheme(scheme);
}

pub fn show(win: &Rc<Window>) {
    let dialog = adw::PreferencesDialog::builder().title("Preferences").build();
    let page = adw::PreferencesPage::new();
    dialog.add(&page);

    let look = adw::PreferencesGroup::builder().title("Appearance").build();
    page.add(&look);
    let scheme = adw::ComboRow::builder()
        .title("Colour scheme")
        .model(&gtk::StringList::new(&["Follow system", "Light", "Dark"]))
        .build();
    let current = win.config().borrow().color_scheme.clone();
    scheme.set_selected(SCHEMES.iter().position(|(n, _)| *n == current).unwrap_or(0) as u32);
    scheme.connect_selected_notify({
        let win = win.clone();
        move |row| {
            let mut cfg = win.config().borrow_mut();
            cfg.color_scheme = SCHEMES[row.selected() as usize].0.to_string();
            cfg.save();
            apply_color_scheme(&cfg);
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
