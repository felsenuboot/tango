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
- **JMdict ships German/Dutch/French glosses as separate senses** with no
  alignment to the English ones. `Entry::grouped` pairs a language by position
  when its sense count equals the English count, else shows it as a block
  (issue #3, numbers in `docs/DEVELOPMENT.md`). Do not "fix" that by attaching
  a lone German sense to meaning 1; the samples say it is often not meaning 1.
- **Colour scheme is a three-way toggle: Follow system / Light / Dark (issue #1).**
  Felix wants all three and said so twice; never reduce it to "follow the
  system". His desktop (Hyprland, Matugen) writes `~/.config/gtk-4.0/colors.css`,
  which GTK loads above app CSS, so the forced schemes re-declare libadwaita's
  named colours one priority above user CSS (`src/ui/theme.rs`).

- **The dictionary database is disposable, user data is not.** A schema bump
  drops and recreates `tango.sqlite` (re-import from the cache); word lists live
  in `user.sqlite` with forward migrations. Headless runs must set
  `TANGO_USER_DB` so they never touch Felix's lists.
- **Roadmap order is in `docs/ROADMAP.md`** and as GitHub milestones 0.2–0.6.
  Work them in that order unless Felix says otherwise.

## Working here

- `cargo test`, `cargo clippy --all-targets -- -D warnings`, `cargo fmt`
  (max_width 110) must stay clean; CI runs exactly those plus shellcheck.
- Headless UI checks: `TANGO_AUTOPILOT` script + `TANGO_DB` + cage + grim,
  see `docs/DEVELOPMENT.md`. The real JMdict imports in ~13 s (release).
- Stop headless runs by PID, never `pkill` by name (took Hyprland down once).
- Felix's Hyprland uses the Lua config: `hyprctl dispatch movecursor 1 2` is a
  syntax error there, so drive the app through `TANGO_AUTOPILOT` instead.
- Hyprland shrinks popups of fullscreen windows by the top bar's reserved strip
  (see `docs/DEVELOPMENT.md`); `window.rs` has the workaround.
- One issue → one branch (`<issue>-<slug>`) → one pull request (`Closes #N`,
  milestone set) → squash-merge with the subject `area: what (#N)`. No direct
  pushes to master, CI must be green (a convention; GitHub cannot enforce it on
  a private free repo). Milestone done → bump
  `Cargo.toml`, tag `vX.Y.0`, GitHub release. Details: `docs/DEVELOPMENT.md`.
- Session transcripts live in `docs/sessions/`.

## Roadmap

Milestones on GitHub, details in `docs/ROADMAP.md`: 0.2 Solid JMdict (#2 #3
#9), 0.3 Search (#7 #16 #8), 0.4 Lists (#10 #14 #15), 0.5 More dictionaries
(#4 #5 #6 #17 #18 #19 #20), 0.6 Accounts (#11 #12). #13 is the Jisho-parity
umbrella; #16–#20 are its sub-issues.
