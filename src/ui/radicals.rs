//! Search kanji by their parts: pick radicals, optionally a stroke count, get a grid of kanji.
//! Radicals that no remaining kanji contains are greyed out, as on Jisho, or hidden altogether
//! (a toggle, remembered in the config) so the picker stays short.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::{Rc, Weak};

use adw::prelude::*;
use gtk::glib::{self, clone};

use super::window::Window;

/// More than this many kanji in the grid is noise; the stroke filter narrows it down.
const MAX_KANJI: usize = 300;

pub struct RadicalsPage {
    pub widget: gtk::Box,
    radicals_box: gtk::Box,
    results: gtk::FlowBox,
    summary: gtk::Label,
    strokes: gtk::DropDown,
    /// Hide the parts that cannot follow instead of greying them out.
    hide: gtk::ToggleButton,
    /// The stroke-count caption and grid pairs, hidden together when nothing in them is shown.
    groups: RefCell<Vec<(gtk::Label, gtk::FlowBox)>>,
    empty: adw::StatusPage,
    stack: gtk::Stack,
    win: RefCell<Weak<Window>>,
    selected: RefCell<Vec<String>>,
    buttons: RefCell<HashMap<String, gtk::ToggleButton>>,
    /// Set while the buttons are being programmatically toggled.
    quiet: Cell<bool>,
}

impl RadicalsPage {
    pub fn new() -> Rc<Self> {
        let radicals_box = gtk::Box::new(gtk::Orientation::Vertical, 6);
        let strokes_model = gtk::StringList::new(&["Any strokes"]);
        for n in 1..=30 {
            strokes_model.append(&format!("{n} strokes"));
        }
        let strokes = gtk::DropDown::builder().model(&strokes_model).build();
        let clear = gtk::Button::builder()
            .label("Clear")
            .css_classes(["flat"])
            .build();
        let hide = gtk::ToggleButton::builder()
            .icon_name("view-conceal-symbolic")
            .tooltip_text("Hide parts that no longer fit, instead of greying them out")
            .css_classes(["flat"])
            .halign(gtk::Align::End)
            .hexpand(true)
            .build();
        // 12 px page margins (GNOME HIG); the grids' flat cells add their own 2 px.
        let controls = gtk::Box::builder()
            .orientation(gtk::Orientation::Horizontal)
            .spacing(6)
            .margin_start(12)
            .margin_end(12)
            .margin_top(12)
            .build();
        controls.append(&strokes);
        controls.append(&clear);
        controls.append(&hide);
        let summary = gtk::Label::builder()
            .xalign(0.0)
            .margin_start(12)
            .margin_end(12)
            .wrap(true)
            .css_classes(["dim-label", "caption"])
            .build();
        let results = gtk::FlowBox::builder()
            .selection_mode(gtk::SelectionMode::None)
            .homogeneous(true)
            .min_children_per_line(6)
            .max_children_per_line(20)
            .row_spacing(2)
            .column_spacing(2)
            .margin_start(12)
            .margin_end(12)
            .build();
        let column = gtk::Box::new(gtk::Orientation::Vertical, 6);
        column.append(&controls);
        column.append(&summary);
        column.append(&results);
        column.append(&gtk::Separator::new(gtk::Orientation::Horizontal));
        column.append(&radicals_box);
        let scroller = gtk::ScrolledWindow::builder()
            .child(&column)
            .vexpand(true)
            .hscrollbar_policy(gtk::PolicyType::Never)
            .build();
        let empty = adw::StatusPage::builder()
            .title("No radical data")
            .description("Install Radicals (RADKFILE) from the Dictionaries page in Preferences.")
            .icon_name("accessories-character-map-symbolic")
            .build();
        let stack = gtk::Stack::new();
        stack.add_named(&empty, Some("empty"));
        stack.add_named(&scroller, Some("page"));
        let widget = gtk::Box::new(gtk::Orientation::Vertical, 0);
        widget.append(&stack);
        let this = Rc::new(Self {
            widget,
            radicals_box,
            results,
            summary,
            strokes,
            empty,
            stack,
            hide,
            groups: RefCell::new(Vec::new()),
            win: RefCell::new(Weak::new()),
            selected: RefCell::new(Vec::new()),
            buttons: RefCell::new(HashMap::new()),
            quiet: Cell::new(false),
        });
        this.strokes.connect_selected_notify(clone!(
            #[weak]
            this,
            move |_| this.update()
        ));
        clear.connect_clicked(clone!(
            #[weak]
            this,
            move |_| this.clear()
        ));
        this.hide.connect_toggled(clone!(
            #[weak]
            this,
            move |button| {
                if this.quiet.get() {
                    return;
                }
                if let Some(win) = this.window() {
                    let mut cfg = win.config().borrow_mut();
                    cfg.hide_unusable_radicals = button.is_active();
                    cfg.save();
                }
                this.update();
            }
        ));
        this
    }

    pub fn attach(self: &Rc<Self>, win: &Rc<Window>) {
        *self.win.borrow_mut() = Rc::downgrade(win);
        self.quiet.set(true);
        self.hide.set_active(win.config().borrow().hide_unusable_radicals);
        self.quiet.set(false);
        self.refresh();
    }

    /// Flips the hide toggle (autopilot).
    pub fn set_hide(&self, on: bool) {
        self.hide.set_active(on);
    }

    fn window(&self) -> Option<Rc<Window>> {
        self.win.borrow().upgrade()
    }

    /// Rebuilds the radical grid from the database (after an import or a removal).
    pub fn refresh(self: &Rc<Self>) {
        let Some(win) = self.window() else { return };
        let radicals = win.db().radicals().unwrap_or_default();
        while let Some(child) = self.radicals_box.first_child() {
            self.radicals_box.remove(&child);
        }
        self.buttons.borrow_mut().clear();
        self.selected.borrow_mut().clear();
        self.groups.borrow_mut().clear();
        if radicals.is_empty() {
            self.stack.set_visible_child(&self.empty);
            return;
        }
        self.stack.set_visible_child_name("page");
        let mut current_strokes = 0;
        let mut flow: Option<gtk::FlowBox> = None;
        for (radical, strokes, _count) in radicals {
            if flow.is_none() || strokes != current_strokes {
                current_strokes = strokes;
                let header = gtk::Label::builder()
                    .label(format!("{strokes}"))
                    .xalign(0.0)
                    .margin_start(12)
                    .tooltip_text("Radicals with this many strokes")
                    .css_classes(["dim-label", "caption"])
                    .build();
                self.radicals_box.append(&header);
                let f = gtk::FlowBox::builder()
                    .selection_mode(gtk::SelectionMode::None)
                    .homogeneous(true)
                    .min_children_per_line(8)
                    .max_children_per_line(20)
                    .row_spacing(2)
                    .column_spacing(2)
                    .margin_start(12)
                    .margin_end(12)
                    .build();
                self.radicals_box.append(&f);
                self.groups.borrow_mut().push((header, f.clone()));
                flow = Some(f);
            }
            let button = gtk::ToggleButton::builder()
                .label(&radical)
                .css_classes(["flat", "tango-radical"])
                .build();
            let name = radical.clone();
            button.connect_toggled(clone!(
                #[weak(rename_to = this)]
                self,
                move |b| {
                    if this.quiet.get() {
                        return;
                    }
                    let mut selected = this.selected.borrow_mut();
                    selected.retain(|r| *r != name);
                    if b.is_active() {
                        selected.push(name.clone());
                    }
                    drop(selected);
                    this.update();
                }
            ));
            if let Some(f) = &flow {
                f.append(&button);
            }
            self.buttons.borrow_mut().insert(radical, button);
        }
        self.update();
    }

    /// Toggles a radical as if clicked (autopilot).
    pub fn toggle(&self, radical: &str) {
        if let Some(b) = self.buttons.borrow().get(radical) {
            b.set_active(!b.is_active());
        }
    }

    fn clear(&self) {
        self.quiet.set(true);
        for b in self.buttons.borrow().values() {
            b.set_active(false);
        }
        self.quiet.set(false);
        self.selected.borrow_mut().clear();
        self.strokes.set_selected(0);
        self.update();
    }

    /// Queries the kanji for the selection and greys out radicals that cannot follow.
    fn update(&self) {
        let Some(win) = self.window() else { return };
        let selected = self.selected.borrow().clone();
        let strokes = match self.strokes.selected() {
            0 => None,
            n => Some(n as u8),
        };
        while let Some(child) = self.results.first_child() {
            self.results.remove(&child);
        }
        if selected.is_empty() {
            self.summary
                .set_text("Pick the parts of the kanji you are looking for.");
            for b in self.buttons.borrow().values() {
                b.set_sensitive(true);
                show_cell(b, true);
            }
            self.show_groups();
            return;
        }
        let kanji = win
            .db()
            .kanji_with_radicals(&selected, strokes)
            .unwrap_or_default();
        let compatible = win.db().radicals_compatible(&selected).unwrap_or_default();
        let hide = self.hide.is_active();
        log::debug!(
            "radicals: {} selected, {} kanji, {} parts can follow, hide={hide}",
            selected.len(),
            kanji.len(),
            compatible.len()
        );
        for (name, b) in self.buttons.borrow().iter() {
            let usable = b.is_active() || compatible.contains(name);
            b.set_sensitive(usable);
            show_cell(b, !hide || usable);
        }
        self.show_groups();
        let shown = kanji.len().min(MAX_KANJI);
        self.summary.set_text(&if kanji.len() > MAX_KANJI {
            format!(
                "{} kanji, showing {shown}. Add a part or a stroke count.",
                kanji.len()
            )
        } else {
            format!("{} kanji", kanji.len())
        });
        for c in kanji.into_iter().take(shown) {
            let button = gtk::Button::builder()
                .label(c.to_string())
                .css_classes(["flat", "tango-kanji-cell"])
                .tooltip_text("Show this kanji")
                .build();
            button.connect_clicked(clone!(
                #[weak]
                win,
                move |_| win.show_kanji(c)
            ));
            self.results.append(&button);
        }
    }

    /// A stroke-count caption and its grid go when every part in it is hidden.
    fn show_groups(&self) {
        for (header, flow) in self.groups.borrow().iter() {
            let mut child = flow.first_child();
            let mut any = false;
            while let Some(c) = child {
                if c.get_visible() {
                    any = true;
                    break;
                }
                child = c.next_sibling();
            }
            header.set_visible(any);
            flow.set_visible(any);
        }
    }
}

/// Shows or hides a grid cell: the button sits inside a `FlowBoxChild`, which is what takes the
/// space, so that is what is hidden.
fn show_cell(button: &gtk::ToggleButton, visible: bool) {
    match button.parent() {
        Some(child) => child.set_visible(visible),
        None => button.set_visible(visible),
    }
}
