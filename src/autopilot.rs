//! Scripted UI driving for screenshots and smoke tests (no input tool needed).
//!
//! Set TANGO_AUTOPILOT to a semicolon-separated script, e.g.
//!   "sleep 2; search 猫; sleep 1; select 0; sleep 2; quit"
//! Commands:
//!   sleep <seconds>      wait
//!   search <text>        type into the search entry
//!   select <index>       select the result row at that index
//!   import [source] <path>   queue an import of a dictionary file (JMdict when no source id is given)
//!   wait                 wait until the job queue (downloads, imports) is empty
//!   theme light|dark|system   switch the colour scheme for this run, without saving it
//!   kanji <char>         show the kanji page for that character
//!   back                 go back to the previous view (the header's back button)
//!   radical <r>          toggle that radical on the Kanji sidebar page
//!   hide on|off          hide (or grey out) the parts that no longer fit on the Kanji page
//!   wanikani sync|disconnect|connect <token>   the WaniKani account (TANGO_WANIKANI_TOKEN works for sync)
//!   star                 toggle the current entry in Favourites
//!   sidebar search|lists show that sidebar page
//!   list <name>          open that word list in the sidebar
//!   pick <n>             open the n-th entry of the open list
//!   filter <kind> <level> <stage>   rows of the WaniKani list's three drop-downs
//!   grid cards|tiles|search on|off   the grid-view preferences (#83), for this run only
//!   lists                log the word lists and their entry counts
//!   preferences [general|dictionaries]   open the preferences, on that page
//!   about                open the about dialog
//!   menu                 open the primary menu (or close it, if open)
//!   menustate            log the menu button's position and the popover's scroll metrics
//!   resize <w> <h>       resize the main window
//!   fullscreen | maximize | unfullscreen   change the window state
//!   scroll top|end       scroll the content pane (entry, kanji or sentence page)
//!   state                log the search text, result count and selected entry
//!   quit                 exit the application

use std::collections::VecDeque;
use std::path::PathBuf;
use std::rc::{Rc, Weak};
use std::time::Duration;

use adw::prelude::*;
use gtk::glib;

use crate::ui::window::{Window, find_scrolled_window};

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
        "import" => {
            let (source, path) = match arg.split_once(' ') {
                Some((id, path)) if crate::dict::sources::by_id(id).is_some() => (
                    crate::dict::sources::by_id(id).unwrap_or(&crate::dict::sources::JMDICT),
                    path,
                ),
                _ => (&crate::dict::sources::JMDICT, arg),
            };
            win.import_source_file(source, PathBuf::from(path), || {});
        }
        "wait" => {
            if win.jobs().is_busy() {
                steps.push_front("wait".into());
                delay = Duration::from_millis(500);
            }
        }
        "theme" => crate::ui::theme::apply(crate::ui::theme::Scheme::from_name(arg)),
        "preferences" => crate::ui::preferences::show(&win, (!arg.is_empty()).then_some(arg)),
        "menu" => {
            // Toggles, so a script can close and reopen the menu.
            if win.menu_button.popover().is_some_and(|p| p.is_visible()) {
                win.menu_button.popdown();
                schedule(app, Rc::downgrade(&win), steps, delay);
                return;
            }
            win.menu_button.popup();
            let button = win.menu_button.clone();
            glib::timeout_add_local_once(Duration::from_millis(400), move || log_menu_metrics(&button));
        }
        "menustate" => log_menu_metrics(&win.menu_button),
        "star" => win.toggle_favourite(),
        "kanji" => {
            if let Some(c) = arg.chars().next() {
                win.show_kanji(c);
            }
        }
        "back" => win.go_back(),
        "sidebar" => win.show_sidebar_page(arg),
        "radical" => win.radicals_page().toggle(arg),
        "hide" => win.radicals_page().set_hide(arg == "on"),
        "wanikani" => match arg.split_once(' ').unwrap_or((arg, "")) {
            ("sync", _) => win.sync_wanikani(|| {}),
            ("disconnect", _) => win.disconnect_wanikani(|| {}),
            ("connect", token) => win.connect_wanikani(token.to_string(), || {}),
            other => log::warn!("autopilot: unknown wanikani step {other:?}"),
        },
        "list" if arg == "WaniKani" => {
            win.show_sidebar_page("lists");
            win.lists_page().open_list(crate::ui::lists::WANIKANI_LIST);
        }
        "pick" => win.lists_page().open_index(arg.parse().unwrap_or(0)),
        "grid" => {
            let (key, value) = arg.split_once(' ').unwrap_or((arg, "on"));
            let on = value == "on";
            {
                let mut cfg = win.config().borrow_mut();
                match key {
                    "cards" => cfg.list_cards = on,
                    "tiles" => cfg.list_tiles = on,
                    "search" => cfg.grid_search = on,
                    other => log::warn!("autopilot: unknown grid setting {other:?}"),
                }
            }
            win.lists_page().refresh_view(on || win.cards_visible());
            win.refresh_search();
        }
        "filter" => {
            let mut rows = arg.split_whitespace().map(|s| s.parse::<u32>().unwrap_or(0));
            let (kind, level, stage) = (
                rows.next().unwrap_or(0),
                rows.next().unwrap_or(0),
                rows.next().unwrap_or(0),
            );
            win.lists_page().set_filters(kind, level, stage);
        }
        "list" => match win.user().list_by_name(arg) {
            Ok(Some(list)) => {
                win.show_sidebar_page("lists");
                win.lists_page().open_list(list.id);
            }
            other => log::warn!("autopilot: list {arg:?}: {other:?}"),
        },
        "lists" => match win.user().lists() {
            Ok(lists) => {
                for l in lists {
                    log::info!("autopilot list: {} ({} entries)", l.name, l.entries);
                }
            }
            Err(e) => log::warn!("autopilot: lists: {e:#}"),
        },
        "fullscreen" => win.win.fullscreen(),
        "maximize" => win.win.maximize(),
        "unfullscreen" => {
            win.win.unfullscreen();
            win.win.unmaximize();
        }
        "about" => app.activate_action("about", None),
        "resize" => {
            let mut it = arg.split_whitespace().map(|v| v.parse::<i32>().unwrap_or(800));
            if let (Some(w), Some(h)) = (it.next(), it.next()) {
                win.win.set_default_size(w, h);
            }
        }
        "scroll" => win.scroll_content(arg == "end"),
        "state" => log::info!(
            "autopilot state: search={:?} results={} selected={:?} learned={} jobs={}",
            win.search.text(),
            win.result_count(),
            win.current_entry_id(),
            win.user().learned_count("wanikani").unwrap_or(0),
            win.jobs().current().map_or("idle".to_string(), |c| format!(
                "{} ({}), {} queued",
                c.title,
                c.message,
                win.jobs().queued()
            ))
        ),
        "quit" => {
            app.quit();
            return;
        }
        other => log::warn!("autopilot: unknown step {other:?}"),
    }
    schedule(app, Rc::downgrade(&win), steps, delay);
}

/// Logs where the menu button is and whether its popover got a scrollable area (Hyprland shrinks
/// it for fullscreen windows, see `keep_menu_out_of_reserved_strip` in ui/window.rs).
fn log_menu_metrics(button: &gtk::MenuButton) {
    let bounds = button
        .root()
        .and_then(|root| button.compute_bounds(root.upcast_ref::<gtk::Widget>()))
        .map(|b| format!("{},{} {}x{}", b.x(), b.y(), b.width(), b.height()));
    let Some(popover) = button.popover() else { return };
    let Some(scrolled) = find_scrolled_window(popover.upcast_ref()) else {
        log::info!("autopilot menu: button at {bounds:?}, no scrolled window in the popover");
        return;
    };
    let adj = scrolled.vadjustment();
    let child_natural = scrolled
        .child()
        .map(|c| c.measure(gtk::Orientation::Vertical, -1).1);
    log::info!(
        "autopilot menu: button at {bounds:?} visible={} popover {}x{} scrolled {}x{} content upper={} page={} child natural height={:?}",
        popover.is_visible(),
        popover.width(),
        popover.height(),
        scrolled.width(),
        scrolled.height(),
        adj.upper(),
        adj.page_size(),
        child_natural
    );
}
