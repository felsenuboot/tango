# Tango 単語 – notes for Claude Code

Japanese dictionary for GNOME in Rust with gtk4-rs 0.11 and libadwaita-rs 0.9
(crate features `v4_12` / `v1_5`, so Ubuntu 24.04 CI builds). Private repo
`felsenuboot/tango`, scaffolded 2026-09-04.

## Decisions

- **Rust, deliberately.** Python was recommended first; Felix chose Rust for
  performance and to learn the language. Do not suggest switching back.
- **Plain Rust, no GObject subclassing.** State lives in `Rc<RefCell<…>>`,
  callbacks capture `clone!(#[weak] …)`. Comment idioms where a newcomer would
  stumble; `docs/DEVELOPMENT.md` has the reading order.
- **JMdict comes from `https://www.edrdg.org/pub/Nihongo/JMdict.gz`.** The
  `ftp.edrdg.org` host has a broken TLS certificate. Nothing is bundled.
- **No Flatpak for Felix, no Flathub ever.** Felix runs Arch and wants a PKGBUILD;
  Flathub is against its terms for this project. A Flatpak manifest is at most
  a low-priority option for other distros.
- **JMdict ships German/Dutch/French glosses as separate senses.** Merging
  them per meaning is the first roadmap item.
- **Colour scheme is a three-way toggle: Follow system / Light / Dark (issue #1).**
  Felix wants all three and said so twice; never reduce it to "follow the
  system". His desktop (Hyprland, Matugen) writes `~/.config/gtk-4.0/colors.css`,
  which GTK loads above app CSS, so the forced schemes re-declare libadwaita's
  named colours one priority above user CSS (`src/ui/theme.rs`).

## Working here

- `cargo test`, `cargo clippy --all-targets -- -D warnings`, `cargo fmt`
  (max_width 110) must stay clean; CI runs exactly those plus shellcheck.
- Headless UI checks: `TANGO_AUTOPILOT` script + `TANGO_DB` + cage + grim,
  see `docs/DEVELOPMENT.md`. The real JMdict imports in ~13 s (release).
- Stop headless runs by PID, never `pkill` by name (took Hyprland down once).
- Commit and push at sensible milestones; the repo stays private.
- Session transcripts live in `docs/sessions/`.

## Roadmap

Wadoku (German, pitch accent), kanji view (KANJIDIC, KanjiVG, radicals),
Tatoeba examples, romaji input and deinflection, FTS5, Arch PKGBUILD.
Integrations: Jisho feature parity, WaniKani, MaruMori, Kitsun.io, Takoboto.
Tracked as GitHub issues #2–#15.
