//! The preferences dialog: a General page (appearance, glosses), a Dictionaries page that lists
//! every source Tango knows, installed or not, with update, remove and search toggles, and an
//! Accounts page for the learning sites (WaniKani).

use std::rc::Rc;

use adw::prelude::*;
use gtk::glib::{self, clone};

use super::jobs::State;
use super::theme::{self, Scheme};
use super::thousands;
use super::window::Window;
use crate::config::cache_dir;
use crate::dict::sources::{self, Source};
use crate::model::language_name;
use crate::store::db::SourceStatus;

/// Opens the dialog, on the page named `page` ("general", "dictionaries") if given.
pub fn show(win: &Rc<Window>, page: Option<&str>) {
    let dialog = adw::PreferencesDialog::builder().title("Preferences").build();
    dialog.add(&general_page(win));
    dialog.add(&dictionaries_page(win));
    dialog.add(&accounts_page(win));
    if let Some(name) = page {
        dialog.set_visible_page_name(name);
    }
    dialog.present(Some(&win.win));
}

fn general_page(win: &Rc<Window>) -> adw::PreferencesPage {
    let page = adw::PreferencesPage::builder()
        .title("General")
        .name("general")
        .icon_name("preferences-system-symbolic")
        .build();

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
        .title("Languages")
        .description(
            "Which translations an entry shows, and which comes first. An entry that has none of \
             them shows English, or whatever it has.",
        )
        .build();
    page.add(&glosses);
    let list = gtk::ListBox::builder()
        .selection_mode(gtk::SelectionMode::None)
        .css_classes(["boxed-list"])
        .build();
    glosses.add(&list);
    rebuild_languages(&list, win);
    page
}

/// The gloss languages JMdict and Wadoku carry, in the order the switches are listed.
const LANGUAGES: &[&str] = &["eng", "ger", "dut", "fre", "rus", "spa", "hun", "slv", "swe"];

/// A "Show first" choice over the enabled languages, then a switch per language.
fn rebuild_languages(list: &gtk::ListBox, win: &Rc<Window>) {
    list.remove_all();
    let chosen = win.config().borrow().gloss_languages.clone();
    let enabled: Vec<&str> = LANGUAGES
        .iter()
        .copied()
        .filter(|l| chosen.iter().any(|c| c == l))
        .collect();
    let (weak_list, weak_win) = (list.downgrade(), Rc::downgrade(win));
    let rebuild = move || {
        let (list, win) = (weak_list.clone(), weak_win.clone());
        move || {
            if let (Some(list), Some(win)) = (list.upgrade(), win.upgrade()) {
                rebuild_languages(&list, &win);
            }
        }
    };
    // Saves `langs` as the new list, re-renders the entry and rebuilds these rows.
    let apply = |win: &Rc<Window>, langs: Vec<String>, rebuild: &dyn Fn()| {
        {
            let mut cfg = win.config().borrow_mut();
            cfg.gloss_languages = langs;
            cfg.save();
        }
        win.rerender();
        win.refresh_search();
        rebuild();
    };
    if enabled.len() > 1 {
        let names: Vec<String> = enabled.iter().map(|l| language_name(l)).collect();
        let name_refs: Vec<&str> = names.iter().map(String::as_str).collect();
        let first = adw::ComboRow::builder()
            .title("Show first")
            .model(&gtk::StringList::new(&name_refs))
            .build();
        let current = chosen.first().and_then(|c| enabled.iter().position(|l| l == c));
        first.set_selected(current.unwrap_or(0) as u32);
        let enabled_owned: Vec<String> = enabled.iter().map(|l| l.to_string()).collect();
        first.connect_selected_notify(clone!(
            #[weak]
            win,
            #[strong]
            rebuild,
            move |row| {
                let Some(pick) = enabled_owned.get(row.selected() as usize) else {
                    return;
                };
                let mut langs = vec![pick.clone()];
                langs.extend(enabled_owned.iter().filter(|l| *l != pick).cloned());
                apply(&win, langs, &rebuild());
            }
        ));
        list.append(&first);
    }
    for lang in LANGUAGES {
        let row = adw::SwitchRow::builder()
            .title(language_name(lang))
            .active(chosen.iter().any(|c| c == lang))
            .build();
        row.connect_active_notify(clone!(
            #[weak]
            win,
            #[strong]
            rebuild,
            move |row| {
                let mut langs = win.config().borrow().gloss_languages.clone();
                langs.retain(|l| l != lang);
                if row.is_active() {
                    langs.push(lang.to_string());
                }
                apply(&win, langs, &rebuild());
            }
        ));
        list.append(&row);
    }
}

fn accounts_page(win: &Rc<Window>) -> adw::PreferencesPage {
    let page = adw::PreferencesPage::builder()
        .title("Accounts")
        .name("accounts")
        .icon_name("system-users-symbolic")
        .build();
    let group = adw::PreferencesGroup::builder()
        .title("WaniKani")
        .description(
            "Shows the level and SRS stage of words and kanji you learn on WaniKani, and adds the \
             #known, #unknown, #kanji-known and #wk-level-N filters. The token stays in the keyring; \
             a read-only personal access token from wanikani.com/settings/personal_access_tokens is \
             enough.",
        )
        .build();
    let list = gtk::ListBox::builder()
        .selection_mode(gtk::SelectionMode::None)
        .css_classes(["boxed-list"])
        .build();
    group.add(&list);
    page.add(&group);
    rebuild_wanikani(&list, win);
    // The rows follow the WaniKani job: buttons off and a status line while it runs. Only a
    // change of that job's state rebuilds them; rebuilding on every progress message of any job
    // (an import running in the background, say) would recreate the token entry under the
    // user's cursor and drop its focus.
    let weak_list = list.downgrade();
    let weak_win = Rc::downgrade(win);
    let last = std::cell::RefCell::new(win.jobs().state("wanikani"));
    win.jobs()
        .connect(move || match (weak_list.upgrade(), weak_win.upgrade()) {
            (Some(list), Some(win)) => {
                let now = win.jobs().state("wanikani");
                let changed = std::mem::discriminant(&now) != std::mem::discriminant(&*last.borrow());
                *last.borrow_mut() = now;
                if changed {
                    rebuild_wanikani(&list, &win);
                }
                true
            }
            _ => false,
        });
    page
}

/// The WaniKani rows: token entry and Connect when nobody is connected; otherwise who it is,
/// the sync state, Sync now, Disconnect, and the "show on entries" switch.
fn rebuild_wanikani(list: &gtk::ListBox, win: &Rc<Window>) {
    list.remove_all();
    let state = win.jobs().state("wanikani");
    let busy = state != State::Idle;
    let (weak_list, weak_win) = (list.downgrade(), Rc::downgrade(win));
    let rebuild = move || {
        let (list, win) = (weak_list.clone(), weak_win.clone());
        move || {
            if let (Some(list), Some(win)) = (list.upgrade(), win.upgrade()) {
                rebuild_wanikani(&list, &win);
            }
        }
    };
    let status_line = match &state {
        State::Idle => None,
        State::Queued => Some("Queued…".to_string()),
        State::Running { message, .. } => Some(message.clone()),
    };
    match win.wanikani_status() {
        Some(status) => {
            let synced = match &status.last_sync {
                Some(t) => format!("synced {}", t.get(..16).unwrap_or(t).replace('T', " ")),
                None => "not synced yet".to_string(),
            };
            let row = adw::ActionRow::builder()
                .title(format!("Connected as {}", status.username))
                .subtitle(status_line.clone().unwrap_or_else(|| {
                    format!(
                        "Level {} · {} items · {synced}",
                        status.level,
                        thousands(status.items as i64)
                    )
                }))
                .build();
            let sync = gtk::Button::builder()
                .label("Sync now")
                .valign(gtk::Align::Center)
                .sensitive(!busy)
                .build();
            sync.connect_clicked(clone!(
                #[weak]
                win,
                #[strong]
                rebuild,
                move |_| win.sync_wanikani(rebuild())
            ));
            row.add_suffix(&sync);
            let disconnect = gtk::Button::builder()
                .icon_name("user-trash-symbolic")
                .css_classes(["flat"])
                .tooltip_text("Disconnect and forget everything synced from WaniKani")
                .valign(gtk::Align::Center)
                .sensitive(!busy)
                .build();
            disconnect.connect_clicked(clone!(
                #[weak]
                win,
                #[strong]
                rebuild,
                move |_| win.disconnect_wanikani(rebuild())
            ));
            row.add_suffix(&disconnect);
            list.append(&row);
            let show = adw::SwitchRow::builder()
                .title("Show on entries")
                .subtitle("Level and stage chips on entries and kanji pages")
                .active(win.config().borrow().show_wanikani)
                .build();
            show.connect_active_notify(clone!(
                #[weak]
                win,
                move |row| {
                    {
                        let mut cfg = win.config().borrow_mut();
                        cfg.show_wanikani = row.is_active();
                        cfg.save();
                    }
                    win.rerender();
                }
            ));
            list.append(&show);
        }
        None => {
            let token = adw::PasswordEntryRow::builder()
                .title("API token")
                .sensitive(!busy)
                .build();
            let connect = gtk::Button::builder()
                .label("Connect")
                .valign(gtk::Align::Center)
                .sensitive(!busy)
                .css_classes(["suggested-action"])
                .build();
            connect.connect_clicked(clone!(
                #[weak]
                win,
                #[weak]
                token,
                #[strong]
                rebuild,
                move |_| {
                    let text = token.text().trim().to_string();
                    if !text.is_empty() {
                        win.connect_wanikani(text, rebuild());
                    }
                }
            ));
            token.add_suffix(&connect);
            list.append(&token);
            if let Some(line) = status_line {
                list.append(&adw::ActionRow::builder().title(line).build());
            }
        }
    }
}

fn dictionaries_page(win: &Rc<Window>) -> adw::PreferencesPage {
    let page = adw::PreferencesPage::builder()
        .title("Dictionaries")
        .name("dictionaries")
        .icon_name("accessories-dictionary-symbolic")
        .build();
    let group = adw::PreferencesGroup::builder()
        .title("Dictionaries")
        .description(
            "Search looks in the enabled dictionaries, in this order. Downloads go to the cache \
             directory and are imported into the local database.",
        )
        .build();
    let list = gtk::ListBox::builder()
        .selection_mode(gtk::SelectionMode::None)
        .css_classes(["boxed-list"])
        .build();
    group.add(&list);
    page.add(&group);
    rebuild_sources(&list, win);
    page
}

/// Fills the list with one row per known source. Every change rebuilds it, which is simpler than
/// keeping rows in sync and cheap for a handful of dictionaries.
fn rebuild_sources(list: &gtk::ListBox, win: &Rc<Window>) {
    list.remove_all();
    let settings = win.config().borrow().source_settings();
    let installed = win.db().sources().unwrap_or_else(|e| {
        log::error!("cannot list the sources: {e:#}");
        Vec::new()
    });
    for (index, setting) in settings.iter().enumerate() {
        let Some(source) = sources::by_id(&setting.id) else {
            continue;
        };
        let status = installed.iter().find(|s| s.id == setting.id);
        let can_move_up = index > 0;
        list.append(&source_row(
            list,
            win,
            source,
            setting.enabled,
            status,
            can_move_up,
        ));
    }
}

fn source_row(
    list: &gtk::ListBox,
    win: &Rc<Window>,
    source: &'static Source,
    enabled: bool,
    status: Option<&SourceStatus>,
    can_move_up: bool,
) -> adw::ActionRow {
    let subtitle = match status {
        Some(s) => format!(
            "{}\n{} entries, version {}\nImported {}",
            source.description,
            thousands(s.entries),
            s.version.as_deref().unwrap_or("unknown"),
            s.imported.get(..10).unwrap_or(&s.imported)
        ),
        None => format!(
            "{}\nNot installed. The download is about {} MB.",
            source.description, source.size_mb
        ),
    };
    let row = adw::ActionRow::builder()
        .title(source.name)
        .subtitle(&subtitle)
        .tooltip_text(format!("{}\n{}", source.licence, source.licence_url))
        .build();
    // The buttons go insensitive and a spinner appears while a job for this source is queued or
    // running; the subtitle carries the job's progress line.
    let buttons: Rc<std::cell::RefCell<Vec<gtk::Widget>>> = Default::default();
    let spinner = gtk::Spinner::builder()
        .visible(false)
        .valign(gtk::Align::Center)
        .build();
    row.add_suffix(&spinner);
    // Makes a "rebuild this list" callback for the jobs to run when they are done. Weak
    // references, so a job finishing after the dialog closed does nothing.
    let (weak_list, weak_win) = (list.downgrade(), Rc::downgrade(win));
    let rebuild = move || {
        let (list, win) = (weak_list.clone(), weak_win.clone());
        move || {
            if let (Some(list), Some(win)) = (list.upgrade(), win.upgrade()) {
                rebuild_sources(&list, &win);
            }
        }
    };

    let track = |button: &gtk::Button| buttons.borrow_mut().push(button.clone().upcast());
    if can_move_up {
        let up = gtk::Button::builder()
            .icon_name("go-up-symbolic")
            .valign(gtk::Align::Center)
            .css_classes(["flat"])
            .tooltip_text("Search this dictionary earlier")
            .build();
        up.connect_clicked(clone!(
            #[weak]
            list,
            #[weak]
            win,
            move |_| {
                {
                    let mut cfg = win.config().borrow_mut();
                    cfg.move_source_up(source.id);
                    cfg.save();
                }
                win.refresh_search();
                rebuild_sources(&list, &win);
            }
        ));
        track(&up);
        row.add_suffix(&up);
    }

    let switch = gtk::Switch::builder()
        .active(enabled)
        .sensitive(status.is_some())
        .valign(gtk::Align::Center)
        .tooltip_text("Include in search")
        .build();
    switch.connect_active_notify(clone!(
        #[weak]
        win,
        move |switch| {
            {
                let mut cfg = win.config().borrow_mut();
                cfg.set_source_enabled(source.id, switch.is_active());
                cfg.save();
            }
            win.refresh_search();
        }
    ));
    row.add_suffix(&switch);

    if status.is_some() {
        let update = gtk::Button::builder()
            .label("Update")
            .valign(gtk::Align::Center)
            .tooltip_text(format!("Download today's {} and import it again", source.name))
            .build();
        update.connect_clicked(clone!(
            #[weak]
            win,
            #[strong]
            rebuild,
            move |_| win.download_source(source, rebuild())
        ));
        track(&update);
        row.add_suffix(&update);
        let remove = gtk::Button::builder()
            .icon_name("user-trash-symbolic")
            .valign(gtk::Align::Center)
            .css_classes(["flat"])
            .tooltip_text(format!("Remove {}", source.name))
            .build();
        remove.connect_clicked(clone!(
            #[weak]
            list,
            #[weak]
            win,
            move |_| confirm_remove(&list, &win, source)
        ));
        track(&remove);
        row.add_suffix(&remove);
    } else {
        // One text button, the rest icons, so the row keeps room for its title. With a cached
        // download the text button imports that; downloading again moves to an icon.
        let cached = cache_dir().join(source.filename);
        let has_cache = cached.exists();
        if has_cache {
            let import = gtk::Button::builder()
                .label("Import downloaded copy")
                .valign(gtk::Align::Center)
                .tooltip_text(format!(
                    "Import {} without downloading it again",
                    cached.display()
                ))
                .build();
            import.connect_clicked(clone!(
                #[weak]
                win,
                #[strong]
                rebuild,
                move |_| win.import_source_file(source, cached.clone(), rebuild())
            ));
            track(&import);
            row.add_suffix(&import);
        }
        let download = if has_cache {
            gtk::Button::builder()
                .icon_name("folder-download-symbolic")
                .css_classes(["flat"])
                .tooltip_text(format!("Download today's {} instead", source.name))
        } else {
            gtk::Button::builder().label("Download")
        }
        .valign(gtk::Align::Center)
        .build();
        download.connect_clicked(clone!(
            #[weak]
            win,
            #[strong]
            rebuild,
            move |_| win.download_source(source, rebuild())
        ));
        track(&download);
        row.add_suffix(&download);
        let file = gtk::Button::builder()
            .icon_name("document-open-symbolic")
            .css_classes(["flat"])
            .tooltip_text(format!("Import a {} file from disk", source.name))
            .valign(gtk::Align::Center)
            .build();
        file.connect_clicked(clone!(
            #[weak]
            win,
            #[strong]
            rebuild,
            move |_| win.choose_import_file(source, rebuild())
        ));
        track(&file);
        row.add_suffix(&file);
    }

    let jobs = win.jobs().clone();
    let apply = Rc::new(clone!(
        #[weak]
        row,
        #[weak]
        spinner,
        move || {
            let state = jobs.state(source.id);
            let (line, active) = match &state {
                State::Idle => (None, false),
                State::Queued => (Some("Queued…".to_string()), true),
                State::Running { message, fraction } => (
                    Some(match fraction {
                        Some(f) => format!("{message} ({:.0} %)", f * 100.0),
                        None => message.clone(),
                    }),
                    true,
                ),
            };
            // While a job runs the entry count is stale (the import registers the source first),
            // so the row shows the description and the progress line only.
            row.set_subtitle(&match line {
                Some(line) => format!("{}\n{line}", source.description),
                None => subtitle.clone(),
            });
            spinner.set_visible(active);
            spinner.set_spinning(active);
            for button in buttons.borrow().iter() {
                button.set_sensitive(!active);
            }
        }
    ));
    apply();
    let weak_row = row.downgrade();
    win.jobs().connect(move || {
        if weak_row.upgrade().is_none() {
            return false;
        }
        apply();
        true
    });
    row
}

fn confirm_remove(list: &gtk::ListBox, win: &Rc<Window>, source: &'static Source) {
    let dialog = adw::AlertDialog::builder()
        .heading(format!("Remove {}?", source.name))
        .body("Its entries leave the database and the downloaded file is deleted. It can be downloaded again any time.")
        .build();
    dialog.add_responses(&[("cancel", "Cancel"), ("remove", "Remove")]);
    dialog.set_response_appearance("remove", adw::ResponseAppearance::Destructive);
    dialog.set_default_response(Some("cancel"));
    dialog.set_close_response("cancel");
    dialog.connect_response(
        Some("remove"),
        clone!(
            #[weak]
            list,
            #[weak]
            win,
            move |_, _| {
                win.remove_source(
                    source,
                    clone!(
                        #[weak]
                        list,
                        #[weak]
                        win,
                        move || rebuild_sources(&list, &win)
                    ),
                )
            }
        ),
    );
    dialog.present(Some(list));
}
