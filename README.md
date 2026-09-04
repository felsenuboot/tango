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
- **Search.** Type kana or kanji for a headword/reading lookup, or an English
  or German word for a gloss lookup. Exact matches and common words come first.
- **Entries.** Headword, readings, alternative spellings, every sense with its
  parts of speech and the glosses per language, in the order you prefer.
- Adaptive layout (sidebar collapses on narrow windows), follows the system colour scheme,
  keyboard shortcuts (`Ctrl+F` / `/` search, `Ctrl+I` import, `Ctrl+,` preferences).

## Roadmap

- Merge the per-language senses JMdict ships (the German, Dutch and French
  glosses come as separate senses) so each meaning shows its translations together
- Wadoku (German, with pitch accent) next to JMdict
- Kanji view: KANJIDIC details, KanjiVG stroke order, radicals
- Example sentences from Tatoeba
- Romaji input, deinflection of verbs and adjectives
- FTS5 index for faster gloss search
- Arch package (PKGBUILD) instead of `install.sh`

Everything above plus the WaniKani, MaruMori, Kitsun.io and Takoboto
integrations is tracked in the [issues](https://github.com/felsenuboot/tango/issues).

## Install

Rust 1.85+, GTK 4.12+, libadwaita 1.5+, SQLite.

```
# Arch
sudo pacman -S --needed rust gtk4 libadwaita sqlite
# Debian/Ubuntu: cargo libgtk-4-dev libadwaita-1-dev libsqlite3-dev

git clone https://github.com/felsenuboot/tango.git
cd tango
./install.sh        # builds a release binary into ~/.local/bin and adds the desktop entry
```

Or just `cargo run` from the checkout.

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
