# Development notes

```
cargo test                                   # unit tests, no display needed
cargo clippy --all-targets -- -D warnings    # what CI runs
cargo fmt
cargo run                                    # dev build against the real config and database
```

## Working on an issue

Master is always releasable; nothing lands on it without a pull request and a
green CI. That is a convention, not a rule GitHub enforces: rulesets and
branch protection need GitHub Pro on a private repository. Tags are pushed
directly.

1. Every change starts from an issue. No issue yet? Open one, even a one-liner,
   and put it in a milestone.
2. Branch from master, named `<issue>-<slug>`, e.g. `7-romaji-deinflection`.
3. Commit subjects name the area and the issue: `search: romaji input and
   deinflection (#7)`. Bodies say why. The `Claude-Session` trailer stays.
4. Open the pull request with `Closes #<issue>` in the body and the issue's
   milestone. Squash-merge it with the subject from step 3, so master carries
   one commit per issue and every commit traces to an issue and a milestone.
5. Sessions live in `docs/sessions/`; screenshots that document a change go
   into the pull request.

## Releasing

Milestones are minor versions: `0.3 Search` ships as `v0.3.0`. When the last
issue of a milestone closes:

```
# on a branch, like any change
sed -i 's/^version = ".*"/version = "0.3.0"/' Cargo.toml && cargo check
git commit -am "Release 0.3.0"           # pull request, squash-merge
git tag -a v0.3.0 -m "Tango 0.3.0" <merge commit> && git push origin v0.3.0
gh release create v0.3.0 --generate-notes --notes-start-tag v0.2.0 --title "Tango 0.3.0"
```

Then edit the generated notes to start with two or three sentences of what
the release means to a user, and close the milestone. Fixes between milestones
go out as patch releases (`v0.3.1`). The Arch package takes its version from
`Cargo.toml` plus the commit count, so the tag is not needed for it.

## Where things live

| Path | What |
| --- | --- |
| `src/main.rs` | entry point, application id, log setup |
| `src/model.rs` | `Entry`, `Sense`, `Gloss`: the plain data everything else passes around |
| `src/dict/jmdict.rs` | streaming JMdict XML reader, expands the DTD entities |
| `src/dict/wadoku.rs` | streaming Wadoku XML reader: spellings, reading, accent numbers, grammar mapped to JMdict wording, German senses; unpacks the tar.xz |
| `src/dict/kanjidic.rs` | KANJIDIC2 reader: readings, meanings per language, grade, strokes, JLPT, frequency, radical |
| `src/dict/kanjivg.rs` | KanjiVG single-XML reader: SVG path data per stroke; newest release from the GitHub API |
| `src/dict/radkfile.rs` | RADKFILE reader (EUC-JP inside kradzip.zip): radical → kanji |
| `src/dict/sources.rs` | the source registry (id, name, URL, file, licence) and the download helper |
| `src/search/mod.rs` | the search pipeline: romaji to kana, deinflection, database, hits with notes |
| `src/search/query.rs` | the box syntax: `#tags`, `"exact"` quotes, `*`/`?` wildcards |
| `src/search/romaji.rs` | romaji → hiragana/katakana (Hepburn and the usual typing variants) |
| `src/search/deinflect.rs` | rule table from inflected verbs and adjectives back to dictionary forms |
| `src/store/db.rs` | SQLite schema (v3: `sources`, entries per source, FTS5 over glosses), insert, load, lookup, search |
| `src/store/user.rs` | the user database: word lists, migrated forward, JSON backup |
| `src/store/csv.rs` | just enough CSV for list import and export |
| `src/store/export.rs` | list files in other tools' layouts (CSV, Anki, Kitsun, Takoboto) and reading Takoboto exports |
| `src/store/import.rs` | the import, download and remove jobs the UI runs on a worker thread |
| `src/ui/mod.rs` | app startup, actions, CSS, the one main window |
| `src/ui/window.rs` | search entry, result list, split view, import flow |
| `src/ui/entry_view.rs` | renders one entry |
| `src/ui/import_dialog.rs` | progress dialog + worker thread + channel |
| `src/ui/preferences.rs` | preferences dialog: General, and Dictionaries (installed sources) |
| `src/ui/lists.rs` | the Lists sidebar page: lists, one list's entries, rename/delete/export/import |
| `src/ui/kanji_view.rs` | the kanji page: diagram, facts, readings, meanings, parts, words with the kanji |
| `src/ui/strokes.rs` | SVG path parser and the cairo drawing area that writes a kanji stroke by stroke |
| `src/ui/radicals.rs` | the Kanji sidebar page: search by radicals with a stroke filter |
| `src/ui/theme.rs` | colour scheme: follow the system, or force light / dark above the user's GTK CSS |
| `src/config.rs` | JSON config in `~/.config/tango`, XDG paths |
| `src/autopilot.rs` | scripted UI driving (below) |
| `tests/fixtures/` | small samples of JMdict, Wadoku, KANJIDIC2, KanjiVG and RADKFILE the unit tests use |
| `packaging/arch/PKGBUILD` | the `tango-git` Arch package; `install.sh` builds it from the checkout |

## The two kinds of data

The dictionary database (`~/.local/share/tango/tango.sqlite`) is derived data:
every row can be imported again from the downloads in `~/.cache/tango`. So a
schema version bump (`SCHEMA_VERSION` in `src/store/db.rs`) does not migrate
anything; `Database::open` drops the tables and the app shows the empty state
with an "Import the downloaded copy" button. User data lives in `user.sqlite`
next to it (`src/store/user.rs`): word lists, migrated forward with
`PRAGMA user_version` and never dropped, precisely so the dictionary file can
stay disposable. A list entry keeps the headword, reading and first gloss it
was added with, so lists read and export without the dictionary.

Each source in `src/dict/sources.rs` is one row in `sources` (version as the
file states it, import time, entry count) and owns its rows in `entries`
through the `source` column; `(source, seq)` is unique, `seq` being the
source's own number (JMdict `ent_seq`). Search takes the enabled sources in the
order from the config file and ranks exact matches, then common words, then
source order, then length.

## Search

`search::run` takes the query apart before the database sees it. Japanese text
runs the prefix search and then `deinflect::deinflect`, a rule table in the
style of Yomitan's: every rule strips a suffix and says what kind of word it
applies to and what kind comes out (ichidan, godan, i-adjective, 来る, する).
Candidates are generated blindly, at most six rules deep, and verified with one
exact lookup plus a part-of-speech check, so an over-eager rule costs a lookup,
never a wrong result. The chain is shown in the result row, innermost form
first ("書かれました → 書く: passive, polite, past"). Text that is romaji
throughout is converted to hiragana and katakana as well; exact reading
matches come first, then the gloss search, then the deinflected readings.

`query::parse` takes `#tags` (common, noun, verb, adjective, adverb, expression,
counter, particle, prefix, suffix, pronoun, conjunction, interjection,
abbreviation, kana), `"quotes"` and `*`/`?` wildcards off the text. Tags filter
the hits after the database (fetched four times over to compensate); quotes
mean a whole gloss or an exact form; wildcards run a LIKE pattern search, the
one slow path, only when typed. Japanese text that matches nothing as a whole
is cut into words, longest dictionary match from the left with inflections,
and the result list shows a header per word (`Hit::group`). Unknown tags do
not filter; they show up as a hint under "No results".

The gloss search is an FTS5 query over `gloss_fts`, an external-content index
on `glosses.text` with the `unicode61` tokenizer and diacritics removed, so
"uber" finds "über". Every typed word is quoted and the last one is a prefix.
The index is rebuilt after each import and removal (11 s for the full JMdict,
35 MB on disk). Measured on the 2026-09-04 JMdict: the previous LIKE scan took
470 ms per query, FTS5 takes 2 to 14 ms.

## Word list files

`store::export` writes a list in a chosen layout and `store::csv` does the
quoting. Plain CSV has a header (headword, reading, meaning, note, added). The
Anki file is a TSV with the `#separator`, `#html` and `#columns` directives
Anki's text importer reads (word, reading, meaning, note, tags). The Kitsun
file is a CSV with named columns (word, reading, meaning_en, meaning_de, note,
tags) for Kitsun's importer, which maps columns to card fields itself; there is
no Kitsun API yet, so sync waits. Meanings come from the dictionary entry (first
sense per language) and fall back to the gloss the list kept.
Takoboto's layout is what its Android app writes: comma-separated, UTF-8 with a
byte-order mark, no header, list name in column 1, `word, , reading` in column
4, meanings joined with `, , ` in column 5; columns 2 and 3 are undocumented
and stay empty. Importing a CSV into a list detects a Takoboto export by that
fourth column and puts its rows into the lists it names instead. Words are
matched against the enabled dictionaries by headword, then reading.

## Kanji data

Three sources feed the kanji page and are managed like the dictionaries:
KANJIDIC2 (`kanji` table), KanjiVG (`kanji_strokes`: the SVG path data per
stroke, drawn with cairo by `ui::strokes`, which parses the move, line and
cubic commands KanjiVG uses) and RADKFILE (`kanji_radicals`). They hold no
entries, so `remove_source` clears their table instead. The kanji page shows
whatever is installed; "words with this kanji" is a GLOB scan over the kanji
forms, fine on demand. The radical search intersects `kanji_radicals` and
greys out radicals no remaining kanji contains.

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

## Packaging

`packaging/arch/PKGBUILD` builds `tango-git` from git: `cargo fetch --locked` in
`prepare()`, `--frozen` builds and tests after, PNG icons rendered with
`rsvg-convert`. `TANGO_GIT_URL=file://<checkout>` makes it clone the local
repository instead of GitHub, which is what `install.sh` does on Arch and what
the CI job does in an `archlinux:base-devel` container. The package is built
from the committed state, never from the working tree. `options=(!lto)` is
needed: makepkg's link-time optimisation drops the C and assembly objects of
the `ring` crate (TLS for the downloads) and the link fails.

## Environment variables

| Variable | Purpose |
| --- | --- |
| `TANGO_DB` | use this SQLite file instead of `~/.local/share/tango/tango.sqlite` |
| `TANGO_USER_DB` | the word lists file instead of `~/.local/share/tango/user.sqlite`; set it for every headless run so test runs never touch the real lists |

`tango https://takoboto.jp/?w=1467640` (or `tango 1467640`) opens that JMdict
entry, in the running instance if there is one; the desktop entry passes
URLs through (`Exec=tango %U`).
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
export TANGO_DB=/tmp/tango-test/tango.sqlite TANGO_USER_DB=/tmp/tango-test/user.sqlite XDG_CONFIG_HOME=/tmp/tango-test/config
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

The README slideshow and `docs/TOUR.md` are captured the same way, with all
five dictionaries imported, a throwaway user database holding a few
favourites, and `XDG_CONFIG_HOME` pointing at a config with
`{"gloss_languages": ["ger", "eng"]}`. One 1280×720 capture per screen goes
into `data/screenshots/`; the GIF is built from the same files:

```
magick -delay 280 -loop 0 entry.png deinflect.png sentence.png wadoku.png examples.png \
  kanji.png radicals.png lists.png dictionaries.png light.png -resize 960x540 -layers Optimize tour.gif
```

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
