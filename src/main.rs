//! Tango (単語): a Japanese dictionary for the GNOME desktop.
//!
//! Module map, roughly in the order the data flows:
//!   dict/      readers for the dictionary source files and where to download them
//!   model.rs   `Entry` and `Sense`, the plain data every other module passes around
//!   store/     the SQLite database: import, lookup and search
//!   search/    the search pipeline: romaji to kana, deinflection, then the database
//!   ui/        the libadwaita widgets
//!   config.rs  the JSON preferences file and the XDG directories
//!   autopilot.rs  scripted UI driving for screenshots and smoke tests

mod autopilot;
mod config;
mod dict;
mod model;
mod search;
mod store;
mod ui;

use gtk::{gio, glib, prelude::*};

pub const APP_ID: &str = "io.github.felsenuboot.Tango";
pub const APP_NAME: &str = "Tango";
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

fn main() -> glib::ExitCode {
    // RUST_LOG=debug for more; TANGO_DEBUG=1 is the short form.
    let level = if std::env::var_os("TANGO_DEBUG").is_some() {
        "debug"
    } else {
        "info"
    };
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or(level)).init();

    if std::env::args().any(|a| a == "--version") {
        println!("{APP_NAME} {VERSION}");
        return glib::ExitCode::SUCCESS;
    }

    let mut flags = gio::ApplicationFlags::empty();
    if std::env::var_os("TANGO_AUTOPILOT").is_some() {
        // Test instances must not join a running desktop instance.
        flags |= gio::ApplicationFlags::NON_UNIQUE;
    }
    let app = adw::Application::builder()
        .application_id(APP_ID)
        .flags(flags)
        .build();
    app.connect_startup(ui::startup);
    app.connect_activate(ui::activate);
    // `run()` parses the command line itself, so pass nothing and it uses std::env::args.
    app.run_with_args::<&str>(&[])
}
