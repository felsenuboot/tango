//! The main window: search entry above a result list on the left, the selected entry on the right.

use std::cell::{Cell, RefCell};
use std::path::PathBuf;
use std::rc::Rc;
use std::time::Duration;

use adw::prelude::*;
use gtk::{gio, glib, glib::clone};

use super::entry_view::EntryView;
use super::import_dialog;
use super::thousands;
use crate::APP_NAME;
use crate::config::{Config, cache_dir};
use crate::dict::sources::{self, Source};
use crate::model::Entry;
use crate::search::{self, Hit};
use crate::store::db::Database;
use crate::store::import;

const SEARCH_DEBOUNCE: Duration = Duration::from_millis(120);
const RESULT_LIMIT: usize = 100;

pub struct Window {
    pub win: adw::ApplicationWindow,
    pub search: gtk::SearchEntry,
    pub menu_button: gtk::MenuButton,
    /// "Import the downloaded copy" on the empty state, shown when the cache has a JMdict file.
    import_cached: gtk::Button,
    config: Rc<RefCell<Config>>,
    db: Rc<Database>,
    db_path: PathBuf,
    results: gtk::ListBox,
    /// The "No results" page under the list; its description carries search hints.
    no_results: adw::StatusPage,
    stack: gtk::Stack,
    split: adw::NavigationSplitView,
    toasts: adw::ToastOverlay,
    entry_view: EntryView,
    /// The results behind the rows, by row index.
    found: RefCell<Vec<Hit>>,
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
        keep_menu_out_of_reserved_strip(&win, &menu_button);
        let header = adw::HeaderBar::builder().show_title(false).build();
        header.pack_end(&menu_button);
        let results = gtk::ListBox::builder()
            .selection_mode(gtk::SelectionMode::Single)
            .css_classes(["navigation-sidebar"])
            .build();
        let no_results = adw::StatusPage::builder()
            .title("No results")
            .icon_name("edit-find-symbolic")
            .vexpand(true)
            .build();
        results.set_placeholder(Some(&no_results));
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
            .description(format!(
                "Tango needs the JMdict file from the EDRDG. It is about {} MB and imports in a moment.",
                sources::JMDICT.size_mb
            ))
            .icon_name("folder-download-symbolic")
            .vexpand(true)
            .build();
        let download = gtk::Button::builder()
            .label("Download JMdict")
            .css_classes(["pill", "suggested-action"])
            .build();
        let import_cached = gtk::Button::builder()
            .label("Import the downloaded copy")
            .css_classes(["pill"])
            .visible(false)
            .build();
        let actions = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(12)
            .halign(gtk::Align::Center)
            .build();
        actions.append(&download);
        actions.append(&import_cached);
        no_dictionary.set_child(Some(&actions));
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
            import_cached,
            config,
            db,
            db_path,
            results,
            no_results,
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
        // A sentence cut into words gets a header above the first row of each word.
        this.results.set_header_func(clone!(
            #[weak]
            this,
            move |row, before| {
                let found = this.found.borrow();
                let group = found.get(row.index() as usize).and_then(|h| h.group.as_deref());
                let previous = before
                    .and_then(|b| found.get(b.index() as usize))
                    .and_then(|h| h.group.as_deref());
                match group {
                    Some(word) if group != previous => row.set_header(Some(&group_header(word))),
                    _ => row.set_header(None::<&gtk::Widget>),
                }
            }
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
            move |_| this.download_source(&sources::JMDICT, || {})
        ));
        this.import_cached.connect_clicked(clone!(
            #[weak]
            this,
            move |_| this.import_file(cache_dir().join(sources::JMDICT.filename))
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
            move |_, _| this.choose_import_file(&sources::JMDICT, || {})
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
        self.import_cached
            .set_visible(cache_dir().join(sources::JMDICT.filename).exists());
        if count == 0 {
            self.stack.set_visible_child_name("no-dictionary");
        } else if self.current.borrow().is_none() {
            self.stack.set_visible_child_name("empty");
        }
    }

    pub fn config(&self) -> &Rc<RefCell<Config>> {
        &self.config
    }

    pub fn db(&self) -> &Rc<Database> {
        &self.db
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

    /// Runs the current search again, e.g. after the dictionaries changed.
    pub fn refresh_search(&self) {
        let query = self.search.text().to_string();
        self.run_search(&query);
    }

    fn run_search(&self, query: &str) {
        let enabled = self.config.borrow().enabled_sources();
        let started = std::time::Instant::now();
        let outcome = match search::run(&self.db, query, RESULT_LIMIT, &enabled) {
            Ok(outcome) => outcome,
            Err(e) => {
                log::error!("search failed: {e:#}");
                search::Outcome::default()
            }
        };
        log::debug!(
            "search {query:?}: {} hits in {} ms",
            outcome.hits.len(),
            started.elapsed().as_millis()
        );
        let langs = self.config.borrow().gloss_languages.clone();
        self.no_results.set_description(outcome.hint.as_deref());
        let any = !outcome.hits.is_empty();
        self.results.remove_all();
        *self.found.borrow_mut() = outcome.hits;
        for hit in self.found.borrow().iter() {
            self.results.append(&result_row(hit, &langs));
        }
        self.results.invalidate_headers();
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
        let entry = self.found.borrow().get(index as usize).map(|h| h.entry.clone());
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

    /// Downloads `source` into the cache and imports it, replacing what the database had of it.
    /// `after` runs on the main thread once the job is done, success or not.
    pub fn download_source(self: &Rc<Self>, source: &'static Source, after: impl FnOnce() + 'static) {
        let path = self.db_path.clone();
        let cache = cache_dir();
        self.run_job(
            move |report| {
                let db = Database::open(&path)?;
                let n = import::download_and_import(&db, source, &cache, report)?;
                Ok(format!(
                    "Imported {} {} entries",
                    thousands(n as i64),
                    source.name
                ))
            },
            after,
        );
    }

    /// Imports a local JMdict file: the menu action, the autopilot `import` step, the cached copy.
    pub fn import_file(self: &Rc<Self>, file: PathBuf) {
        self.import_source_file(&sources::JMDICT, file, || {});
    }

    pub fn import_source_file(
        self: &Rc<Self>,
        source: &'static Source,
        file: PathBuf,
        after: impl FnOnce() + 'static,
    ) {
        let path = self.db_path.clone();
        self.run_job(
            move |report| {
                let db = Database::open(&path)?;
                let n = import::import_file(&db, source, &file, report)?;
                Ok(format!(
                    "Imported {} {} entries",
                    thousands(n as i64),
                    source.name
                ))
            },
            after,
        );
    }

    pub fn remove_source(self: &Rc<Self>, source: &'static Source, after: impl FnOnce() + 'static) {
        let path = self.db_path.clone();
        let cache = cache_dir();
        self.run_job(
            move |report| {
                let db = Database::open(&path)?;
                import::remove(&db, source, &cache, report)?;
                Ok(format!("Removed {}", source.name))
            },
            after,
        );
    }

    /// Runs a job on the worker thread behind the progress dialog; the job's `Ok` text becomes a
    /// toast. Then the window state and the search are refreshed and `after` runs.
    fn run_job(
        self: &Rc<Self>,
        job: impl FnOnce(import::Report) -> anyhow::Result<String> + Send + 'static,
        after: impl FnOnce() + 'static,
    ) {
        let this = Rc::downgrade(self);
        import_dialog::run(&self.win, job, move |result| {
            let Some(this) = this.upgrade() else { return };
            match result {
                Ok(message) => this.toasts.add_toast(adw::Toast::new(&message)),
                Err(e) => {
                    let toast = adw::Toast::new(&format!("Failed: {e}"));
                    toast.set_timeout(0);
                    this.toasts.add_toast(toast);
                }
            }
            *this.current.borrow_mut() = None;
            this.refresh_state();
            this.refresh_search();
            after();
        });
    }

    pub fn choose_import_file(self: &Rc<Self>, source: &'static Source, after: impl FnOnce() + 'static) {
        let filter = gtk::FileFilter::new();
        filter.set_name(Some(&format!("{} file", source.name)));
        filter.add_pattern("*.xml");
        filter.add_pattern("*.gz");
        filter.add_pattern(&format!("{}*", source.name));
        let filters = gio::ListStore::new::<gtk::FileFilter>();
        filters.append(&filter);
        let dialog = gtk::FileDialog::builder()
            .title(format!("Import {}", source.name))
            .filters(&filters)
            .build();
        let this = Rc::downgrade(self);
        dialog.open(Some(&self.win), gio::Cancellable::NONE, move |result| {
            if let (Ok(file), Some(this)) = (result, this.upgrade())
                && let Some(path) = file.path()
            {
                this.import_source_file(source, path, after);
            }
        });
    }
}

fn group_header(word: &str) -> gtk::Label {
    gtk::Label::builder()
        .label(word)
        .xalign(0.0)
        .margin_start(12)
        .margin_top(12)
        .margin_bottom(4)
        .css_classes(["heading"])
        .build()
}

fn result_row(hit: &Hit, langs: &[String]) -> adw::ActionRow {
    let entry = &hit.entry;
    let mut title = glib::markup_escape_text(entry.headword()).to_string();
    if !entry.kanji.is_empty() && !entry.reading().is_empty() {
        title.push_str(&format!(
            "  <span alpha='70%'>{}</span>",
            glib::markup_escape_text(entry.reading())
        ));
    }
    // A deinflected match says how it was reached above the gloss.
    let mut subtitle = glib::markup_escape_text(entry.summary(langs)).to_string();
    if let Some(note) = &hit.note {
        subtitle = format!("<i>{}</i>\n{subtitle}", glib::markup_escape_text(note));
    }
    let row = adw::ActionRow::builder()
        .activatable(true)
        .use_markup(true)
        .title(&title)
        .subtitle(&subtitle)
        .title_lines(1)
        .subtitle_lines(if hit.note.is_some() { 2 } else { 1 })
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

/// Workaround for a Hyprland bug (0.56, still in master as of 2026-09). Hyprland keeps a window's
/// popups out of the strip a top bar reserves, even when the window is fullscreen and covers the
/// bar. GTK asks for the menu popover 40 px below the window's top edge; with a 52 px bar Hyprland
/// answers by shrinking the popup by the overlap instead of sliding it down, and GTK shows the three
/// menu items with a scrollbar. Asking GTK to move the open popover down makes Hyprland cut it even
/// more, but a popover that *opens* lower is left alone. So on the first shrunk open the missing
/// height becomes the popover's offset and the menu is reopened; the offset stays while the window
/// is fullscreen and is dropped when it leaves fullscreen.
fn keep_menu_out_of_reserved_strip(win: &adw::ApplicationWindow, menu_button: &gtk::MenuButton) {
    let Some(popover) = menu_button.popover() else {
        return;
    };
    let Some(scrolled) = find_scrolled_window(popover.upcast_ref()) else {
        return;
    };
    scrolled.vadjustment().connect_changed(clone!(
        #[weak]
        win,
        #[weak]
        popover,
        move |adj| {
            let shortfall = (adj.upper() - adj.page_size()).ceil() as i32;
            // Only a small cut of a fullscreen window's menu; a menu taller than the screen scrolls.
            let bar_sized = (1..=128).contains(&shortfall);
            if !bar_sized || !win.is_fullscreen() || !popover.is_mapped() || popover.offset().1 != 0 {
                return;
            }
            // Reopening from inside GTK's size negotiation would fight it; do it right afterwards.
            glib::idle_add_local_once(clone!(
                #[weak]
                popover,
                move || {
                    popover.set_offset(0, shortfall);
                    popover.popdown();
                    popover.popup();
                }
            ));
        }
    ));
    win.connect_fullscreened_notify(clone!(
        #[weak]
        popover,
        move |win| {
            if !win.is_fullscreen() {
                popover.set_offset(0, 0);
            }
        }
    ));
}

/// The first `gtk::ScrolledWindow` below `widget`, depth first. GTK's menu popover keeps its items
/// in one, which is not exposed through the API.
pub fn find_scrolled_window(widget: &gtk::Widget) -> Option<gtk::ScrolledWindow> {
    if let Some(found) = widget.downcast_ref::<gtk::ScrolledWindow>() {
        return Some(found.clone());
    }
    let mut child = widget.first_child();
    while let Some(c) = child {
        if let Some(found) = find_scrolled_window(&c) {
            return Some(found);
        }
        child = c.next_sibling();
    }
    None
}
