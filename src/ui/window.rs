//! The main window: search entry above a result list on the left, the selected entry on the right.

use std::cell::{Cell, RefCell};
use std::path::PathBuf;
use std::rc::Rc;
use std::time::Duration;

use adw::prelude::*;
use gtk::{gio, glib, glib::clone};

use super::entry_view::EntryView;
use super::import_dialog;
use crate::APP_NAME;
use crate::config::{Config, cache_dir};
use crate::model::Entry;
use crate::store::db::Database;
use crate::store::import;

const SEARCH_DEBOUNCE: Duration = Duration::from_millis(120);
const RESULT_LIMIT: usize = 100;

pub struct Window {
    pub win: adw::ApplicationWindow,
    pub search: gtk::SearchEntry,
    pub menu_button: gtk::MenuButton,
    config: Rc<RefCell<Config>>,
    db: Rc<Database>,
    db_path: PathBuf,
    results: gtk::ListBox,
    stack: gtk::Stack,
    split: adw::NavigationSplitView,
    toasts: adw::ToastOverlay,
    entry_view: EntryView,
    /// The entries behind the result rows, by row index.
    found: RefCell<Vec<Entry>>,
    current: RefCell<Option<Entry>>,
    search_timer: Cell<Option<glib::SourceId>>,
}

impl Window {
    pub fn new(
        app: &adw::Application,
        config: Rc<RefCell<Config>>,
        db: Rc<Database>,
        db_path: PathBuf,
    ) -> Rc<Self> {
        let state = config.borrow().window.clone();
        let win = adw::ApplicationWindow::builder()
            .application(app)
            .title(APP_NAME)
            .default_width(state.width)
            .default_height(state.height)
            .maximized(state.maximized)
            .build();

        // -- sidebar: search + results
        let search = gtk::SearchEntry::builder()
            .placeholder_text("Search Japanese, English or German…")
            .hexpand(true)
            .build();
        let menu = gio::Menu::new();
        menu.append(Some("Import dictionary…"), Some("win.import"));
        menu.append(Some("Preferences"), Some("app.preferences"));
        menu.append(Some("About Tango"), Some("app.about"));
        let menu_button = gtk::MenuButton::builder()
            .icon_name("open-menu-symbolic")
            .menu_model(&menu)
            .tooltip_text("Main menu")
            .build();
        let header = adw::HeaderBar::builder().show_title(false).build();
        header.pack_end(&menu_button);
        let results = gtk::ListBox::builder()
            .selection_mode(gtk::SelectionMode::Single)
            .css_classes(["navigation-sidebar"])
            .build();
        results.set_placeholder(Some(
            &adw::StatusPage::builder()
                .title("No results")
                .icon_name("edit-find-symbolic")
                .vexpand(true)
                .build(),
        ));
        let scroller = gtk::ScrolledWindow::builder()
            .child(&results)
            .vexpand(true)
            .hscrollbar_policy(gtk::PolicyType::Never)
            .build();
        let search_box = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .margin_start(6)
            .margin_end(6)
            .margin_top(6)
            .margin_bottom(6)
            .build();
        search_box.append(&search);
        let sidebar_box = gtk::Box::new(gtk::Orientation::Vertical, 0);
        sidebar_box.append(&search_box);
        sidebar_box.append(&scroller);
        let sidebar_view = adw::ToolbarView::builder().content(&sidebar_box).build();
        sidebar_view.add_top_bar(&header);
        let sidebar_page = adw::NavigationPage::builder()
            .child(&sidebar_view)
            .title("Search")
            .build();

        // -- content: the entry, or one of the empty states
        let entry_view = EntryView::new();
        let empty = adw::StatusPage::builder()
            .title("Tango")
            .description("Look up a word in Japanese, English or German.")
            .icon_name(crate::APP_ID)
            .vexpand(true)
            .build();
        let no_dictionary = adw::StatusPage::builder()
            .title("No dictionary yet")
            .description(
                "Tango needs the JMdict file from the EDRDG. It is about 25 MB and imports in a moment.",
            )
            .icon_name("folder-download-symbolic")
            .vexpand(true)
            .build();
        let download = gtk::Button::builder()
            .label("Download JMdict")
            .halign(gtk::Align::Center)
            .css_classes(["pill", "suggested-action"])
            .build();
        no_dictionary.set_child(Some(&download));
        let stack = gtk::Stack::builder()
            .transition_type(gtk::StackTransitionType::Crossfade)
            .build();
        stack.add_named(&no_dictionary, Some("no-dictionary"));
        stack.add_named(&empty, Some("empty"));
        stack.add_named(entry_view.widget(), Some("entry"));
        let content_view = adw::ToolbarView::builder().content(&stack).build();
        content_view.add_top_bar(&adw::HeaderBar::new());
        let content_page = adw::NavigationPage::builder()
            .child(&content_view)
            .title(APP_NAME)
            .build();

        let split = adw::NavigationSplitView::builder()
            .sidebar(&sidebar_page)
            .content(&content_page)
            .min_sidebar_width(280.0)
            .max_sidebar_width(420.0)
            .sidebar_width_fraction(0.32)
            .build();
        let toasts = adw::ToastOverlay::new();
        toasts.set_child(Some(&split));
        win.set_content(Some(&toasts));
        let breakpoint = adw::Breakpoint::new(adw::BreakpointCondition::parse("max-width: 640sp").unwrap());
        breakpoint.add_setter(&split, "collapsed", Some(&true.to_value()));
        win.add_breakpoint(breakpoint);

        let this = Rc::new(Self {
            win,
            search,
            menu_button,
            config,
            db,
            db_path,
            results,
            stack,
            split,
            toasts,
            entry_view,
            found: RefCell::new(Vec::new()),
            current: RefCell::new(None),
            search_timer: Cell::new(None),
        });

        // Callbacks hold *weak* references (`#[weak]`), so the window can be freed: a strong
        // `Rc` inside a closure owned by the window would keep it alive forever.
        this.search.connect_search_changed(clone!(
            #[weak]
            this,
            move |entry| this.schedule_search(entry.text().to_string())
        ));
        this.search.connect_activate(clone!(
            #[weak]
            this,
            move |_| this.select_result(0)
        ));
        this.results.connect_row_selected(clone!(
            #[weak]
            this,
            move |_, row| {
                if let Some(row) = row {
                    this.on_row_selected(row.index());
                }
            }
        ));
        download.connect_clicked(clone!(
            #[weak]
            this,
            move |_| this.download_jmdict()
        ));
        let search_action = gio::SimpleAction::new("search", None);
        search_action.connect_activate(clone!(
            #[weak]
            this,
            move |_, _| {
                this.search.grab_focus();
            }
        ));
        this.win.add_action(&search_action);
        let import_action = gio::SimpleAction::new("import", None);
        import_action.connect_activate(clone!(
            #[weak]
            this,
            move |_, _| this.choose_import_file()
        ));
        this.win.add_action(&import_action);
        this.win.connect_close_request(clone!(
            #[weak]
            this,
            #[upgrade_or]
            glib::Propagation::Proceed,
            move |win| {
                let mut cfg = this.config.borrow_mut();
                cfg.window.width = win.width();
                cfg.window.height = win.height();
                cfg.window.maximized = win.is_maximized();
                cfg.save();
                glib::Propagation::Proceed
            }
        ));

        this.refresh_state();
        this.search.grab_focus();
        this
    }

    // -- state ------------------------------------------------------------------------------

    fn refresh_state(&self) {
        let count = self.db.entry_count().unwrap_or(0);
        if count == 0 {
            self.stack.set_visible_child_name("no-dictionary");
        } else if self.current.borrow().is_none() {
            self.stack.set_visible_child_name("empty");
        }
    }

    pub fn config(&self) -> &Rc<RefCell<Config>> {
        &self.config
    }

    pub fn result_count(&self) -> usize {
        self.found.borrow().len()
    }

    pub fn current_entry_id(&self) -> Option<i64> {
        self.current.borrow().as_ref().map(|e| e.id)
    }

    // -- search -----------------------------------------------------------------------------

    fn schedule_search(self: &Rc<Self>, query: String) {
        if let Some(id) = self.search_timer.take() {
            id.remove();
        }
        let this = Rc::downgrade(self);
        let id = glib::timeout_add_local_once(SEARCH_DEBOUNCE, move || {
            if let Some(this) = this.upgrade() {
                this.search_timer.set(None);
                this.run_search(&query);
            }
        });
        self.search_timer.set(Some(id));
    }

    fn run_search(&self, query: &str) {
        let entries = match self.db.search(query, RESULT_LIMIT) {
            Ok(entries) => entries,
            Err(e) => {
                log::error!("search failed: {e:#}");
                Vec::new()
            }
        };
        let langs = self.config.borrow().gloss_languages.clone();
        self.results.remove_all();
        for e in &entries {
            self.results.append(&result_row(e, &langs));
        }
        let any = !entries.is_empty();
        *self.found.borrow_mut() = entries;
        if any && !query.trim().is_empty() {
            self.select_result(0);
        }
    }

    pub fn select_result(&self, index: usize) {
        if let Some(row) = self.results.row_at_index(index as i32) {
            self.results.select_row(Some(&row));
        }
    }

    fn on_row_selected(&self, index: i32) {
        let entry = self.found.borrow().get(index as usize).cloned();
        if let Some(entry) = entry {
            self.show_entry(entry);
            if self.split.is_collapsed() {
                self.split.set_show_content(true);
            }
        }
    }

    pub fn show_entry(&self, entry: Entry) {
        let langs = self.config.borrow().gloss_languages.clone();
        self.entry_view.show(&entry, &langs);
        *self.current.borrow_mut() = Some(entry);
        self.stack.set_visible_child_name("entry");
    }

    /// Re-renders the current entry, e.g. after the gloss language order changed.
    pub fn rerender(&self) {
        let current = self.current.borrow().clone();
        if let Some(entry) = current {
            self.show_entry(entry);
        }
    }

    // -- import -----------------------------------------------------------------------------

    pub fn download_jmdict(self: &Rc<Self>) {
        let path = self.db_path.clone();
        let cache = cache_dir();
        self.run_import(move |report| {
            let db = Database::open(&path)?;
            import::download_and_import_jmdict(&db, &cache, report)
        });
    }

    pub fn import_file(self: &Rc<Self>, file: PathBuf) {
        let path = self.db_path.clone();
        self.run_import(move |report| {
            let db = Database::open(&path)?;
            import::import_jmdict(&db, &file, report)
        });
    }

    fn run_import(
        self: &Rc<Self>,
        job: impl FnOnce(import::Report) -> anyhow::Result<usize> + Send + 'static,
    ) {
        let this = Rc::downgrade(self);
        import_dialog::run(&self.win, job, move |result| {
            let Some(this) = this.upgrade() else { return };
            match result {
                Ok(n) => this
                    .toasts
                    .add_toast(adw::Toast::new(&format!("Imported {n} entries"))),
                Err(e) => {
                    let toast = adw::Toast::new(&format!("Import failed: {e}"));
                    toast.set_timeout(0);
                    this.toasts.add_toast(toast);
                }
            }
            *this.current.borrow_mut() = None;
            this.refresh_state();
            let query = this.search.text().to_string();
            this.run_search(&query);
        });
    }

    fn choose_import_file(self: &Rc<Self>) {
        let filter = gtk::FileFilter::new();
        filter.set_name(Some("JMdict XML"));
        filter.add_pattern("*.xml");
        filter.add_pattern("*.gz");
        filter.add_pattern("JMdict*");
        let filters = gio::ListStore::new::<gtk::FileFilter>();
        filters.append(&filter);
        let dialog = gtk::FileDialog::builder()
            .title("Import JMdict")
            .filters(&filters)
            .build();
        let this = Rc::downgrade(self);
        dialog.open(Some(&self.win), gio::Cancellable::NONE, move |result| {
            if let (Ok(file), Some(this)) = (result, this.upgrade())
                && let Some(path) = file.path()
            {
                this.import_file(path);
            }
        });
    }
}

fn result_row(entry: &Entry, langs: &[String]) -> adw::ActionRow {
    let mut title = glib::markup_escape_text(entry.headword()).to_string();
    if !entry.kanji.is_empty() && !entry.reading().is_empty() {
        title.push_str(&format!(
            "  <span alpha='70%'>{}</span>",
            glib::markup_escape_text(entry.reading())
        ));
    }
    let row = adw::ActionRow::builder()
        .activatable(true)
        .use_markup(true)
        .title(&title)
        .subtitle(glib::markup_escape_text(entry.summary(langs)).as_str())
        .title_lines(1)
        .subtitle_lines(1)
        .build();
    if entry.common {
        let tag = gtk::Label::builder()
            .label("common")
            .valign(gtk::Align::Center)
            .css_classes(["tango-common"])
            .build();
        row.add_suffix(&tag);
    }
    row
}
