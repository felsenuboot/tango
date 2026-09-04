//! The preferences dialog.
//!
//! There is deliberately no colour-scheme switch. The app follows the system: `adw::StyleManager`
//! reads the preference from the settings portal (or GSettings / `gtk-application-prefer-dark-theme`
//! where there is no portal). A per-app "force light" could never win anyway: GTK loads the user's
//! own `~/.config/gtk-4.0/gtk.css` above every application style provider, and desktops such as
//! Hyprland with Matugen push their whole palette through that file (issue #1).

use std::rc::Rc;

use adw::prelude::*;

use super::window::Window;

pub fn show(win: &Rc<Window>) {
    let dialog = adw::PreferencesDialog::builder().title("Preferences").build();
    let page = adw::PreferencesPage::new();
    dialog.add(&page);

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
