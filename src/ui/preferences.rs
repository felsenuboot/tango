//! The preferences dialog: a General page (appearance, glosses) and a Dictionaries page that lists
//! every source Tango knows, installed or not, with update, remove and search toggles.

use std::rc::Rc;

use adw::prelude::*;
use gtk::glib::{self, clone};

use super::theme::{self, Scheme};
use super::thousands;
use super::window::Window;
use crate::dict::sources::{self, Source};
use crate::store::db::SourceStatus;

/// Opens the dialog, on the page named `page` ("general", "dictionaries") if given.
pub fn show(win: &Rc<Window>, page: Option<&str>) {
    let dialog = adw::PreferencesDialog::builder().title("Preferences").build();
    dialog.add(&general_page(win));
    dialog.add(&dictionaries_page(win));
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
    page
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
        row.add_suffix(&remove);
    } else {
        let download = gtk::Button::builder()
            .label("Download")
            .valign(gtk::Align::Center)
            .build();
        download.connect_clicked(clone!(
            #[weak]
            win,
            #[strong]
            rebuild,
            move |_| win.download_source(source, rebuild())
        ));
        row.add_suffix(&download);
        let file = gtk::Button::builder()
            .label("Import file…")
            .valign(gtk::Align::Center)
            .build();
        file.connect_clicked(clone!(
            #[weak]
            win,
            #[strong]
            rebuild,
            move |_| win.choose_import_file(source, rebuild())
        ));
        row.add_suffix(&file);
    }
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
