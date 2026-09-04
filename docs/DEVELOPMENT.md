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
| `src/dict/sources.rs` | download URLs and the download helper |
| `src/store/db.rs` | SQLite schema, insert, load, search |
| `src/store/import.rs` | the import jobs the UI runs on a worker thread |
| `src/ui/mod.rs` | app startup, actions, CSS, the one main window |
| `src/ui/window.rs` | search entry, result list, split view, import flow |
| `src/ui/entry_view.rs` | renders one entry |
| `src/ui/import_dialog.rs` | progress dialog + worker thread + channel |
| `src/ui/preferences.rs` | preferences dialog |
| `src/ui/theme.rs` | colour scheme: follow the system, or force light / dark above the user's GTK CSS |
| `src/config.rs` | JSON config in `~/.config/tango`, XDG paths |
| `src/autopilot.rs` | scripted UI driving (below) |
| `tests/fixtures/` | a six-entry JMdict sample the unit tests use |

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
