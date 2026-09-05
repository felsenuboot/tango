//! Word lists in the sidebar: the lists, one list's entries, and the actions behind them.
//!
//! The page is built before the window exists and linked to it with `attach`, so its callbacks
//! hold a weak reference to the window like every other callback here.

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use adw::prelude::*;
use gtk::gio;
use gtk::glib::{self, clone};

use std::collections::{HashMap, HashSet};

use super::window::Window;
use crate::accounts::{Kind, Learned};
use crate::model::Entry;
use crate::store::csv;
use crate::store::export::{self, Layout};
use crate::store::user::{Backup, List, ListEntry};

/// The id of the built-in WaniKani list: not a row in `lists`, built from the synced items.
pub const WANIKANI_LIST: i64 = -1;

pub struct ListsPage {
    pub widget: gtk::Stack,
    lists_box: gtk::ListBox,
    entries_box: gtk::ListBox,
    title: gtk::Label,
    /// Kind, level and stage filters; shown for the WaniKani list only.
    filters: gtk::Box,
    kind: gtk::DropDown,
    level: gtk::DropDown,
    stage: gtk::DropDown,
    win: RefCell<Weak<Window>>,
    /// The list whose entries are shown, if the entries page is up.
    current: Cell<Option<i64>>,
    lists: RefCell<Vec<List>>,
    entries: RefCell<Vec<ListEntry>>,
    /// The WaniKani list after the filters, and how many of its items have rows so far; the
    /// rest come in pages of `PAGE` through the "Show more" row (issue #73).
    wk_items: RefCell<Vec<Learned>>,
    wk_shown: Cell<usize>,
    more_row: RefCell<Option<gtk::ListBoxRow>>,
    /// What WaniKani knows about each of `entries`, parallel to it (`None` on plain lists).
    learned: RefCell<Vec<Option<Learned>>>,
    /// Rows or tiles (#83): the "rows" and "tiles" pages, and the header button that switches.
    view: gtk::Stack,
    tiles_box: adw::Bin,
    tiles_toggle: gtk::ToggleButton,
}

/// Rows the WaniKani list builds at a time.
pub const PAGE: usize = 200;

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
        for (layout, target) in [
            (Layout::Csv, "csv"),
            (Layout::Anki, "anki"),
            (Layout::Kitsun, "kitsun"),
            (Layout::Takoboto, "takoboto"),
        ] {
            export.append(Some(layout.label()), Some(&format!("lists.export::{target}")));
        }
        menu.append_submenu(Some("Export as…"), &export);
        menu.append(Some("Import CSV into this list…"), Some("lists.import-csv"));
        menu.append(Some("Delete list"), Some("lists.delete"));
        let menu_button = gtk::MenuButton::builder()
            .icon_name("view-more-symbolic")
            .menu_model(&menu)
            .css_classes(["flat"])
            .tooltip_text("Rename, export, import or delete this list")
            .build();
        let header = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .spacing(6)
            .margin_start(6)
            .margin_end(6)
            .margin_top(6)
            .margin_bottom(6)
            .build();
        let tiles_toggle = gtk::ToggleButton::builder()
            .icon_name("view-grid-symbolic")
            .css_classes(["flat"])
            .tooltip_text("Tiles instead of rows")
            .build();
        header.append(&back);
        header.append(&title);
        header.append(&tiles_toggle);
        header.append(&menu_button);
        // Filters for the WaniKani list (#55): what kind of item, which level, which stage.
        let kind = gtk::DropDown::from_strings(&["Words and kanji", "Words", "Kanji"]);
        // Rows 1..=60 are single levels, 61.. are "up to" a level (#82); typing in the
        // popover's search field narrows the 120 rows.
        let mut levels = vec!["Any level".to_string()];
        levels.extend((1..=60).map(|n| format!("Level {n}")));
        levels.extend((2..=60).map(|n| format!("Up to level {n}")));
        let level_refs: Vec<&str> = levels.iter().map(String::as_str).collect();
        let level = gtk::DropDown::from_strings(&level_refs);
        level.set_expression(Some(&gtk::PropertyExpression::new(
            gtk::StringObject::static_type(),
            gtk::Expression::NONE,
            "string",
        )));
        level.set_enable_search(true);
        let stage = gtk::DropDown::from_strings(&[
            "Any stage",
            "Unlocked",
            "Apprentice",
            "Guru",
            "Master",
            "Enlightened",
            "Burned",
            "Locked",
        ]);
        let filters = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .spacing(6)
            .margin_start(12)
            .margin_end(12)
            .margin_bottom(6)
            .visible(false)
            .build();
        for d in [&kind, &level, &stage] {
            d.set_hexpand(true);
            filters.append(d);
        }
        let entries_page = gtk::Box::new(gtk::Orientation::Vertical, 0);
        entries_page.append(&header);
        entries_page.append(&filters);
        let tiles_box = adw::Bin::new();
        let view = gtk::Stack::new();
        view.add_named(&entries_box, Some("rows"));
        view.add_named(&tiles_box, Some("tiles"));
        entries_page.append(
            &gtk::ScrolledWindow::builder()
                .child(&view)
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
            filters,
            kind,
            level,
            stage,
            win: RefCell::new(Weak::new()),
            current: Cell::new(None),
            lists: RefCell::new(Vec::new()),
            entries: RefCell::new(Vec::new()),
            wk_items: RefCell::new(Vec::new()),
            wk_shown: Cell::new(0),
            more_row: RefCell::new(None),
            learned: RefCell::new(Vec::new()),
            view,
            tiles_box,
            tiles_toggle,
        });
        this.install_actions();
        this.tiles_toggle.connect_toggled(clone!(
            #[weak]
            this,
            move |button| {
                let Some(win) = this.window() else { return };
                // `refresh_view` sets the button from the config too; only a real change acts.
                if win.config().borrow().list_tiles == button.is_active() {
                    return;
                }
                {
                    let mut cfg = win.config().borrow_mut();
                    cfg.list_tiles = button.is_active();
                    cfg.save();
                }
                this.refresh_view(win.cards_visible());
            }
        ));
        for d in [&this.kind, &this.level, &this.stage] {
            d.connect_selected_notify(clone!(
                #[weak]
                this,
                move |_| {
                    if this.current.get() == Some(WANIKANI_LIST) {
                        this.show_entries(WANIKANI_LIST, true);
                    }
                }
            ));
        }
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
                let index = row.index() as usize;
                let entry = this.entries.borrow().get(index).cloned();
                match (entry, this.window()) {
                    (Some(entry), Some(win)) => win.open_list_entry(&entry),
                    // Past the entries sits the "Show more" row.
                    (None, Some(win)) if this.current.get() == Some(WANIKANI_LIST) => {
                        this.append_page(&win, win.cards_visible())
                    }
                    _ => {}
                }
            }
        ));
        this
    }

    /// Links the page to its window; the window must call this once it exists.
    pub fn attach(&self, win: &Rc<Window>) {
        *self.win.borrow_mut() = Rc::downgrade(win);
        self.tiles_toggle.set_active(win.config().borrow().list_tiles);
        self.refresh();
    }

    /// After the grid preferences changed: the toggle, and the open list shown again.
    pub fn refresh_view(&self, cards: bool) {
        let Some(win) = self.window() else { return };
        let tiles = win.config().borrow().list_tiles;
        self.tiles_toggle.set_active(tiles);
        match self.current.get() {
            Some(id) => self.show_entries(id, cards),
            None => self
                .view
                .set_visible_child_name(if tiles { "tiles" } else { "rows" }),
        }
    }

    fn window(&self) -> Option<Rc<Window>> {
        self.win.borrow().upgrade()
    }

    /// Reloads the lists, and the open list's entries.
    pub fn refresh(&self) {
        let Some(win) = self.window() else { return };
        let mut lists = win.user().lists().unwrap_or_else(|e| {
            log::error!("cannot read the lists: {e:#}");
            Vec::new()
        });
        // The WaniKani list exists while an account has synced something.
        let wanikani = win.user().learned_count("wanikani").unwrap_or(0);
        if wanikani > 0 {
            lists.push(List {
                id: WANIKANI_LIST,
                name: "WaniKani".into(),
                entries: wanikani as i64,
            });
        }
        super::clear_rows(&self.lists_box);
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
                let cards = self.window().is_some_and(|w| w.cards_visible());
                self.show_entries(id, cards);
            } else {
                self.back();
            }
        }
    }

    pub fn open_list(&self, id: i64) {
        self.current.set(Some(id));
        self.show_entries(id, true);
        self.widget.set_visible_child_name("entries");
    }

    fn back(&self) {
        self.current.set(None);
        self.widget.set_visible_child_name("lists");
    }

    /// Fills the entries page; `cards` also shows the list in the content pane when that is
    /// enabled (an open list refreshing after a change keeps whatever the pane shows).
    fn show_entries(&self, id: i64, cards: bool) {
        let Some(win) = self.window() else { return };
        let name = self
            .lists
            .borrow()
            .iter()
            .find(|l| l.id == id)
            .map(|l| l.name.clone())
            .unwrap_or_default();
        self.title.set_text(&name);
        self.filters.set_visible(id == WANIKANI_LIST);
        super::clear_rows(&self.entries_box);
        *self.more_row.borrow_mut() = None;
        self.entries.borrow_mut().clear();
        self.learned.borrow_mut().clear();
        if id == WANIKANI_LIST {
            *self.wk_items.borrow_mut() = self.filtered_wanikani(&win);
            self.wk_shown.set(0);
            self.append_page(&win, cards);
            return;
        }
        let entries = win.user().entries(id).unwrap_or_else(|e| {
            log::error!("cannot read list {id}: {e:#}");
            Vec::new()
        });
        for e in &entries {
            self.entries_box.append(&self.row(&win, id, e, None));
        }
        let n = entries.len();
        *self.entries.borrow_mut() = entries;
        *self.learned.borrow_mut() = (0..n).map(|_| None).collect();
        super::name_icon_buttons(self.entries_box.upcast_ref());
        self.after_rows(&win, cards);
    }

    /// After rows were added (#83): the tiles page when tiles are on, and the list as cards
    /// in the content pane when `cards` asks for it and the preference allows.
    fn after_rows(&self, win: &Rc<Window>, cards: bool) {
        let (tiles_on, cards_on) = {
            let cfg = win.config().borrow();
            (cfg.list_tiles, cfg.list_cards)
        };
        let entries = self.entries.borrow();
        let learned = self.learned.borrow();
        let weak = Rc::downgrade(win);
        let wanikani = self.current.get() == Some(WANIKANI_LIST);
        let (shown, total) = (self.wk_shown.get(), self.wk_items.borrow().len());
        let more = || {
            let remaining = total.saturating_sub(shown);
            if !wanikani || remaining == 0 {
                return None;
            }
            let weak = weak.clone();
            Some(super::grid::More {
                remaining,
                load: Rc::new(move || {
                    if let Some(win) = weak.upgrade() {
                        let lists = win.lists_page().clone();
                        lists.append_page(&win, win.cards_visible());
                    }
                }),
            })
        };
        if tiles_on {
            self.tiles_box
                .set_child(Some(&super::grid::tiles(&weak, &entries, &learned, more())));
            self.view.set_visible_child_name("tiles");
        } else {
            self.tiles_box.set_child(None::<&gtk::Widget>);
            self.view.set_visible_child_name("rows");
        }
        if cards && cards_on && !win.split_collapsed() {
            let subtitle = if wanikani {
                format!("{shown} of {total} items after the filters")
            } else {
                format!("{} words", entries.len())
            };
            let wall = super::grid::cards(&weak, &self.title.text(), &subtitle, &entries, &learned, more());
            win.show_cards(&wall);
        }
    }

    /// One row: headword and reading, gloss and note, then the learned chip or a remove button.
    fn row(&self, win: &Rc<Window>, id: i64, e: &ListEntry, l: Option<&Learned>) -> adw::ActionRow {
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
        if let Some(l) = l {
            let chip = super::entry_view::learned_chip(l, "");
            chip.set_valign(gtk::Align::Center);
            row.add_suffix(&chip);
        } else {
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
        }
        row
    }

    /// The next page of the WaniKani list: resolves those items only, appends their rows, and
    /// a "Show more" row while items remain. Building every row at once froze the window with a
    /// real account's thousands of items.
    fn append_page(&self, win: &Rc<Window>, cards: bool) {
        if let Some(more) = self.more_row.borrow_mut().take() {
            self.entries_box.remove(&more);
        }
        let total = self.wk_items.borrow().len();
        let from = self.wk_shown.get();
        let to = (from + PAGE).min(total);
        let page: Vec<Learned> = self.wk_items.borrow()[from..to].to_vec();
        let (entries, learned) = self.wanikani_rows(win, &page);
        for (e, l) in entries.iter().zip(&learned) {
            self.entries_box
                .append(&self.row(win, WANIKANI_LIST, e, l.as_ref()));
        }
        self.entries.borrow_mut().extend(entries);
        self.learned.borrow_mut().extend(learned);
        self.wk_shown.set(to);
        if to < total {
            let more = adw::ActionRow::builder()
                .activatable(true)
                .title(format!("Show {} more", PAGE.min(total - to)))
                .subtitle(format!(
                    "{to} of {total} shown; the filters above narrow the list"
                ))
                .build();
            self.entries_box.append(&more);
            *self.more_row.borrow_mut() = Some(more.upcast::<gtk::ListBoxRow>());
        }
        self.after_rows(win, cards);
    }

    /// Opens the `index`-th entry of the open list (for the autopilot).
    pub fn open_index(&self, index: usize) {
        let entry = self.entries.borrow().get(index).cloned();
        if let (Some(entry), Some(win)) = (entry, self.window()) {
            win.open_list_entry(&entry);
        }
    }

    /// Selects rows of the three WaniKani filters (for the autopilot).
    pub fn set_filters(&self, kind: u32, level: u32, stage: u32) {
        self.kind.set_selected(kind);
        self.level.set_selected(level);
        self.stage.set_selected(stage);
    }

    /// The synced WaniKani items that pass the kind, level and stage filters.
    fn filtered_wanikani(&self, win: &Rc<Window>) -> Vec<Learned> {
        let all = win.user().learned_of("wanikani").unwrap_or_else(|e| {
            log::error!("cannot read the WaniKani items: {e:#}");
            Vec::new()
        });
        let kind = match self.kind.selected() {
            1 => Some(Kind::Vocabulary),
            2 => Some(Kind::Kanji),
            _ => None,
        };
        // (level, and whether everything below it counts too)
        let level = match self.level.selected() {
            0 => None,
            n if n <= 60 => Some((n, false)),
            n => Some((n - 59, true)),
        };
        // The stage drop-down's rows, from a WaniKani stage number.
        let stage_row = |s: u8| -> u32 {
            match s {
                0 => 7,
                1..=4 => 2,
                5 | 6 => 3,
                7 => 4,
                8 => 5,
                _ => 6,
            }
        };
        let wanted_stage = self.stage.selected();
        all.into_iter()
            .filter(|l| kind.is_none_or(|k| l.kind == k))
            .filter(|l| level.is_none_or(|(n, below)| if below { l.level <= n } else { l.level == n }))
            .filter(|l| match wanted_stage {
                0 => true,
                1 => l.stage != 0, // Unlocked: every stage but Locked
                row => stage_row(l.stage) == row,
            })
            .collect()
    }

    /// `items` resolved for display: words to their dictionary entries where one has the form
    /// (the first of the enabled sources wins), kanji as rows of their own, each with its
    /// learned item for the chip.
    fn wanikani_rows(&self, win: &Rc<Window>, items: &[Learned]) -> (Vec<ListEntry>, Vec<Option<Learned>>) {
        let items: Vec<&Learned> = items.iter().collect();
        // One lookup per chunk of forms, then the first entry that carries each form.
        let enabled = win.config().borrow().enabled_sources();
        let langs = win.config().borrow().gloss_languages.clone();
        let texts: Vec<String> = items
            .iter()
            .filter(|l| l.kind == Kind::Vocabulary)
            .map(|l| l.text.clone())
            .collect();
        let mut by_text: HashMap<&str, Entry> = HashMap::new();
        for chunk in texts.chunks(400) {
            let wanted: HashSet<&str> = chunk.iter().map(String::as_str).collect();
            let found = win
                .db()
                .lookup(chunk, &enabled, chunk.len() * 4)
                .unwrap_or_default();
            for entry in found {
                let forms: Vec<String> = entry.kanji.iter().chain(&entry.readings).cloned().collect();
                for form in forms {
                    if let Some(&key) = wanted.get(form.as_str())
                        && !by_text.contains_key(key)
                    {
                        by_text.insert(key, entry.clone());
                    }
                }
            }
        }
        let blank = |headword: &str, source: &str, gloss: &str| ListEntry {
            source: source.into(),
            seq: 0,
            headword: headword.into(),
            reading: String::new(),
            gloss: gloss.into(),
            note: String::new(),
            added: String::new(),
        };
        let mut entries = Vec::with_capacity(items.len());
        let mut learned = Vec::with_capacity(items.len());
        for l in items {
            let row = match (l.kind, by_text.get(l.text.as_str())) {
                (Kind::Kanji, _) => blank(&l.text, "kanji", "kanji"),
                // The row carries WaniKani's own form (今日は and こんにちは are two items), the
                // entry's reading when it adds something.
                (Kind::Vocabulary, Some(e)) => ListEntry {
                    source: e.source.clone(),
                    seq: e.id,
                    headword: l.text.clone(),
                    reading: if e.reading() == l.text {
                        String::new()
                    } else {
                        e.reading().to_string()
                    },
                    gloss: e.summary(&langs).to_string(),
                    note: String::new(),
                    added: String::new(),
                },
                (Kind::Vocabulary, None) => blank(&l.text, "", "not in the installed dictionaries"),
            };
            entries.push(row);
            learned.push(Some(l.clone()));
        }
        (entries, learned)
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
                Some("anki") => Layout::Anki,
                Some("kitsun") => Layout::Kitsun,
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
        if self.built_in(&win) {
            return;
        }
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
        if self.built_in(&win) {
            return;
        }
        if let Err(e) = win.user().move_list_up(id) {
            log::error!("cannot move list {id}: {e:#}");
        }
        win.lists_changed();
    }

    fn delete(&self) {
        let (Some(win), Some(id)) = (self.window(), self.current.get()) else {
            return;
        };
        if self.built_in(&win) {
            return;
        }
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
    /// The WaniKani list is built from the account, so its rows cannot be edited here.
    fn built_in(&self, win: &Rc<Window>) -> bool {
        if self.current.get() == Some(WANIKANI_LIST) {
            win.toast("The WaniKani list comes from your account; sync it on the Accounts page.");
            return true;
        }
        false
    }

    fn export(&self, layout: Layout) {
        let (Some(win), Some(id)) = (self.window(), self.current.get()) else {
            return;
        };
        let name = self.title.text().to_string();
        let items = if id == WANIKANI_LIST {
            let all = self.wk_items.borrow().clone();
            self.wanikani_rows(&win, &all).0
        } else {
            match win.user().entries(id) {
                Ok(e) => e,
                Err(e) => return win.toast(&format!("Cannot read the list: {e}")),
            }
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
        if self.built_in(&win) {
            return;
        }
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
