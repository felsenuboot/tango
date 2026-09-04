//! The libadwaita user interface.
//!
//! GTK is single-threaded, so UI objects live on the main thread inside `Rc` (reference counted,
//! not thread safe) with `RefCell` for the parts that change. A `thread_local!` holds the one
//! main window so application actions (`app.preferences`, `app.about`) can reach it.

pub mod entry_view;
pub mod import_dialog;
pub mod preferences;
pub mod theme;
pub mod window;

use std::cell::RefCell;
use std::rc::Rc;

use adw::prelude::*;
use gtk::{gdk, gio};

use crate::config::{Config, database_path};
use crate::store::db::Database;
use crate::{APP_ID, APP_NAME, VERSION};
use window::Window;

const CSS: &str = include_str!("style.css");

thread_local! {
    static WINDOW: RefCell<Option<Rc<Window>>> = const { RefCell::new(None) };
}

/// Runs `f` with the main window, if it exists yet.
pub fn with_window(f: impl FnOnce(&Rc<Window>)) {
    WINDOW.with(|w| {
        if let Some(win) = w.borrow().as_ref() {
            f(win);
        }
    });
}

pub fn startup(app: &adw::Application) {
    let display = gdk::Display::default().expect("no display");
    let css = gtk::CssProvider::new();
    css.load_from_string(CSS);
    gtk::style_context_add_provider_for_display(&display, &css, gtk::STYLE_PROVIDER_PRIORITY_APPLICATION);
    // Icons from the checkout, so `cargo run` shows the app icon without installing it.
    gtk::IconTheme::for_display(&display).add_search_path(concat!(env!("CARGO_MANIFEST_DIR"), "/data/icons"));

    let actions = [
        gio::ActionEntry::builder("quit")
            .activate(|app: &adw::Application, _, _| app.quit())
            .build(),
        gio::ActionEntry::builder("about")
            .activate(|_: &adw::Application, _, _| show_about())
            .build(),
        gio::ActionEntry::builder("preferences")
            .activate(|_: &adw::Application, _, _| with_window(|w| preferences::show(w, None)))
            .build(),
    ];
    app.add_action_entries(actions);
    app.set_accels_for_action("app.quit", &["<Control>q"]);
    app.set_accels_for_action("app.preferences", &["<Control>comma"]);
    app.set_accels_for_action("win.search", &["<Control>f", "<Control>k", "slash"]);
    app.set_accels_for_action("win.import", &["<Control>i"]);
}

pub fn activate(app: &adw::Application) {
    let existing = WINDOW.with(|w| w.borrow().clone());
    if let Some(win) = existing {
        win.win.present();
        return;
    }
    let config = Rc::new(RefCell::new(Config::load()));
    theme::apply(theme::Scheme::from_name(&config.borrow().color_scheme));
    let db_path = database_path();
    let db = match Database::open(&db_path) {
        Ok(db) => Rc::new(db),
        Err(e) => {
            log::error!("cannot open database {}: {e:#}", db_path.display());
            app.quit();
            return;
        }
    };
    let win = Window::new(app, config, db, db_path);
    WINDOW.with(|w| *w.borrow_mut() = Some(win.clone()));
    crate::autopilot::install(app, &win);
    win.win.present();
}

/// 220412 → "220,412".
pub fn thousands(n: i64) -> String {
    let digits = n.abs().to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3 + 1);
    if n < 0 {
        out.push('-');
    }
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    out
}

fn show_about() {
    let about = adw::AboutDialog::builder()
        .application_name(APP_NAME)
        .application_icon(APP_ID)
        .version(VERSION)
        .developer_name("Felix Schramm")
        .license_type(gtk::License::MitX11)
        .website("https://github.com/felsenuboot/tango")
        .comments("単語 – a Japanese dictionary for GNOME.")
        .build();
    about.add_legal_section(
        "JMdict",
        Some("© Electronic Dictionary Research and Development Group"),
        gtk::License::Custom,
        Some("Creative Commons Attribution-ShareAlike 4.0\nhttps://www.edrdg.org/edrdg/licence.html"),
    );
    with_window(|w| about.present(Some(&w.win)));
}

#[cfg(test)]
mod tests {
    #[test]
    fn thousands_separators() {
        assert_eq!(super::thousands(0), "0");
        assert_eq!(super::thousands(999), "999");
        assert_eq!(super::thousands(1000), "1,000");
        assert_eq!(super::thousands(220412), "220,412");
        assert_eq!(super::thousands(-1234567), "-1,234,567");
    }
}
