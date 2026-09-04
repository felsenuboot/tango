//! Word lists in the sidebar: the lists, one list's entries, and the actions behind them.
//!
//! The page is built before the window exists and linked to it with `attach`, so its callbacks
//! hold a weak reference to the window like every other callback here.

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use adw::prelude::*;
use gtk::gio;
use gtk::glib::{self, clone};

use super::window::Window;
use crate::store::csv;
use crate::store::export::{self, Layout};
use crate::store::user::{Backup, List, ListEntry};

pub struct ListsPage {
    pub widget: gtk::Stack,
    lists_box: gtk::ListBox,
    entries_box: gtk::ListBox,
    title: gtk::Label,
    win: RefCell<Weak<Window>>,
    /// The list whose entries are shown, if the entries page is up.
    current: Cell<Option<i64>>,
    lists: RefCell<Vec<List>>,
    entries: RefCell<Vec<ListEntry>>,
}

impl ListsPage {
    pub fn new() -> Rc<Self> {
        let lists_box = gtk::ListBox::builder()
            .selection_mode(gtk::SelectionMode::None)
            .css_classes(["navigation-sidebar"])
            .build();
        lists_box.set_placeholder(Some(
            &adw::StatusPage::builder()
                .title("No lists")
                .icon_name("view-list-symbolic")
                .build(),
        ));
        let new_list = gtk::Button::builder()
            .label("New list…")
            .action_name("lists.new")
            .build();
        let more = gio::Menu::new();
        more.append(Some("Export all lists…"), Some("lists.export-backup"));
        more.append(Some("Import a backup…"), Some("lists.import-backup"));
        let more_button = gtk::MenuButton::builder()
            .icon_name("view-more-symbolic")
            .menu_model(&more)
            .tooltip_text("Backup")
            .build();
        let bar = gtk::ActionBar::new();
        bar.pack_start(&new_list);
        bar.pack_end(&more_button);
        let lists_page = gtk::Box::new(gtk::Orientation::Vertical, 0);
        lists_page.append(
            &gtk::ScrolledWindow::builder()
                .child(&lists_box)
                .vexpand(true)
                .hscrollbar_policy(gtk::PolicyType::Never)
                .build(),
        );
        lists_page.append(&bar);

        let entries_box = gtk::ListBox::builder()
            .selection_mode(gtk::SelectionMode::None)
            .css_classes(["navigation-sidebar"])
            .build();
        entries_box.set_placeholder(Some(
            &adw::StatusPage::builder()
                .title("Empty list")
                .description("Star an entry or use \"Add to list\" above it.")
                .icon_name("non-starred-symbolic")
                .build(),
        ));
        let back = gtk::Button::builder()
            .icon_name("go-previous-symbolic")
            .css_classes(["flat"])
            .action_name("lists.back")
            .tooltip_text("All lists")
            .build();
        let title = gtk::Label::builder()
            .hexpand(true)
            .xalign(0.0)
            .ellipsize(gtk::pango::EllipsizeMode::End)
            .css_classes(["heading"])
            .build();
        let menu = gio::Menu::new();
        menu.append(Some("Rename…"), Some("lists.rename"));
        menu.append(Some("Move up"), Some("lists.move-up"));
        let export = gio::Menu::new();
        export.append(Some(Layout::Csv.label()), Some("lists.export::csv"));
        export.append(Some(Layout::Takoboto.label()), Some("lists.export::takoboto"));
        menu.append_submenu(Some("Export as…"), &export);
        menu.append(Some("Import CSV into this list…"), Some("lists.import-csv"));
        menu.append(Some("Delete list"), Some("lists.delete"));
        let menu_button = gtk::MenuButton::builder()
            .icon_name("view-more-symbolic")
            .menu_model(&menu)
            .css_classes(["flat"])
            .build();
        let header = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .spacing(6)
            .margin_start(6)
            .margin_end(6)
            .margin_top(6)
            .margin_bottom(6)
            .build();
        header.append(&back);
        header.append(&title);
        header.append(&menu_button);
        let entries_page = gtk::Box::new(gtk::Orientation::Vertical, 0);
        entries_page.append(&header);
        entries_page.append(
            &gtk::ScrolledWindow::builder()
                .child(&entries_box)
                .vexpand(true)
                .hscrollbar_policy(gtk::PolicyType::Never)
                .build(),
        );

        let widget = gtk::Stack::builder()
            .transition_type(gtk::StackTransitionType::SlideLeftRight)
            .build();
        widget.add_named(&lists_page, Some("lists"));
        widget.add_named(&entries_page, Some("entries"));

        let this = Rc::new(Self {
            widget,
            lists_box,
            entries_box,
            title,
            win: RefCell::new(Weak::new()),
            current: Cell::new(None),
            lists: RefCell::new(Vec::new()),
            entries: RefCell::new(Vec::new()),
        });
        this.install_actions();
        this.lists_box.connect_row_activated(clone!(
            #[weak]
            this,
            move |_, row| {
                let id = this.lists.borrow().get(row.index() as usize).map(|l| l.id);
                if let Some(id) = id {
                    this.open_list(id);
                }
            }
        ));
        this.entries_box.connect_row_activated(clone!(
            #[weak]
            this,
            move |_, row| {
                let entry = this.entries.borrow().get(row.index() as usize).cloned();
                if let (Some(entry), Some(win)) = (entry, this.window()) {
                    win.open_list_entry(&entry);
                }
            }
        ));
        this
    }

    /// Links the page to its window; the window must call this once it exists.
    pub fn attach(&self, win: &Rc<Window>) {
        *self.win.borrow_mut() = Rc::downgrade(win);
        self.refresh();
    }

    fn window(&self) -> Option<Rc<Window>> {
        self.win.borrow().upgrade()
    }

    /// Reloads the lists, and the open list's entries.
    pub fn refresh(&self) {
        let Some(win) = self.window() else { return };
        let lists = win.user().lists().unwrap_or_else(|e| {
            log::error!("cannot read the lists: {e:#}");
            Vec::new()
        });
        self.lists_box.remove_all();
        for list in &lists {
            let row = adw::ActionRow::builder()
                .activatable(true)
                .title(glib::markup_escape_text(&list.name).as_str())
                .subtitle(format!(
                    "{} {}",
                    list.entries,
                    if list.entries == 1 { "entry" } else { "entries" }
                ))
                .build();
            row.add_suffix(&gtk::Image::from_icon_name("go-next-symbolic"));
            self.lists_box.append(&row);
        }
        *self.lists.borrow_mut() = lists;
        if let Some(id) = self.current.get() {
            if self.lists.borrow().iter().any(|l| l.id == id) {
                self.show_entries(id);
            } else {
                self.back();
            }
        }
    }

    pub fn open_list(&self, id: i64) {
        self.current.set(Some(id));
        self.show_entries(id);
        self.widget.set_visible_child_name("entries");
    }

    fn back(&self) {
        self.current.set(None);
        self.widget.set_visible_child_name("lists");
    }

    fn show_entries(&self, id: i64) {
        let Some(win) = self.window() else { return };
        let name = self
            .lists
            .borrow()
            .iter()
            .find(|l| l.id == id)
            .map(|l| l.name.clone())
            .unwrap_or_default();
        self.title.set_text(&name);
        let entries = win.user().entries(id).unwrap_or_else(|e| {
            log::error!("cannot read list {id}: {e:#}");
            Vec::new()
        });
        self.entries_box.remove_all();
        for e in &entries {
            let mut title = glib::markup_escape_text(&e.headword).to_string();
            if !e.reading.is_empty() && e.reading != e.headword {
                title.push_str(&format!(
                    "  <span alpha='70%'>{}</span>",
                    glib::markup_escape_text(&e.reading)
                ));
            }
            let mut subtitle = glib::markup_escape_text(&e.gloss).to_string();
            if !e.note.is_empty() {
                subtitle.push_str(&format!(" · <i>{}</i>", glib::markup_escape_text(&e.note)));
            }
            let row = adw::ActionRow::builder()
                .activatable(true)
                .use_markup(true)
                .title(&title)
                .subtitle(&subtitle)
                .title_lines(1)
                .subtitle_lines(1)
                .build();
            let remove = gtk::Button::builder()
                .icon_name("list-remove-symbolic")
                .valign(gtk::Align::Center)
                .css_classes(["flat"])
                .tooltip_text("Remove from this list")
                .build();
            let (source, seq) = (e.source.clone(), e.seq);
            remove.connect_clicked(clone!(
                #[weak]
                win,
                move |_| {
                    if let Err(e) = win.user().remove(id, &source, seq) {
                        log::error!("cannot remove from list {id}: {e:#}");
                    }
                    win.lists_changed();
                }
            ));
            row.add_suffix(&remove);
            self.entries_box.append(&row);
        }
        *self.entries.borrow_mut() = entries;
    }

    fn install_actions(self: &Rc<Self>) {
        type Action = Box<dyn Fn(&Rc<ListsPage>)>;
        let group = gio::SimpleActionGroup::new();
        let add = |name: &str, f: Action| {
            let action = gio::SimpleAction::new(name, None);
            let this = Rc::downgrade(self);
            action.connect_activate(move |_, _| {
                if let Some(this) = this.upgrade() {
                    f(&this);
                }
            });
            group.add_action(&action);
        };
        add("back", Box::new(|this| this.back()));
        add("new", Box::new(|this| this.new_list()));
        add("rename", Box::new(|this| this.rename()));
        add("move-up", Box::new(|this| this.move_up()));
        add("delete", Box::new(|this| this.delete()));
        let export = gio::SimpleAction::new("export", Some(&String::static_variant_type()));
        let weak = Rc::downgrade(self);
        export.connect_activate(move |_, target| {
            let layout = match target.and_then(|t| t.get::<String>()).as_deref() {
                Some("takoboto") => Layout::Takoboto,
                _ => Layout::Csv,
            };
            if let Some(this) = weak.upgrade() {
                this.export(layout);
            }
        });
        group.add_action(&export);
        add("import-csv", Box::new(|this| this.import_csv()));
        add("export-backup", Box::new(|this| this.export_backup()));
        add("import-backup", Box::new(|this| this.import_backup()));
        self.widget.insert_action_group("lists", Some(&group));
    }

    // -- list actions -----------------------------------------------------------------------

    fn new_list(self: &Rc<Self>) {
        let Some(win) = self.window() else { return };
        ask_name(&win, "New list", "", "Create", {
            let this = self.clone();
            let win = win.clone();
            move |name| match win.user().create_list(&name) {
                Ok(list) => {
                    win.lists_changed();
                    this.open_list(list.id);
                }
                Err(e) => win.toast(&format!("Cannot create the list: {e}")),
            }
        });
    }

    fn rename(self: &Rc<Self>) {
        let (Some(win), Some(id)) = (self.window(), self.current.get()) else {
            return;
        };
        let current = self.title.text().to_string();
        let target = win.clone();
        ask_name(&win, "Rename list", &current, "Rename", move |name| match target
            .user()
            .rename_list(id, &name)
        {
            Ok(()) => target.lists_changed(),
            Err(e) => target.toast(&format!("Cannot rename the list: {e}")),
        });
    }

    fn move_up(&self) {
        let (Some(win), Some(id)) = (self.window(), self.current.get()) else {
            return;
        };
        if let Err(e) = win.user().move_list_up(id) {
            log::error!("cannot move list {id}: {e:#}");
        }
        win.lists_changed();
    }

    fn delete(&self) {
        let (Some(win), Some(id)) = (self.window(), self.current.get()) else {
            return;
        };
        let name = self.title.text().to_string();
        let dialog = adw::AlertDialog::builder()
            .heading(format!("Delete \"{name}\"?"))
            .body("The list and its entries are removed. The dictionary entries themselves stay.")
            .build();
        dialog.add_responses(&[("cancel", "Cancel"), ("delete", "Delete")]);
        dialog.set_response_appearance("delete", adw::ResponseAppearance::Destructive);
        dialog.set_default_response(Some("cancel"));
        dialog.set_close_response("cancel");
        dialog.connect_response(
            Some("delete"),
            clone!(
                #[weak]
                win,
                move |_, _| {
                    if let Err(e) = win.user().delete_list(id) {
                        win.toast(&format!("Cannot delete the list: {e}"));
                    }
                    win.lists_changed();
                }
            ),
        );
        dialog.present(Some(&win.win));
    }

    // -- files ------------------------------------------------------------------------------

    /// Writes the open list in `layout`; the dictionary entries are looked up for the layouts
    /// that want more than the one gloss a list keeps.
    fn export(&self, layout: Layout) {
        let (Some(win), Some(id)) = (self.window(), self.current.get()) else {
            return;
        };
        let name = self.title.text().to_string();
        let items = match win.user().entries(id) {
            Ok(e) => e,
            Err(e) => return win.toast(&format!("Cannot read the list: {e}")),
        };
        let entries: Vec<_> = items
            .iter()
            .map(|i| win.db().get(&i.source, i.seq).ok().flatten())
            .collect();
        let rows: Vec<export::Row> = items
            .iter()
            .zip(&entries)
            .map(|(item, entry)| export::Row {
                list: &name,
                item,
                entry: entry.as_ref(),
            })
            .collect();
        save_text(&win, &layout.file_name(&name), export::render(layout, &rows));
    }

    /// A plain CSV goes into the open list; a Takoboto export goes into the lists it names.
    fn import_csv(&self) {
        let (Some(win), Some(id)) = (self.window(), self.current.get()) else {
            return;
        };
        open_text(&win, "CSV", &["*.csv", "*.tsv", "*.txt"], move |win, text| {
            let rows = csv::parse(&text);
            match export::parse_takoboto(&rows) {
                Some(takoboto) => {
                    let (added, total) = win.add_takoboto_rows(&takoboto);
                    win.toast(&format!(
                        "Takoboto export: added {added} of {total} rows to their lists"
                    ));
                }
                None => {
                    let (added, total) = win.add_rows_to_list(id, &rows);
                    win.toast(&format!("Added {added} of {total} rows"));
                }
            }
            win.lists_changed();
        });
    }

    fn export_backup(&self) {
        let Some(win) = self.window() else { return };
        match win
            .user()
            .backup()
            .and_then(|b| Ok(serde_json::to_string_pretty(&b)?))
        {
            Ok(json) => save_text(&win, "tango-lists.json", json),
            Err(e) => win.toast(&format!("Cannot export: {e}")),
        }
    }

    fn import_backup(&self) {
        let Some(win) = self.window() else { return };
        open_text(&win, "Tango backup", &["*.json"], move |win, text| {
            match serde_json::from_str::<Backup>(&text)
                .map_err(anyhow::Error::from)
                .and_then(|b| win.user().restore(&b))
            {
                Ok(n) => win.toast(&format!("Added {n} entries from the backup")),
                Err(e) => win.toast(&format!("Cannot import the backup: {e}")),
            }
            win.lists_changed();
        });
    }
}

/// A small dialog asking for a list name; `on_ok` gets the trimmed, non-empty text.
pub(super) fn ask_name(
    win: &Rc<Window>,
    heading: &str,
    initial: &str,
    ok_label: &str,
    on_ok: impl Fn(String) + 'static,
) {
    let entry = gtk::Entry::builder()
        .text(initial)
        .activates_default(true)
        .placeholder_text("Name")
        .build();
    let dialog = adw::AlertDialog::builder()
        .heading(heading)
        .extra_child(&entry)
        .build();
    dialog.add_responses(&[("cancel", "Cancel"), ("ok", ok_label)]);
    dialog.set_response_appearance("ok", adw::ResponseAppearance::Suggested);
    dialog.set_default_response(Some("ok"));
    dialog.set_close_response("cancel");
    dialog.connect_response(
        Some("ok"),
        clone!(
            #[weak]
            entry,
            move |_, _| {
                let name = entry.text().trim().to_string();
                if !name.is_empty() {
                    on_ok(name);
                }
            }
        ),
    );
    dialog.present(Some(&win.win));
    entry.grab_focus();
}

fn save_text(win: &Rc<Window>, initial_name: &str, text: String) {
    let dialog = gtk::FileDialog::builder()
        .title("Export")
        .initial_name(initial_name)
        .build();
    let weak = Rc::downgrade(win);
    dialog.save(Some(&win.win), gio::Cancellable::NONE, move |result| {
        let Some(win) = weak.upgrade() else { return };
        if let Ok(file) = result
            && let Some(path) = file.path()
        {
            match std::fs::write(&path, text) {
                Ok(()) => win.toast(&format!("Exported to {}", path.display())),
                Err(e) => win.toast(&format!("Cannot write {}: {e}", path.display())),
            }
        }
    });
}

fn open_text(
    win: &Rc<Window>,
    kind: &str,
    patterns: &[&str],
    on_text: impl Fn(&Rc<Window>, String) + 'static,
) {
    let filter = gtk::FileFilter::new();
    filter.set_name(Some(kind));
    for p in patterns {
        filter.add_pattern(p);
    }
    let filters = gio::ListStore::new::<gtk::FileFilter>();
    filters.append(&filter);
    let dialog = gtk::FileDialog::builder()
        .title(format!("Import {kind}"))
        .filters(&filters)
        .build();
    let weak = Rc::downgrade(win);
    dialog.open(Some(&win.win), gio::Cancellable::NONE, move |result| {
        let Some(win) = weak.upgrade() else { return };
        if let Ok(file) = result
            && let Some(path) = file.path()
        {
            match std::fs::read_to_string(&path) {
                Ok(text) => on_text(&win, text),
                Err(e) => win.toast(&format!("Cannot read {}: {e}", path.display())),
            }
        }
    });
}
