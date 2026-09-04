//! Scripted UI driving for screenshots and smoke tests (no input tool needed).
//!
//! Set TANGO_AUTOPILOT to a semicolon-separated script, e.g.
//!   "sleep 2; search 猫; sleep 1; select 0; sleep 2; quit"
//! Commands:
//!   sleep <seconds>      wait
//!   search <text>        type into the search entry
//!   select <index>       select the result row at that index
//!   import <path>        import a JMdict file (plain or .gz) into the database, in the background
//!   theme light|dark|system   switch the colour scheme for this run
//!   preferences | about  open that dialog
//!   resize <w> <h>       resize the main window
//!   state                log the search text, result count and selected entry
//!   quit                 exit the application

use std::collections::VecDeque;
use std::path::PathBuf;
use std::rc::{Rc, Weak};
use std::time::Duration;

use adw::prelude::*;
use gtk::glib;

use crate::ui::window::Window;

pub fn install(app: &adw::Application, win: &Rc<Window>) {
    let Some(script) = std::env::var_os("TANGO_AUTOPILOT") else {
        return;
    };
    let steps: VecDeque<String> = script
        .to_string_lossy()
        .split(';')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(String::from)
        .collect();
    schedule(app.clone(), Rc::downgrade(win), steps, Duration::from_millis(500));
}

fn schedule(app: adw::Application, win: Weak<Window>, steps: VecDeque<String>, delay: Duration) {
    glib::timeout_add_local_once(delay, move || run(app, win, steps));
}

fn run(app: adw::Application, win: Weak<Window>, mut steps: VecDeque<String>) {
    let Some(step) = steps.pop_front() else { return };
    let Some(win) = win.upgrade() else { return };
    let (cmd, arg) = step.split_once(' ').unwrap_or((step.as_str(), ""));
    let mut delay = Duration::from_millis(50);
    match cmd {
        "sleep" => delay = Duration::from_secs_f64(arg.parse().unwrap_or(1.0)),
        "search" => win.search.set_text(arg),
        "select" => win.select_result(arg.parse().unwrap_or(0)),
        "import" => win.import_file(PathBuf::from(arg)),
        "theme" => {
            let mut cfg = win.config().borrow_mut();
            cfg.color_scheme = arg.to_string();
            crate::ui::preferences::apply_color_scheme(&cfg);
        }
        "preferences" => app.activate_action("preferences", None),
        "about" => app.activate_action("about", None),
        "resize" => {
            let mut it = arg.split_whitespace().map(|v| v.parse::<i32>().unwrap_or(800));
            if let (Some(w), Some(h)) = (it.next(), it.next()) {
                win.win.set_default_size(w, h);
            }
        }
        "state" => log::info!(
            "autopilot state: search={:?} results={} selected={:?}",
            win.search.text(),
            win.result_count(),
            win.current_entry_id()
        ),
        "quit" => {
            app.quit();
            return;
        }
        other => log::warn!("autopilot: unknown step {other:?}"),
    }
    schedule(app, Rc::downgrade(&win), steps, delay);
}
