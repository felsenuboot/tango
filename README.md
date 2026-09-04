# <img src="data/icons/hicolor/scalable/apps/io.github.felsenuboot.Tango.svg" width="40" alt=""> Tango 単語

[![CI](https://github.com/felsenuboot/tango/actions/workflows/ci.yml/badge.svg)](https://github.com/felsenuboot/tango/actions/workflows/ci.yml)

A Japanese dictionary for the GNOME desktop: GTK 4, libadwaita and Rust.
Offline, with English and German glosses side by side.

> [!NOTE]
> Early scaffold. It imports JMdict and searches it; everything under
> *Roadmap* is still to come. Personal project, written largely with Claude
> Code and reviewed by a human. No warranty.

![Searching 猫, dark theme](data/screenshots/entry-dark.png)

## What works

- **JMdict import.** Download the file from inside the app (about 22 MB) or
  import a local copy. ~220k entries land in a local SQLite file in seconds.
- **Search.** Type kana, kanji or romaji for a headword/reading lookup, or an
  English or German word for a gloss lookup. Inflected forms find their
  dictionary entry with the chain shown (書きました → 書く: polite, past); a
  pasted sentence is cut into words. `#common`, `#verb`, `#noun` and friends
  filter, `"quotes"` mean exactly that, `*` and `?` are wildcards. Exact
  matches and common words come first.
- **Entries.** Headword, readings, alternative spellings, every meaning with
  its parts of speech and the translations per language in the order you
  prefer. JMdict ships the German, Dutch and French glosses as separate
  senses; Tango lines them up with the English meanings where the sense
  counts match and lists the rest per language.
- **Word lists.** Star an entry (Ctrl+D) for Favourites or add it to any list
  from the button next to the star; the Lists page in the sidebar browses,
  renames and reorders them. Export a list as CSV, import a CSV of words, and
  back up all lists as JSON.
- **Dictionaries page.** Preferences lists every source with its version,
  import date and entry count, with update, remove and a search toggle per
  dictionary. Today that is JMdict; the roadmap adds the others.
- Adaptive layout (sidebar collapses on narrow windows), light and dark theme (follow the system or force one),
  keyboard shortcuts (`Ctrl+F` / `/` search, `Ctrl+I` import, `Ctrl+,` preferences).

## Roadmap

- Wadoku (German, with pitch accent) next to JMdict
- Kanji view: KANJIDIC details, KanjiVG stroke order, radicals
- Example sentences from Tatoeba

Everything above plus the WaniKani, MaruMori, Kitsun.io and Takoboto
integrations is tracked in the [issues](https://github.com/felsenuboot/tango/issues);
the order and the reasoning are in [docs/ROADMAP.md](docs/ROADMAP.md).

## Install

Rust 1.85+, GTK 4.12+, libadwaita 1.5+, SQLite.

```
git clone https://github.com/felsenuboot/tango.git
cd tango
./install.sh
```

On Arch Linux that builds the `tango-git` package from the checkout
(`packaging/arch/PKGBUILD`, following the Rust package guidelines) and installs
it with pacman; `makepkg -si` in `packaging/arch` builds it from GitHub instead.
Anywhere else it puts a release build into `~/.local/bin` with the desktop entry
and icons (Debian/Ubuntu: `cargo libgtk-4-dev libadwaita-1-dev libsqlite3-dev`).
Or just `cargo run` from the checkout. There is no Flatpak, and Flathub is not
planned.

## Dictionaries and licences

- [JMdict](https://www.edrdg.org/wiki/index.php/JMdict-EDICT_Dictionary_Project) is
  the property of the Electronic Dictionary Research and Development Group and
  used under its [licence](https://www.edrdg.org/edrdg/licence.html)
  (CC BY-SA 4.0). The app downloads it on first use; nothing is bundled.

## Development

See [docs/DEVELOPMENT.md](docs/DEVELOPMENT.md) for the code layout, tests,
the autopilot for scripted UI runs and screenshots, and a reading order for
the Rust newcomer.

## Licence

MIT, see [LICENSE](LICENSE).
