<div align="center">
  <img src="data/icons/hicolor/scalable/apps/io.github.felsenuboot.Tango.svg" width="128" alt="">
  <h1>Tango 単語</h1>
  <p>A Japanese dictionary for GNOME</p>
  <a href="https://github.com/felsenuboot/tango/actions/workflows/ci.yml"><img src="https://github.com/felsenuboot/tango/actions/workflows/ci.yml/badge.svg" alt="CI"></a>
  <a href="https://github.com/felsenuboot/tango/releases"><img src="https://img.shields.io/github/v/release/felsenuboot/tango?display_name=tag" alt="Release"></a>
</div>

Tango is an offline Japanese dictionary for the GNOME desktop, written in Rust
with GTK 4 and libadwaita. JMdict and Wadoku side by side, English and German,
kanji with stroke order, word lists, and the search a Jisho user expects.

![The main screens of Tango, one after the other](data/screenshots/tour.gif)

> [!NOTE]
> A personal project, written largely with Claude Code and reviewed by a
> human. It works on my machine (Arch, Hyprland). No warranty.

## Features

- 🔍 **Search the way you think.** Kana, kanji, romaji, English or German.
  Inflected forms find their dictionary entry with the chain shown
  (書かれました → 書く: passive, polite, past); a pasted sentence is cut into
  words; `#common`, `#verb` and friends filter; `"quotes"` and `*` wildcards
  do what they say. Everything answers in a few milliseconds.
- 📖 **Two dictionaries.** JMdict from the EDRDG, with its German, Dutch and
  French glosses lined up with the English meanings; Wadoku from wadoku.de
  with pitch accent shown as ⓪ ① ② beside the reading. Search covers both,
  in the order you set.
- 🈷 **Kanji.** Click a kanji in a headword for its page: stroke order from
  KanjiVG, written stroke by stroke on request, readings, meanings, grade,
  JLPT level, frequency, its parts, and the words that use it. Find a kanji
  by its parts on the Kanji page of the sidebar.
- ⭐ **Word lists.** Star an entry for Favourites, add it to any list from the
  button above the entry or a right-click on a result. Export as CSV, for
  Anki, Kitsun or Takoboto; import a CSV or a Takoboto export; back up all
  lists as JSON.
- 🔗 **Takoboto.** Open an entry in Takoboto, and open Takoboto links in Tango.
- 🗂️ **Dictionaries page.** Every source with version, import date and entry
  count; update, remove, toggle. Nothing is bundled; downloads happen on
  request.
- 🎨 **Desktop.** Adaptive layout, light and dark theme or the system's,
  keyboard shortcuts (`Ctrl+F` / `/` search, `Ctrl+D` star, `Ctrl+I` import,
  `Ctrl+,` preferences).

The [tour](docs/TOUR.md) shows each of these with screenshots.

## Install

Rust 1.85+, GTK 4.12+, libadwaita 1.5+, SQLite, liblzma.

```
git clone https://github.com/felsenuboot/tango.git
cd tango
./install.sh
```

**Arch Linux.** `./install.sh` builds the `tango-git` package from the
checkout (`packaging/arch/PKGBUILD`, following the Rust package guidelines)
and installs it with pacman; `makepkg -si` in `packaging/arch` builds it from
GitHub instead.

**Anywhere else.** `./install.sh` puts a release build into `~/.local/bin`
with the desktop entry and icons. Or just `cargo run` from the checkout.

| Distribution | Packages |
| --- | --- |
| Arch | `rust gtk4 libadwaita sqlite xz` |
| Debian, Ubuntu | `cargo libgtk-4-dev libadwaita-1-dev libsqlite3-dev liblzma-dev` |

There is no Flatpak, and Flathub is not planned.

On first start, click **Download JMdict** (about 22 MB). Wadoku, KANJIDIC2,
KanjiVG and the radical index are one click each on the Dictionaries page in
Preferences.

## Dictionaries and licences

- [JMdict](https://www.edrdg.org/wiki/index.php/JMdict-EDICT_Dictionary_Project),
  [KANJIDIC2](https://www.edrdg.org/wiki/KANJIDIC_Project.html) and
  [RADKFILE](https://www.edrdg.org/krad/kradinf.html) are the property of the
  Electronic Dictionary Research and Development Group and used under its
  [licence](https://www.edrdg.org/edrdg/licence.html) (CC BY-SA 4.0).
- [Wadoku](https://www.wadoku.de/) (Japanese–German, with pitch accent) is
  © Ulrich Apel and the Wadoku.de contributors, used under the
  [Wadoku dictionary licence](https://www.wadoku.de/wiki/display/WAD/W%C3%B6rterbuch+Lizenz),
  which allows use in free software with attribution.
- [KanjiVG](https://kanjivg.tagaini.net/) stroke order data is © Ulrich Apel,
  Creative Commons Attribution-ShareAlike 3.0.

Everything is downloaded on request from the Dictionaries page; nothing is
bundled with the app.

## Roadmap

Example sentences from Tatoeba, the remaining JMdict entry details, JMnedict
names, JLPT levels, and the WaniKani and MaruMori integrations. Tracked as
[milestones](https://github.com/felsenuboot/tango/milestones); the order and
the reasoning are in [docs/ROADMAP.md](docs/ROADMAP.md).

## Development

See [docs/DEVELOPMENT.md](docs/DEVELOPMENT.md) for the code layout, tests,
the autopilot for scripted UI runs and screenshots, the branch-and-release
process, and a reading order for the Rust newcomer.

## Name and licence

単語 (*tango*) is the Japanese word for "word". MIT licence, see
[LICENSE](LICENSE).
