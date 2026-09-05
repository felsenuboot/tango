//! The main window: search entry above a result list on the left, the selected entry on the right.

use std::cell::{Cell, RefCell};
use std::path::PathBuf;
use std::rc::Rc;
use std::time::Duration;

use adw::prelude::*;
use gtk::{gdk, gio, glib, glib::clone};

use super::entry_view::EntryView;
use super::jobs::{self, Jobs};
use super::kanji_view::KanjiView;
use super::lists::{ListsPage, ask_name};
use super::radicals::RadicalsPage;
use super::sentence_view::SentenceView;
use super::thousands;
use crate::APP_NAME;
use crate::accounts::{self, Kind, wanikani};
use crate::config::user_database_path;
use crate::config::{Config, cache_dir};
use crate::dict::sources::{self, Source};
use crate::dict::tatoeba;
use crate::model::{Entry, Sentence, SentenceWord};
use crate::search::{self, Hit};
use crate::secrets;
use crate::store::db::Database;
use crate::store::export::TakobotoRow;
use crate::store::import;
use crate::store::now_iso8601;
use crate::store::user::{LearnedIndex, ListEntry, UserDb};

const SEARCH_DEBOUNCE: Duration = Duration::from_millis(120);
const RESULT_LIMIT: usize = 100;
/// Views the back button remembers.
const HISTORY_LIMIT: usize = 100;

/// What the content pane can show, as the history keeps it.
#[derive(Clone, PartialEq)]
enum View {
    Entry(Entry),
    Kanji(char),
    Sentence(Sentence),
}

/// Example sentences shown under an entry at first, and after "Show all".
const EXAMPLES_FIRST: usize = 5;
const EXAMPLES_ALL: usize = 100;

pub struct Window {
    pub win: adw::ApplicationWindow,
    pub search: gtk::SearchEntry,
    pub menu_button: gtk::MenuButton,
    /// "Import the downloaded copy" on the empty state, shown when the cache has a JMdict file.
    import_cached: gtk::Button,
    download_jmdict: gtk::Button,
    /// Under the empty state's buttons: what the queue is doing.
    job_status: gtk::Label,
    /// Downloads, imports and removals, one after the other on a worker thread.
    jobs: Rc<Jobs>,
    /// In the sidebar header while a job runs.
    busy: gtk::Spinner,
    config: Rc<RefCell<Config>>,
    db: Rc<Database>,
    db_path: PathBuf,
    user: Rc<UserDb>,
    lists: Rc<ListsPage>,
    radicals: Rc<RadicalsPage>,
    /// Search / Lists in the sidebar.
    sidebar_stack: adw::ViewStack,
    /// Star = in Favourites; the menu button next to it picks any list.
    star: gtk::ToggleButton,
    add_to_list: gtk::MenuButton,
    /// "Open in Takoboto" for JMdict entries.
    open_in: gtk::MenuButton,
    results: gtk::ListBox,
    /// The "No results" page under the list; its description carries search hints.
    no_results: adw::StatusPage,
    stack: gtk::Stack,
    split: adw::NavigationSplitView,
    toasts: adw::ToastOverlay,
    entry_view: EntryView,
    kanji_view: Rc<KanjiView>,
    sentence_view: SentenceView,
    /// Back from the kanji page to the entry.
    back: gtk::Button,
    /// "Kanji 猫" under the search box when the query is one kanji with data behind it.
    kanji_hint: gtk::Button,
    /// On the "No results" page: the query on jisho.org, in the browser.
    search_online: gtk::Button,
    /// The results behind the rows, by row index.
    found: RefCell<Vec<Hit>>,
    /// The rows of a `#sentences` search instead; `found` is empty then.
    found_sentences: RefCell<Vec<Sentence>>,
    current: RefCell<Option<Entry>>,
    /// What the content pane shows, and the views before it, for the back button (issue #72).
    view: RefCell<Option<View>>,
    history: RefCell<Vec<View>>,
    /// Set while `go_back` re-shows a view, so it is not pushed again.
    going_back: Cell<bool>,
    /// How many example sentences the entry shows: a few, or all after "Show all".
    example_limit: Cell<usize>,
    /// What the learning accounts know, for the `#known` filters; reloaded after a sync.
    learned: RefCell<LearnedIndex>,
    search_timer: Cell<Option<glib::SourceId>>,
}

impl Window {
    pub fn new(
        app: &adw::Application,
        config: Rc<RefCell<Config>>,
        db: Rc<Database>,
        db_path: PathBuf,
        user: Rc<UserDb>,
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
        let header = adw::HeaderBar::new();
        header.pack_end(&menu_button);
        let busy = gtk::Spinner::builder().visible(false).build();
        header.pack_end(&busy);
        let results = gtk::ListBox::builder()
            .selection_mode(gtk::SelectionMode::Single)
            .css_classes(["navigation-sidebar"])
            .build();
        let no_results = adw::StatusPage::builder()
            .title("No results")
            .icon_name("edit-find-symbolic")
            .vexpand(true)
            .build();
        // The way out when the local dictionaries have nothing: the same query on jisho.org, in
        // the browser. Nothing is fetched by Tango itself.
        let search_online = gtk::Button::builder()
            .label("Search on jisho.org")
            .halign(gtk::Align::Center)
            .css_classes(["pill"])
            .sensitive(false)
            .build();
        no_results.set_child(Some(&search_online));
        results.set_placeholder(Some(&no_results));
        let scroller = gtk::ScrolledWindow::builder()
            .child(&results)
            .vexpand(true)
            .hscrollbar_policy(gtk::PolicyType::Never)
            .build();
        // GNOME HIG: 12 px between content and the edges of its container; the list rows below
        // bring their own padding, so the entry keeps only 6 px towards them.
        let search_box = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .margin_start(12)
            .margin_end(12)
            .margin_top(12)
            .margin_bottom(6)
            .build();
        search_box.append(&search);
        // A flat button pads its label by about 10 px, so 8 px lines its text up with the
        // search entry's (12 px margin + 8 px inner padding).
        let kanji_hint = gtk::Button::builder()
            .halign(gtk::Align::Start)
            .margin_start(8)
            .margin_bottom(6)
            .css_classes(["flat"])
            .visible(false)
            .build();
        let sidebar_box = gtk::Box::new(gtk::Orientation::Vertical, 0);
        sidebar_box.append(&search_box);
        sidebar_box.append(&kanji_hint);
        sidebar_box.append(&scroller);
        // The sidebar holds two pages, Search and Lists, switched in its header bar.
        let lists = ListsPage::new();
        let sidebar_stack = adw::ViewStack::new();
        sidebar_stack
            .add_titled(&sidebar_box, Some("search"), "Search")
            .set_icon_name(Some("edit-find-symbolic"));
        sidebar_stack
            .add_titled(&lists.widget, Some("lists"), "Lists")
            .set_icon_name(Some("view-list-symbolic"));
        let radicals = RadicalsPage::new();
        sidebar_stack
            .add_titled(&radicals.widget, Some("radicals"), "Kanji")
            .set_icon_name(Some("accessories-character-map-symbolic"));
        let switcher = adw::ViewSwitcher::builder()
            .stack(&sidebar_stack)
            .policy(adw::ViewSwitcherPolicy::Wide)
            .build();
        header.set_title_widget(Some(&switcher));
        let sidebar_view = adw::ToolbarView::builder().content(&sidebar_stack).build();
        sidebar_view.add_top_bar(&header);
        let sidebar_page = adw::NavigationPage::builder()
            .child(&sidebar_view)
            .title("Search")
            .build();

        // -- content: the entry, the kanji page, or one of the empty states
        let entry_view = EntryView::new();
        let kanji_view = KanjiView::new();
        let sentence_view = SentenceView::new();
        // "Look it up" on the start screen searches 単語 in Tango itself.
        let start = super::start::build(clone!(
            #[weak]
            search,
            #[weak]
            sidebar_stack,
            move || {
                sidebar_stack.set_visible_child_name("search");
                search.set_text("単語");
                search.grab_focus();
            }
        ));
        let empty = start.widget.clone();
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
        let job_status = gtk::Label::builder()
            .wrap(true)
            .justify(gtk::Justification::Center)
            .css_classes(["dim-label"])
            .build();
        let actions = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(12)
            .halign(gtk::Align::Center)
            .build();
        actions.append(&download);
        actions.append(&import_cached);
        actions.append(&job_status);
        no_dictionary.set_child(Some(&actions));
        let stack = gtk::Stack::builder()
            .transition_type(gtk::StackTransitionType::Crossfade)
            .build();
        stack.add_named(&no_dictionary, Some("no-dictionary"));
        stack.add_named(&empty, Some("empty"));
        stack.add_named(entry_view.widget(), Some("entry"));
        stack.add_named(kanji_view.widget(), Some("kanji"));
        stack.add_named(sentence_view.widget(), Some("sentence"));
        let content_header = adw::HeaderBar::new();
        let back = gtk::Button::builder()
            .icon_name("go-previous-symbolic")
            .tooltip_text("Back")
            .visible(false)
            .build();
        content_header.pack_start(&back);
        let star = gtk::ToggleButton::builder()
            .icon_name("non-starred-symbolic")
            .action_name("win.star")
            .tooltip_text("Favourite (Ctrl+D)")
            .sensitive(false)
            .build();
        let add_to_list = gtk::MenuButton::builder()
            .icon_name("view-list-symbolic")
            .tooltip_text("Add to a list")
            .sensitive(false)
            .popover(&gtk::Popover::new())
            .build();
        let open_in = gtk::MenuButton::builder()
            .icon_name("external-link-symbolic")
            .menu_model(&open_in_menu(false))
            .tooltip_text("Open on another site")
            .sensitive(false)
            .build();
        content_header.pack_end(&open_in);
        content_header.pack_end(&add_to_list);
        content_header.pack_end(&star);
        let content_view = adw::ToolbarView::builder().content(&stack).build();
        content_view.add_top_bar(&content_header);
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
        // The wide switcher (icon and label side by side) needs about 330 px for its three
        // pages; the sidebar is 32 % of the window down to 280 px, so below about 1030 px the
        // labels would be cut to "Se…". The narrow policy stacks the label under the icon.
        // The two conditions do not overlap, so it does not matter which one libadwaita picks
        // when several match; the collapsed layout repeats the setter.
        let narrow = adw::Breakpoint::new(
            adw::BreakpointCondition::parse("max-width: 1030sp and min-width: 641sp").unwrap(),
        );
        narrow.add_setter(
            &switcher,
            "policy",
            Some(&adw::ViewSwitcherPolicy::Narrow.to_value()),
        );
        // The start screen's calligraphy and entry fit side by side only above 1030 px; below
        // that they stack and the calligraphy shrinks (#91). The collapsed layout repeats it.
        let breakpoint = adw::Breakpoint::new(adw::BreakpointCondition::parse("max-width: 640sp").unwrap());
        breakpoint.add_setter(&split, "collapsed", Some(&true.to_value()));
        breakpoint.add_setter(
            &switcher,
            "policy",
            Some(&adw::ViewSwitcherPolicy::Narrow.to_value()),
        );
        for b in [&narrow, &breakpoint] {
            b.add_setter(
                &start.row,
                "orientation",
                Some(&gtk::Orientation::Vertical.to_value()),
            );
            if let Some(art) = &start.art {
                b.add_setter(art, "content-width", Some(&240.to_value()));
                b.add_setter(art, "content-height", Some(&116.to_value()));
            }
        }
        win.add_breakpoint(narrow);
        win.add_breakpoint(breakpoint);

        let learned = user.learned_index().unwrap_or_else(|e| {
            log::error!("cannot read the learned items: {e:#}");
            LearnedIndex::new()
        });
        let this = Rc::new(Self {
            win,
            search,
            menu_button,
            import_cached,
            download_jmdict: download.clone(),
            job_status,
            jobs: Jobs::new(),
            busy,
            config,
            db,
            db_path,
            user,
            lists,
            radicals,
            sidebar_stack,
            star,
            add_to_list,
            open_in,
            results,
            no_results,
            stack,
            split,
            toasts,
            entry_view,
            kanji_view,
            sentence_view,
            back,
            kanji_hint,
            search_online,
            found: RefCell::new(Vec::new()),
            found_sentences: RefCell::new(Vec::new()),
            current: RefCell::new(None),
            view: RefCell::new(None),
            history: RefCell::new(Vec::new()),
            going_back: Cell::new(false),
            example_limit: Cell::new(EXAMPLES_FIRST),
            learned: RefCell::new(learned),
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
        // Escape: the search is cleared and the empty state comes back.
        this.search.connect_stop_search(|entry| entry.set_text(""));
        this.install_row_menu();
        this.entry_view.connect_kanji(clone!(
            #[weak]
            this,
            move |c| this.show_kanji(c)
        ));
        this.entry_view.connect_ref(clone!(
            #[weak]
            this,
            move |reference| this.open_reference(reference)
        ));
        this.kanji_view.connect_word(clone!(
            #[weak]
            this,
            move |entry| this.show_entry(entry.clone())
        ));
        this.sentence_view.connect_word(clone!(
            #[weak]
            this,
            move |word| this.open_word(word)
        ));
        this.entry_view.connect_more(clone!(
            #[weak]
            this,
            move || this.more_examples()
        ));
        this.back.connect_clicked(clone!(
            #[weak]
            this,
            move |_| this.go_back()
        ));
        this.search_online.connect_clicked(clone!(
            #[weak]
            this,
            move |_| {
                let query = this.search.text().to_string();
                let escaped = glib::Uri::escape_string(query.trim(), None, false);
                this.open_url(&format!("https://jisho.org/search/{escaped}"));
            }
        ));
        this.kanji_hint.connect_clicked(clone!(
            #[weak]
            this,
            move |button| {
                if let Some(c) = button.label().and_then(|l| l.chars().last()) {
                    this.show_kanji(c);
                }
            }
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
        let back_action = gio::SimpleAction::new("back", None);
        back_action.connect_activate(clone!(
            #[weak]
            this,
            move |_, _| this.go_back()
        ));
        this.win.add_action(&back_action);
        let star_action = gio::SimpleAction::new("star", None);
        star_action.connect_activate(clone!(
            #[weak]
            this,
            move |_, _| this.toggle_favourite()
        ));
        this.win.add_action(&star_action);
        let open_in_action = gio::SimpleAction::new("open-in", Some(&String::static_variant_type()));
        open_in_action.connect_activate(clone!(
            #[weak]
            this,
            move |_, target| {
                if let Some(site) = target.and_then(|t| t.get::<String>()) {
                    this.open_in(&site);
                }
            }
        ));
        this.win.add_action(&open_in_action);
        if let Some(popover) = this.add_to_list.popover() {
            popover.connect_show(clone!(
                #[weak]
                this,
                move |popover| this.fill_list_popover(popover)
            ));
        }
        this.lists.attach(&this);
        this.radicals.attach(&this);
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
                {
                    let mut cfg = this.config.borrow_mut();
                    cfg.window.width = win.width();
                    cfg.window.height = win.height();
                    cfg.window.maximized = win.is_maximized();
                    cfg.save();
                }
                if let Some(current) = this.jobs.current() {
                    this.confirm_quit(&current.title);
                    return glib::Propagation::Stop;
                }
                glib::Propagation::Proceed
            }
        ));
        this.jobs.connect(clone!(
            #[weak]
            this,
            #[upgrade_or]
            false,
            move || {
                this.refresh_busy();
                true
            }
        ));

        this.refresh_state();
        this.search.grab_focus();
        super::name_icon_buttons(this.win.upcast_ref());
        this
    }

    // -- state ------------------------------------------------------------------------------

    /// The header spinner and the empty state follow the job queue.
    fn refresh_busy(&self) {
        let current = self.jobs.current();
        self.busy.set_visible(current.is_some());
        self.busy.set_spinning(current.is_some());
        let queued = self.jobs.queued();
        let status = match &current {
            Some(c) if queued > 0 => format!("{}: {} ({queued} more queued)", c.title, c.message),
            Some(c) => format!("{}: {}", c.title, c.message),
            None => String::new(),
        };
        self.busy.set_tooltip_text(Some(&status));
        self.job_status.set_text(&status);
        self.download_jmdict.set_sensitive(current.is_none());
        self.import_cached.set_sensitive(current.is_none());
    }

    /// Closing the window while a job runs: keep it running, or quit and lose the job.
    fn confirm_quit(self: &Rc<Self>, title: &str) {
        let dialog = adw::AlertDialog::builder()
            .heading("A job is still running")
            .body(format!(
                "{title} is not finished. Quitting now stops it; an interrupted import is dropped and \
                 can be started again."
            ))
            .build();
        dialog.add_responses(&[("keep", "Keep running"), ("quit", "Quit anyway")]);
        dialog.set_response_appearance("quit", adw::ResponseAppearance::Destructive);
        dialog.set_default_response(Some("keep"));
        dialog.set_close_response("keep");
        dialog.connect_response(
            Some("quit"),
            clone!(
                #[weak(rename_to = this)]
                self,
                move |_, _| this.win.destroy()
            ),
        );
        dialog.present(Some(&self.win));
    }

    pub fn jobs(&self) -> &Rc<Jobs> {
        &self.jobs
    }

    /// After a schema rebuild: imports every source whose download is still in the cache.
    pub fn reimport_cached(self: &Rc<Self>) {
        let cached: Vec<&'static Source> = sources::SOURCES
            .iter()
            .copied()
            .filter(|s| cache_dir().join(s.filename).exists())
            .collect();
        if cached.is_empty() {
            return;
        }
        self.toast("The dictionary database was rebuilt for this version; importing the downloaded dictionaries again.");
        for source in cached {
            self.import_source_file(source, cache_dir().join(source.filename), || {});
        }
    }

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

    pub fn user(&self) -> &Rc<UserDb> {
        &self.user
    }

    pub fn toast(&self, text: &str) {
        self.toasts.add_toast(adw::Toast::new(text));
    }

    /// Shows the Search or the Lists page of the sidebar.
    pub fn show_sidebar_page(&self, name: &str) {
        self.sidebar_stack.set_visible_child_name(name);
    }

    pub fn lists_page(&self) -> &Rc<ListsPage> {
        &self.lists
    }

    pub fn radicals_page(&self) -> &Rc<RadicalsPage> {
        &self.radicals
    }

    // -- word lists -------------------------------------------------------------------------

    /// After any change to the lists: the Lists page, the star, and the marks in the results.
    pub fn lists_changed(&self) {
        self.lists.refresh();
        self.refresh_star();
        self.refresh_search();
    }

    /// The first gloss in the preferred language, what a list row shows for the entry.
    fn list_gloss(&self, entry: &Entry) -> String {
        entry.summary(&self.config.borrow().gloss_languages).to_string()
    }

    pub fn toggle_favourite(&self) {
        let Some(entry) = self.current.borrow().clone() else {
            return;
        };
        let result = self.user.favourites().and_then(|fav| {
            if self.user.contains(fav.id, &entry.source, entry.id)? {
                self.user.remove(fav.id, &entry.source, entry.id)
            } else {
                self.user.add(fav.id, &entry, &self.list_gloss(&entry))
            }
        });
        if let Err(e) = result {
            self.toast(&format!("Cannot change Favourites: {e}"));
        }
        self.lists_changed();
    }

    /// Shows the entry with that source and number, e.g. from a Takoboto link.
    pub fn open_seq(&self, source: &str, seq: i64) {
        match self.db.get(source, seq) {
            Ok(Some(entry)) => {
                self.show_entry(entry);
                if self.split.is_collapsed() {
                    self.split.set_show_content(true);
                }
            }
            Ok(None) => self.toast(&format!("No entry {seq} in {source}")),
            Err(e) => self.toast(&format!("Cannot open entry {seq}: {e}")),
        }
    }

    /// The current entry on another site: its headword on the search or article page there.
    /// Takoboto links by JMdict number, so only those entries get that item.
    fn open_in(&self, site: &str) {
        let Some(entry) = self.current.borrow().clone() else {
            return;
        };
        let word = glib::Uri::escape_string(entry.headword(), None, false);
        let url = match site {
            "takoboto" if entry.source == "jmdict" => format!("https://takoboto.jp/?w={}", entry.id),
            "jisho" => format!("https://jisho.org/search/{word}"),
            "wadoku" => format!("https://www.wadoku.de/search/{word}"),
            "wikipedia" => format!("https://ja.wikipedia.org/wiki/{word}"),
            "wiktionary" => format!("https://ja.wiktionary.org/wiki/{word}"),
            _ => return,
        };
        self.open_url(&url);
    }

    fn open_url(&self, url: &str) {
        gtk::UriLauncher::new(url).launch(Some(&self.win), gio::Cancellable::NONE, |result| {
            if let Err(e) = result {
                log::warn!("cannot open {e}");
            }
        });
    }

    /// Sets the star and the list button to the current entry.
    fn refresh_star(&self) {
        let current = self.current.borrow().clone();
        let starred = current.as_ref().is_some_and(|e| {
            self.user
                .favourites()
                .and_then(|fav| self.user.contains(fav.id, &e.source, e.id))
                .unwrap_or(false)
        });
        self.star.set_sensitive(current.is_some());
        self.add_to_list.set_sensitive(current.is_some());
        self.open_in.set_sensitive(current.is_some());
        self.open_in.set_menu_model(Some(&open_in_menu(
            current.as_ref().is_some_and(|e| e.source == "jmdict"),
        )));
        self.star.set_active(starred);
        self.star.set_icon_name(if starred {
            "starred-symbolic"
        } else {
            "non-starred-symbolic"
        });
    }

    /// One check button per list for the current entry, plus "New list…".
    fn fill_list_popover(self: &Rc<Self>, popover: &gtk::Popover) {
        let Some(entry) = self.current.borrow().clone() else {
            return;
        };
        let column = gtk::Box::builder()
            .orientation(gtk::Orientation::Vertical)
            .spacing(6)
            .margin_top(6)
            .margin_bottom(6)
            .margin_start(12)
            .margin_end(12)
            .build();
        let lists = self.user.lists().unwrap_or_default();
        let member = self.user.lists_with(&entry.source, entry.id).unwrap_or_default();
        for list in &lists {
            let check = gtk::CheckButton::builder()
                .label(&list.name)
                .active(member.contains(&list.id))
                .build();
            let (id, entry) = (list.id, entry.clone());
            check.connect_toggled(clone!(
                #[weak(rename_to = this)]
                self,
                move |check| {
                    let result = if check.is_active() {
                        this.user.add(id, &entry, &this.list_gloss(&entry))
                    } else {
                        this.user.remove(id, &entry.source, entry.id)
                    };
                    if let Err(e) = result {
                        this.toast(&format!("Cannot change the list: {e}"));
                    }
                    this.lists_changed();
                }
            ));
            column.append(&check);
        }
        let new_list = gtk::Button::builder()
            .label("New list…")
            .css_classes(["flat"])
            .build();
        new_list.connect_clicked(clone!(
            #[weak(rename_to = this)]
            self,
            #[weak]
            popover,
            move |_| {
                popover.popdown();
                let entry = entry.clone();
                ask_name(
                    &this,
                    "New list",
                    "",
                    "Create",
                    clone!(
                        #[weak]
                        this,
                        move |name| {
                            match this.user.create_list(&name) {
                                Ok(list) => {
                                    if let Err(e) = this.user.add(list.id, &entry, &this.list_gloss(&entry)) {
                                        this.toast(&format!("Cannot add to the list: {e}"));
                                    }
                                }
                                Err(e) => this.toast(&format!("Cannot create the list: {e}")),
                            }
                            this.lists_changed();
                        }
                    ),
                );
            }
        ));
        column.append(&new_list);
        popover.set_child(Some(&column));
    }

    // -- context menu on result rows --------------------------------------------------------

    /// Right-click or long-press on a result row opens a menu for that entry: Favourites, the
    /// lists, copying, Takoboto. The actions live in a "row" group on the list and take the row
    /// index, so one menu model serves every row.
    fn install_row_menu(self: &Rc<Self>) {
        type RowAction = Box<dyn Fn(&Rc<Window>, &str)>;
        let group = gio::SimpleActionGroup::new();
        let add = |name: &str, target: bool, f: RowAction| {
            let action = gio::SimpleAction::new(name, target.then_some(glib::VariantTy::STRING));
            let weak = Rc::downgrade(self);
            action.connect_activate(move |_, param| {
                let param = param.and_then(|p| p.get::<String>()).unwrap_or_default();
                if let Some(this) = weak.upgrade() {
                    f(&this, &param);
                }
            });
            group.add_action(&action);
        };
        // Targets are "<row>" or "<row>:<argument>".
        fn split(param: &str) -> (usize, &str) {
            let (row, rest) = param.split_once(':').unwrap_or((param, ""));
            (row.parse().unwrap_or(usize::MAX), rest)
        }
        add(
            "favourite",
            true,
            Box::new(|this, p| {
                let (row, _) = split(p);
                if let Some(entry) = this.entry_at(row) {
                    let result = this.user.favourites().and_then(|fav| {
                        if this.user.contains(fav.id, &entry.source, entry.id)? {
                            this.user.remove(fav.id, &entry.source, entry.id)
                        } else {
                            this.user.add(fav.id, &entry, &this.list_gloss(&entry))
                        }
                    });
                    if let Err(e) = result {
                        this.toast(&format!("Cannot change Favourites: {e}"));
                    }
                    this.lists_changed();
                }
            }),
        );
        add(
            "list",
            true,
            Box::new(|this, p| {
                let (row, id) = split(p);
                let (Some(entry), Ok(id)) = (this.entry_at(row), id.parse::<i64>()) else {
                    return;
                };
                let result = if this.user.contains(id, &entry.source, entry.id).unwrap_or(false) {
                    this.user.remove(id, &entry.source, entry.id)
                } else {
                    this.user.add(id, &entry, &this.list_gloss(&entry))
                };
                if let Err(e) = result {
                    this.toast(&format!("Cannot change the list: {e}"));
                }
                this.lists_changed();
            }),
        );
        add(
            "new-list",
            true,
            Box::new(|this, p| {
                let (row, _) = split(p);
                let Some(entry) = this.entry_at(row) else { return };
                ask_name(
                    this,
                    "New list",
                    "",
                    "Create",
                    clone!(
                        #[weak]
                        this,
                        move |name| {
                            match this.user.create_list(&name) {
                                Ok(list) => {
                                    if let Err(e) = this.user.add(list.id, &entry, &this.list_gloss(&entry)) {
                                        this.toast(&format!("Cannot add to the list: {e}"));
                                    }
                                }
                                Err(e) => this.toast(&format!("Cannot create the list: {e}")),
                            }
                            this.lists_changed();
                        }
                    ),
                );
            }),
        );
        add(
            "copy",
            true,
            Box::new(|this, p| {
                let (row, what) = split(p);
                let Some(entry) = this.entry_at(row) else { return };
                let text = match what {
                    "reading" => entry.reading().to_string(),
                    "meaning" => this.list_gloss(&entry),
                    _ => entry.headword().to_string(),
                };
                this.win.clipboard().set_text(&text);
                this.toast(&format!("Copied {text}"));
            }),
        );
        add(
            "takoboto",
            true,
            Box::new(|this, p| {
                let (row, _) = split(p);
                if let Some(entry) = this.entry_at(row)
                    && entry.source == "jmdict"
                {
                    let url = format!("https://takoboto.jp/?w={}", entry.id);
                    gtk::UriLauncher::new(&url).launch(Some(&this.win), gio::Cancellable::NONE, |r| {
                        if let Err(e) = r {
                            log::warn!("cannot open {e}");
                        }
                    });
                }
            }),
        );
        self.results.insert_action_group("row", Some(&group));

        let open_menu = clone!(
            #[weak(rename_to = this)]
            self,
            move |x: f64, y: f64| {
                let Some(row) = this.results.row_at_y(y as i32) else {
                    return;
                };
                let index = row.index() as usize;
                let Some(model) = this.row_menu(index) else { return };
                let popover = gtk::PopoverMenu::from_model(Some(&model));
                popover.set_parent(&this.results);
                popover.set_has_arrow(false);
                popover.set_pointing_to(Some(&gdk::Rectangle::new(x as i32, y as i32, 1, 1)));
                popover.connect_closed(|p| {
                    // Unparent once closed, or the list keeps every popover ever opened.
                    let p = p.clone();
                    glib::idle_add_local_once(move || p.unparent());
                });
                popover.popup();
            }
        );
        let right_click = gtk::GestureClick::builder().button(3).build();
        right_click.connect_pressed({
            let open_menu = open_menu.clone();
            move |_, _, x, y| open_menu(x, y)
        });
        self.results.add_controller(right_click);
        let long_press = gtk::GestureLongPress::builder().touch_only(true).build();
        long_press.connect_pressed(move |_, x, y| open_menu(x, y));
        self.results.add_controller(long_press);
    }

    fn entry_at(&self, row: usize) -> Option<Entry> {
        self.found.borrow().get(row).map(|h| h.entry.clone())
    }

    /// The menu for the result row at `index`, built fresh so the lists and check marks are current.
    fn row_menu(&self, index: usize) -> Option<gio::Menu> {
        let entry = self.entry_at(index)?;
        let member = self.user.lists_with(&entry.source, entry.id).unwrap_or_default();
        let lists = self.user.lists().unwrap_or_default();
        let favourites = self.user.favourites().ok();
        let menu = gio::Menu::new();

        let keep = gio::Menu::new();
        let in_favourites = favourites.as_ref().is_some_and(|f| member.contains(&f.id));
        keep.append(
            Some(if in_favourites {
                "Remove from Favourites"
            } else {
                "Add to Favourites"
            }),
            Some(&format!("row.favourite('{index}')")),
        );
        let submenu = gio::Menu::new();
        for list in &lists {
            let label = if member.contains(&list.id) {
                format!("✓ {}", list.name)
            } else {
                list.name.clone()
            };
            submenu.append(Some(&label), Some(&format!("row.list('{index}:{}')", list.id)));
        }
        submenu.append(Some("New list…"), Some(&format!("row.new-list('{index}')")));
        keep.append_submenu(Some("Add to list"), &submenu);
        menu.append_section(None, &keep);

        let copy = gio::Menu::new();
        copy.append(
            Some("Copy headword"),
            Some(&format!("row.copy('{index}:headword')")),
        );
        if !entry.reading().is_empty() && entry.reading() != entry.headword() {
            copy.append(
                Some("Copy reading"),
                Some(&format!("row.copy('{index}:reading')")),
            );
        }
        copy.append(
            Some("Copy meaning"),
            Some(&format!("row.copy('{index}:meaning')")),
        );
        menu.append_section(None, &copy);

        if entry.source == "jmdict" {
            let open = gio::Menu::new();
            open.append(
                Some("Open in Takoboto"),
                Some(&format!("row.takoboto('{index}')")),
            );
            menu.append_section(None, &open);
        }
        Some(menu)
    }

    /// Shows the dictionary entry behind a list row, if its source is still installed.
    pub fn open_list_entry(&self, item: &ListEntry) {
        // Rows of the built-in WaniKani list: kanji open their page; a word no installed
        // dictionary has is only a name.
        if item.source == "kanji" {
            if let Some(c) = item.headword.chars().next() {
                self.show_kanji(c);
            }
            return;
        }
        if item.source.is_empty() {
            self.toast(&format!("{} is not in the installed dictionaries", item.headword));
            return;
        }
        match self.db.get(&item.source, item.seq) {
            Ok(Some(entry)) => {
                self.show_entry(entry);
                if self.split.is_collapsed() {
                    self.split.set_show_content(true);
                }
            }
            Ok(None) => self.toast(&format!("{} is not in the installed dictionaries", item.headword)),
            Err(e) => self.toast(&format!("Cannot open {}: {e}", item.headword)),
        }
    }

    /// Adds the rows of a Takoboto export to the lists they name, creating lists as needed.
    /// Returns (added, rows).
    pub fn add_takoboto_rows(&self, rows: &[TakobotoRow]) -> (usize, usize) {
        let enabled = self.config.borrow().enabled_sources();
        let mut added = 0;
        for row in rows {
            let list = match self.user.list_by_name(&row.list) {
                Ok(Some(list)) => list,
                _ => match self.user.create_list(&row.list) {
                    Ok(list) => list,
                    Err(e) => {
                        log::warn!("cannot create list {:?}: {e:#}", row.list);
                        continue;
                    }
                },
            };
            let found = self
                .db
                .lookup(std::slice::from_ref(&row.headword), &enabled, 10)
                .unwrap_or_default();
            let entry = found
                .iter()
                .find(|e| !row.reading.is_empty() && e.readings.contains(&row.reading))
                .or(found.first());
            if let Some(entry) = entry
                && self.user.add(list.id, entry, &self.list_gloss(entry)).is_ok()
            {
                added += 1;
            }
        }
        (added, rows.len())
    }

    /// Adds CSV rows (headword, reading, meaning, note, …; a header row is skipped) to a list by
    /// looking the words up; the note column is kept. Returns (added, rows).
    pub fn add_rows_to_list(&self, list_id: i64, rows: &[Vec<String>]) -> (usize, usize) {
        let enabled = self.config.borrow().enabled_sources();
        let mut added = 0;
        let mut total = 0;
        for row in rows {
            let headword = row.first().map(|s| s.trim()).unwrap_or("");
            if headword.is_empty() || (total == 0 && headword.eq_ignore_ascii_case("headword")) {
                continue;
            }
            total += 1;
            let reading = row.get(1).map(|s| s.trim()).unwrap_or("");
            let found = self
                .db
                .lookup(&[headword.to_string()], &enabled, 10)
                .unwrap_or_default();
            let entry = found
                .iter()
                .find(|e| !reading.is_empty() && e.readings.iter().any(|r| r == reading))
                .or(found.first());
            if let Some(entry) = entry
                && self.user.add(list_id, entry, &self.list_gloss(entry)).is_ok()
            {
                added += 1;
                let note = row.get(3).map(|s| s.trim()).unwrap_or("");
                if !note.is_empty()
                    && let Err(e) = self.user.set_note(list_id, &entry.source, entry.id, note)
                {
                    log::warn!("cannot keep the note for {headword}: {e:#}");
                }
            }
        }
        (added, total)
    }

    /// Autopilot: scrolls whatever the content pane shows to its top or its end.
    pub fn scroll_content(&self, to_end: bool) {
        let Some(child) = self.stack.visible_child() else {
            return;
        };
        if let Some(scrolled) = child.downcast_ref::<gtk::ScrolledWindow>() {
            let adjustment = scrolled.vadjustment();
            adjustment.set_value(if to_end {
                adjustment.upper() - adjustment.page_size()
            } else {
                0.0
            });
        }
    }

    pub fn result_count(&self) -> usize {
        self.found.borrow().len() + self.found_sentences.borrow().len()
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
        let langs = self.config.borrow().gloss_languages.clone();
        let started = std::time::Instant::now();
        let outcome = match search::run(
            &self.db,
            query,
            RESULT_LIMIT,
            &enabled,
            &tatoeba::languages(&langs),
            &self.learned.borrow(),
        ) {
            Ok(outcome) => outcome,
            Err(e) => {
                log::error!("search failed: {e:#}");
                search::Outcome::default()
            }
        };
        log::debug!(
            "search {query:?}: {} hits, {} sentences in {} ms",
            outcome.hits.len(),
            outcome.sentences.len(),
            started.elapsed().as_millis()
        );
        self.no_results.set_description(outcome.hint.as_deref());
        // Toggling the button's visibility inside the placeholder made the whole page vanish
        // after a search, so it stays and is merely insensitive without a query.
        self.search_online.set_sensitive(!query.trim().is_empty());
        self.update_kanji_hint(query);
        let any = !outcome.hits.is_empty() || !outcome.sentences.is_empty();
        super::clear_rows(&self.results);
        *self.found.borrow_mut() = outcome.hits;
        *self.found_sentences.borrow_mut() = outcome.sentences;
        for sentence in self.found_sentences.borrow().iter() {
            self.results.append(&sentence_row(sentence));
        }
        for hit in self.found.borrow().iter() {
            let listed = self
                .user
                .lists_with(&hit.entry.source, hit.entry.id)
                .map(|l| !l.is_empty())
                .unwrap_or(false);
            self.results.append(&result_row(hit, &langs, listed));
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
        let sentence = self.found_sentences.borrow().get(index as usize).cloned();
        let entry = self.found.borrow().get(index as usize).map(|h| h.entry.clone());
        if let Some(sentence) = sentence {
            self.show_sentence(sentence);
        } else if let Some(entry) = entry {
            self.show_entry(entry);
        } else {
            return;
        }
        if self.split.is_collapsed() {
            self.split.set_show_content(true);
        }
    }

    pub fn show_entry(&self, entry: Entry) {
        self.remember();
        self.example_limit.set(EXAMPLES_FIRST);
        self.render_entry(&entry);
        *self.view.borrow_mut() = Some(View::Entry(entry.clone()));
        *self.current.borrow_mut() = Some(entry);
        self.stack.set_visible_child_name("entry");
        self.refresh_back();
        self.refresh_star();
    }

    // -- history ----------------------------------------------------------------------------

    /// Before another view is shown: the current one goes onto the history, unless this is the
    /// back button re-showing an older one.
    fn remember(&self) {
        if self.going_back.get() {
            return;
        }
        if let Some(view) = self.view.borrow().clone() {
            let mut history = self.history.borrow_mut();
            if history.last() != Some(&view) {
                history.push(view);
                if history.len() > HISTORY_LIMIT {
                    history.remove(0);
                }
            }
        }
    }

    fn refresh_back(&self) {
        self.back.set_visible(!self.history.borrow().is_empty());
    }

    /// The view before this one: the entry a see-also link came from, the sentence a word was
    /// opened from, the kanji page behind a "Words with this kanji" row.
    pub fn go_back(&self) {
        let Some(view) = self.history.borrow_mut().pop() else {
            return;
        };
        self.going_back.set(true);
        match view {
            View::Entry(entry) => self.show_entry(entry),
            View::Kanji(c) => self.show_kanji(c),
            View::Sentence(s) => self.show_sentence(s),
        }
        self.going_back.set(false);
        self.refresh_back();
    }

    /// After the dictionaries changed: the views in the history may not exist any more.
    fn forget_history(&self) {
        self.history.borrow_mut().clear();
        *self.view.borrow_mut() = None;
        self.refresh_back();
    }

    /// A "See also" / "Antonym" reference, "猫・ねこ・1": the entry for the form, with that
    /// reading when one is given; a search for the form when it is not installed.
    fn open_reference(&self, reference: &str) {
        let parts: Vec<&str> = reference
            .split('・')
            .filter(|p| !p.is_empty() && !p.chars().all(|c| c.is_ascii_digit()))
            .collect();
        let Some(&form) = parts.first() else { return };
        let reading = parts.get(1).copied();
        let enabled = self.config.borrow().enabled_sources();
        let found = self
            .db
            .lookup(&[form.to_string()], &enabled, 10)
            .unwrap_or_default();
        let entry = found
            .iter()
            .find(|e| reading.is_none_or(|r| e.readings.iter().any(|x| x == r)))
            .or(found.first())
            .cloned();
        match entry {
            Some(entry) => self.show_entry(entry),
            None => self.search.set_text(form),
        }
    }

    /// Fills the entry view: the entry, and its example sentences when Tatoeba is enabled.
    fn render_entry(&self, entry: &Entry) {
        let langs = self.config.borrow().gloss_languages.clone();
        let enabled = self.config.borrow().enabled_sources();
        let (examples, total) = if enabled.iter().any(|s| s == "tatoeba") {
            self.db
                .examples(entry, &tatoeba::languages(&langs), self.example_limit.get())
                .unwrap_or_else(|e| {
                    log::error!("examples for {}: {e:#}", entry.headword());
                    (Vec::new(), 0)
                })
        } else {
            (Vec::new(), 0)
        };
        let learned = if self.config.borrow().show_wanikani {
            let mut texts: Vec<String> = entry.kanji.iter().chain(&entry.readings).cloned().collect();
            texts.extend(
                entry
                    .headword()
                    .chars()
                    .filter(|c| super::entry_view::is_kanji(*c))
                    .map(|c| c.to_string()),
            );
            self.user.learned_for(&texts).unwrap_or_default()
        } else {
            Vec::new()
        };
        self.entry_view.show(entry, &langs, &examples, total, &learned);
    }

    /// "Show all": the entry again with every example, scrolled to where it was.
    fn more_examples(&self) {
        let Some(entry) = self.current.borrow().clone() else {
            return;
        };
        self.example_limit.set(EXAMPLES_ALL);
        let adjustment = self.entry_view.widget().vadjustment();
        let position = adjustment.value();
        self.render_entry(&entry);
        // The new height is known after the next layout pass, so the scroll position is restored
        // from an idle callback, which runs after it.
        glib::idle_add_local_once(move || adjustment.set_value(position));
    }

    // -- sentences --------------------------------------------------------------------------

    /// The page for a sentence from a `#sentences` search.
    fn show_sentence(&self, sentence: Sentence) {
        let words = self.db.sentence_words(sentence.id).unwrap_or_else(|e| {
            log::error!("words of sentence {}: {e:#}", sentence.id);
            Vec::new()
        });
        self.remember();
        self.sentence_view.show(&sentence, &words);
        *self.view.borrow_mut() = Some(View::Sentence(sentence));
        *self.current.borrow_mut() = None;
        self.stack.set_visible_child_name("sentence");
        self.refresh_back();
        self.refresh_star();
    }

    /// A word button on the sentence page: the entry the index names (by JMdict number, else by
    /// headword and reading), or a search for the headword when it is not installed.
    fn open_word(&self, word: &SentenceWord) {
        let enabled = self.config.borrow().enabled_sources();
        let mut found: Vec<Entry> = word
            .seq
            .and_then(|seq| self.db.get("jmdict", seq).ok().flatten())
            .into_iter()
            .collect();
        if found.is_empty() {
            found = self
                .db
                .lookup(std::slice::from_ref(&word.headword), &enabled, 10)
                .unwrap_or_default();
        }
        let entry = found
            .iter()
            .find(|e| word.reading.as_ref().is_none_or(|r| e.readings.contains(r)))
            .or(found.first())
            .cloned();
        match entry {
            Some(entry) => self.show_entry(entry),
            None => self.search.set_text(&word.headword),
        }
    }

    // -- kanji ------------------------------------------------------------------------------

    /// The kanji page for `literal`, with whatever kanji data is installed.
    pub fn show_kanji(&self, literal: char) {
        self.remember();
        let kanji = self.db.kanji(literal).unwrap_or_else(|e| {
            log::error!("kanji {literal}: {e:#}");
            None
        });
        let strokes = self.db.strokes(literal).unwrap_or_default();
        let radicals = self.db.radicals_of(literal).unwrap_or_default();
        let enabled = self.config.borrow().enabled_sources();
        let words = self.db.words_with(literal, 40, &enabled).unwrap_or_default();
        let langs = self.config.borrow().gloss_languages.clone();
        let learned = if self.config.borrow().show_wanikani {
            self.user
                .learned_for(&[literal.to_string()])
                .unwrap_or_default()
                .into_iter()
                .find(|l| l.kind == Kind::Kanji)
        } else {
            None
        };
        self.kanji_view.show(
            literal,
            kanji.as_ref(),
            strokes.as_deref(),
            &radicals,
            words,
            &langs,
            learned.as_ref(),
        );
        *self.view.borrow_mut() = Some(View::Kanji(literal));
        self.stack.set_visible_child_name("kanji");
        self.refresh_back();
        if self.split.is_collapsed() {
            self.split.set_show_content(true);
        }
    }

    /// Offers the kanji page when the query is a single kanji with data behind it.
    fn update_kanji_hint(&self, query: &str) {
        let mut chars = query.trim().chars();
        let single = match (chars.next(), chars.next()) {
            (Some(c), None) if super::entry_view::is_kanji(c) => Some(c),
            _ => None,
        };
        let known = single.is_some_and(|c| {
            self.db.kanji(c).ok().flatten().is_some() || self.db.strokes(c).ok().flatten().is_some()
        });
        self.kanji_hint.set_visible(known);
        if let Some(c) = single.filter(|_| known) {
            self.kanji_hint.set_label(&format!("Kanji {c}"));
        }
    }

    /// Re-renders the current entry, e.g. after the gloss language order changed.
    pub fn rerender(&self) {
        let current = self.current.borrow().clone();
        if let Some(entry) = current {
            self.show_entry(entry);
        }
    }

    // -- import -----------------------------------------------------------------------------

    /// Queues a download of `source` into the cache and its import, replacing what the database
    /// had of it. `after` runs on the main thread once the job is done, success or not.
    pub fn download_source(self: &Rc<Self>, source: &'static Source, after: impl FnOnce() + 'static) {
        let path = self.db_path.clone();
        let cache = cache_dir();
        self.run_job(
            source.id,
            format!("Downloading {}", source.name),
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

    /// Imports a local JMdict file: the menu action and the cached copy.
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
            source.id,
            format!("Importing {}", source.name),
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
            source.id,
            format!("Removing {}", source.name),
            move |report| {
                let db = Database::open(&path)?;
                import::remove(&db, source, &cache, report)?;
                Ok(format!("Removed {}", source.name))
            },
            after,
        );
    }

    /// Queues a job for the worker thread; its `Ok` text becomes a toast when it is done. Then
    /// the window state and the search are refreshed and `after` runs.
    fn run_job(
        self: &Rc<Self>,
        id: &'static str,
        title: String,
        job: impl FnOnce(import::Report) -> anyhow::Result<String> + Send + 'static,
        after: impl FnOnce() + 'static,
    ) {
        let this = Rc::downgrade(self);
        // A dictionary job replaces entries, so the shown one is dropped; an account job leaves
        // the entry where it is and only its chips change.
        let dictionary_job = sources::by_id(id).is_some();
        let done: jobs::DoneFn = Box::new(move |result| {
            let Some(this) = this.upgrade() else { return };
            match result {
                Ok(message) => this.toasts.add_toast(adw::Toast::new(&message)),
                Err(e) => {
                    let toast = adw::Toast::new(&format!("Failed: {e}"));
                    toast.set_timeout(0);
                    this.toasts.add_toast(toast);
                }
            }
            if dictionary_job {
                *this.current.borrow_mut() = None;
                this.forget_history();
                this.refresh_state();
                this.radicals.refresh();
            }
            this.refresh_star();
            this.refresh_search();
            after();
        });
        self.jobs.enqueue(id, title, Box::new(job), done);
    }

    // -- accounts ---------------------------------------------------------------------------

    /// What the user database remembers about the WaniKani account, if one is connected.
    pub fn wanikani_status(&self) -> Option<accounts::Status> {
        let get = |key: &str| self.user.sync_state(wanikani::PROVIDER, key).ok().flatten();
        let username = get("username")?;
        Some(accounts::Status {
            username,
            level: get("level").and_then(|l| l.parse().ok()).unwrap_or(0),
            last_sync: get("last_sync"),
            items: self.user.learned_count(wanikani::PROVIDER).unwrap_or(0),
        })
    }

    /// Checks a WaniKani token, keeps it in the keyring, remembers who it belongs to, and
    /// queues the first sync.
    pub fn connect_wanikani(self: &Rc<Self>, token: String, after: impl FnOnce() + 'static) {
        let user_path = user_database_path();
        let this = Rc::downgrade(self);
        self.run_job(
            "wanikani",
            "Connecting WaniKani".into(),
            move |report| {
                report("Checking the token…".into(), None);
                let account = wanikani::user(&token)?;
                secrets::store(wanikani::PROVIDER, &token)?;
                let db = UserDb::open(&user_path)?;
                db.set_sync_state(wanikani::PROVIDER, "username", &account.username)?;
                db.set_sync_state(wanikani::PROVIDER, "level", &account.level.to_string())?;
                Ok(format!(
                    "Connected to WaniKani as {} (level {})",
                    account.username, account.level
                ))
            },
            move || match this.upgrade() {
                Some(this) => this.sync_wanikani(after),
                None => after(),
            },
        );
    }

    /// Queues an incremental sync with the stored token.
    pub fn sync_wanikani(self: &Rc<Self>, after: impl FnOnce() + 'static) {
        let user_path = user_database_path();
        let this = Rc::downgrade(self);
        self.run_job(
            "wanikani",
            "Syncing WaniKani".into(),
            move |report| {
                let token = secrets::lookup(wanikani::PROVIDER)?
                    .ok_or_else(|| anyhow::anyhow!("no WaniKani token; connect the account first"))?;
                let db = UserDb::open(&user_path)?;
                let stats = wanikani::sync(&token, &db, report)?;
                db.set_sync_state(wanikani::PROVIDER, "last_sync", &now_iso8601())?;
                Ok(format!(
                    "WaniKani: {} items synced",
                    thousands(stats.learned as i64)
                ))
            },
            move || {
                if let Some(this) = this.upgrade() {
                    this.refresh_learned();
                }
                after();
            },
        );
    }

    /// Forgets the token and everything synced from WaniKani.
    pub fn disconnect_wanikani(self: &Rc<Self>, after: impl FnOnce() + 'static) {
        let user_path = user_database_path();
        let this = Rc::downgrade(self);
        self.run_job(
            "wanikani",
            "Disconnecting WaniKani".into(),
            move |_report| {
                if let Err(e) = secrets::clear(wanikani::PROVIDER) {
                    log::warn!("{e:#}");
                }
                UserDb::open(&user_path)?.clear_provider(wanikani::PROVIDER)?;
                Ok("WaniKani disconnected".into())
            },
            move || {
                if let Some(this) = this.upgrade() {
                    this.refresh_learned();
                }
                after();
            },
        );
    }

    /// After a sync: the filters' index and the shown entry follow the new data.
    pub fn refresh_learned(&self) {
        *self.learned.borrow_mut() = self.user.learned_index().unwrap_or_else(|e| {
            log::error!("cannot read the learned items: {e:#}");
            LearnedIndex::new()
        });
        self.rerender();
    }

    pub fn choose_import_file(self: &Rc<Self>, source: &'static Source, after: impl FnOnce() + 'static) {
        let filter = gtk::FileFilter::new();
        filter.set_name(Some(&format!("{} file", source.name)));
        filter.add_pattern("*.xml");
        filter.add_pattern("*.gz");
        filter.add_pattern("*.bz2");
        filter.add_pattern("*.xz");
        filter.add_pattern("*.zip");
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

/// The sites an entry can be opened on; Takoboto only for JMdict entries (it links by number).
fn open_in_menu(jmdict: bool) -> gio::Menu {
    let menu = gio::Menu::new();
    menu.append(Some("Jisho"), Some("win.open-in::jisho"));
    menu.append(Some("Wadoku"), Some("win.open-in::wadoku"));
    menu.append(Some("Wikipedia (Japanese)"), Some("win.open-in::wikipedia"));
    menu.append(Some("Wiktionary (Japanese)"), Some("win.open-in::wiktionary"));
    if jmdict {
        menu.append(Some("Takoboto"), Some("win.open-in::takoboto"));
    }
    menu
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

/// A row of a `#sentences` search: the Japanese, and the first translation under it.
fn sentence_row(sentence: &Sentence) -> adw::ActionRow {
    let row = adw::ActionRow::builder()
        .activatable(true)
        .title(glib::markup_escape_text(&sentence.text))
        .title_lines(2)
        .subtitle_lines(1)
        .build();
    if let Some((_, text)) = sentence.translations.first() {
        row.set_subtitle(&glib::markup_escape_text(text));
    }
    row
}

fn result_row(hit: &Hit, langs: &[String], listed: bool) -> adw::ActionRow {
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
    if let Some(level) = entry.jlpt {
        let tag = gtk::Label::builder()
            .label(format!("N{level}"))
            .valign(gtk::Align::Center)
            .tooltip_text(format!("JLPT N{level} (unofficial lists)"))
            .css_classes(["tango-jlpt"])
            .build();
        row.add_suffix(&tag);
    }
    if entry.source != "jmdict"
        && let Some(source) = sources::by_id(&entry.source)
    {
        let tag = gtk::Label::builder()
            .label(source.name)
            .valign(gtk::Align::Center)
            .css_classes(["tango-lang"])
            .build();
        row.add_suffix(&tag);
    }
    if listed {
        let star = gtk::Image::builder()
            .icon_name("starred-symbolic")
            .tooltip_text("In a word list")
            .css_classes(["dim-label"])
            .build();
        row.add_suffix(&star);
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
