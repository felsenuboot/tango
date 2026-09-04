# Development notes

```
cargo test                                   # unit tests, no display needed
cargo clippy --all-targets -- -D warnings    # what CI runs
cargo fmt
cargo run                                    # dev build against the real config and database
```

## Where things live

| Path | What |
| --- | --- |
| `src/main.rs` | entry point, application id, log setup |
| `src/model.rs` | `Entry`, `Sense`, `Gloss`: the plain data everything else passes around |
| `src/dict/jmdict.rs` | streaming JMdict XML reader, expands the DTD entities |
| `src/dict/sources.rs` | the source registry (id, name, URL, file, licence) and the download helper |
| `src/store/db.rs` | SQLite schema (v2: `sources`, entries per source), insert, load, search |
| `src/store/import.rs` | the import, download and remove jobs the UI runs on a worker thread |
| `src/ui/mod.rs` | app startup, actions, CSS, the one main window |
| `src/ui/window.rs` | search entry, result list, split view, import flow |
| `src/ui/entry_view.rs` | renders one entry |
| `src/ui/import_dialog.rs` | progress dialog + worker thread + channel |
| `src/ui/preferences.rs` | preferences dialog: General, and Dictionaries (installed sources) |
| `src/ui/theme.rs` | colour scheme: follow the system, or force light / dark above the user's GTK CSS |
| `src/config.rs` | JSON config in `~/.config/tango`, XDG paths |
| `src/autopilot.rs` | scripted UI driving (below) |
| `tests/fixtures/` | a six-entry JMdict sample the unit tests use |

## The two kinds of data

The dictionary database (`~/.local/share/tango/tango.sqlite`) is derived data:
every row can be imported again from the downloads in `~/.cache/tango`. So a
schema version bump (`SCHEMA_VERSION` in `src/store/db.rs`) does not migrate
anything; `Database::open` drops the tables and the app shows the empty state
with an "Import the downloaded copy" button. User data (word lists, issue #10)
will live in its own file with real migrations, precisely so the dictionary
file can stay disposable.

Each source in `src/dict/sources.rs` is one row in `sources` (version as the
file states it, import time, entry count) and owns its rows in `entries`
through the `source` column; `(source, seq)` is unique, `seq` being the
source's own number (JMdict `ent_seq`). Search takes the enabled sources in the
order from the config file and ranks exact matches, then common words, then
source order, then length.

## JMdict and its languages

JMdict keeps every language in senses of its own: all the English senses
first, then a block per language (German, Russian, Hungarian, Dutch, Spanish,
French, Swedish, Slovenian), and the project makes "no attempt to align senses
between the languages". Measured on the 2026-09-04 file: no sense mixes
languages, the non-English senses always follow the English ones and never
carry a part of speech, and for German the sense count equals the English
count in 82% of the entries, is one in 6% and larger in 11%.

`Entry::grouped` in `src/model.rs` therefore lines a language up with the
English senses by position when the counts are equal (right in nearly every
sampled case, wrong now and then, e.g. マス目) and otherwise lists it as its
own block after the numbered meanings. A lone German sense is *not* attached
to meaning 1: in the samples it is sometimes meaning 1, sometimes meaning 2,
sometimes two meanings joined.

## Environment variables

| Variable | Purpose |
| --- | --- |
| `TANGO_DB` | use this SQLite file instead of `~/.local/share/tango/tango.sqlite` |
| `TANGO_AUTOPILOT` | script to run after start-up (see below) |
| `TANGO_DEBUG=1` | debug logging (`RUST_LOG` works too) |
| `XDG_CONFIG_HOME` etc. | point at a scratch directory for a fresh instance |

## Scripted UI and screenshots

`TANGO_AUTOPILOT="sleep 2; search 猫; sleep 1; select 0"` drives the UI from a
script; the commands are documented at the top of `src/autopilot.rs`. With
the variable set the app registers a non-unique GApplication, so it does not
join a running desktop instance. Screenshots are taken that way inside a
headless `cage` compositor:

```
export TANGO_DB=/tmp/tango-test/tango.sqlite XDG_CONFIG_HOME=/tmp/tango-test/config
TANGO_AUTOPILOT="sleep 2; import $HOME/.cache/tango/JMdict.gz; sleep 40; search 猫; sleep 2; select 0" \
WLR_BACKENDS=headless WLR_RENDERER=pixman WLR_LIBINPUT_NO_DEVICES=1 \
  cage -- sh -c './target/release/tango & sleep 49; grim shot.png; kill %1'
```

`XDG_CONFIG_HOME` is redirected on purpose: GTK loads `~/.config/gtk-4.0/gtk.css`
above every application style provider, and on a desktop that generates that file
(Hyprland with Matugen, say) "Follow system" repaints the screenshots in the
desktop's palette. The Light and Dark settings beat that file by re-declaring
libadwaita's named colours one priority above it, see `src/ui/theme.rs`. The
`theme light|dark|system` autopilot step switches for one run without saving.

## Hyprland and popups

Hyprland (0.56, and master as of 2026-09) keeps a window's popups out of the
strip a top bar reserves even when the window is fullscreen and covers the bar,
and it shrinks the popup by the overlap instead of sliding it down. The primary
menu then shows a scrollbar. `keep_menu_out_of_reserved_strip` in
`src/ui/window.rs` works around it; reproduce with the autopilot steps
`fullscreen; menu` and read the `autopilot menu:` line, which logs the
popover's content height against its page size.

## Reading the code as a Rust newcomer

Suggested order: `model.rs` (structs, `impl` blocks, `&str` vs `String`),
`config.rs` (serde, `Result` and `?`, `match` on errors), `dict/jmdict.rs`
(a state machine over a streaming parser, `Option` handling, `let else`),
`store/db.rs` (rusqlite, transactions, building `Vec`s), then the UI.

Idioms you will meet in the UI:

- **Builders.** `gtk::Label::builder().label("x").xalign(0.0).build()` is how
  gtk-rs sets construct properties. Setters after construction are `set_*`.
- **`Rc<RefCell<T>>`.** GTK is single-threaded, so shared state is reference
  counted (`Rc`) and mutated through a runtime-checked borrow (`RefCell`).
  Keep borrows short: a `borrow_mut()` held across a call that borrows again
  panics.
- **`clone!(#[weak] this, move |…| …)`.** Callbacks capture a *weak* reference
  to the window struct. A strong `Rc` inside a closure owned by the window
  would keep it alive forever. `#[upgrade_or]` gives the return value when the
  window is already gone.
- **Worker threads talk through channels.** `import_dialog.rs` spawns a thread,
  which sends `Progress` messages over `async_channel`; a future on the GLib
  main loop applies them to widgets. Widgets never leave the main thread.
- **Traits must be in scope.** Most widget methods come from extension traits,
  hence `use adw::prelude::*;` in every UI file (it re-exports GTK's prelude).
- **`anyhow::Result`** everywhere errors can happen; `.with_context(|| …)`
  attaches a message; `{e:#}` prints the whole chain.
