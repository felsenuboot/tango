<div align="center">
  <img src="data/icons/hicolor/scalable/apps/io.github.felsenuboot.Tango.svg" width="128" alt="">
  <h1>Tango 単語</h1>
  <p>A Japanese dictionary for GNOME</p>
  <a href="https://github.com/felsenuboot/tango/actions/workflows/ci.yml"><img src="https://github.com/felsenuboot/tango/actions/workflows/ci.yml/badge.svg" alt="CI"></a>
  <a href="https://github.com/felsenuboot/tango/releases"><img src="https://img.shields.io/badge/release-v0.5.0-4a86cf" alt="Release v0.5.0"></a>
</div>

Tango is an offline Japanese dictionary for the GNOME desktop, written in Rust
with GTK 4 and libadwaita. JMdict and Wadoku side by side, English and German,
kanji with stroke order, word lists, and the search a Jisho user expects.

![The main screens of Tango, one after the other](data/screenshots/tour.gif)

> [!NOTE]
> A personal project, written largely with Claude Code and reviewed by a
> human. It works on my machine (Arch, Hyprland). No warranty.

## Features

- 🔍 **Search.** Kana, kanji, romaji, English or German; inflected forms
  resolve to their entry with the chain shown; a pasted sentence is cut into
  words; `#tags`, `"quotes"` and `*` wildcards. A few milliseconds, offline.
- 📖 **Two dictionaries.** JMdict with every tag, note and cross-reference it
  carries, and Wadoku with pitch accent; searched together, in your order.
- 🔊 **Pronunciation.** The pitch accent as a graph over the moras, and a
  speaker button when the system has a Japanese voice (nothing bundled).
- 🏷️ **Names.** JMnedict as a dictionary of its own.
- 🎓 **JLPT levels.** The unofficial N5–N1 lists as chips and filters, opt-in.
- 💬 **Example sentences.** Tatoeba under every entry, English and German;
  `#sentences` searches the sentences themselves.
- 🈷 **Kanji.** Stroke order from KanjiVG, readings, meanings, parts and
  words; find a kanji by its parts.
- ⭐ **Word lists.** Favourites and lists of your own, as rows, tiles or a
  wall of cards; export for Anki, Kitsun and Takoboto, import CSV or a
  Takoboto export.
- 🔗 **Elsewhere.** Jisho, Wadoku, Wikipedia, Wiktionary and Takoboto one
  button away; Takoboto links open in Tango.
- 🗂️ **Dictionaries page.** Every source with version and count; downloads
  and imports queue in the background while you search.
- 🐊 **WaniKani.** Level and SRS stage chips on entries and kanji, `#known`
  and friends, a filterable WaniKani list.
- 🎨 **Desktop.** Adaptive layout; system, light and dark plus WaniKani and
  Sanzo Wada themes; your own CSS; keyboard shortcuts.

The [tour](docs/TOUR.md) is the short version and the
[guide](docs/guide/README.md) has a page per topic with screenshots.

## Install

Rust 1.85+, GTK 4.12+, libadwaita 1.5+, SQLite, liblzma, libsecret.

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

**Developing.** `./install.sh --user` does the per-user install on Arch as
well, without sudo. Do it once even if you run the app with `cargo run`: the
dock and the app switcher find a window's icon through its installed desktop
entry, and show a generic one until it exists.

| Distribution | Packages |
| --- | --- |
| Arch | `rust gtk4 libadwaita sqlite xz libsecret` |
| Debian, Ubuntu | `cargo libgtk-4-dev libadwaita-1-dev libsqlite3-dev liblzma-dev libsecret-1-dev` |

There is no Flatpak, and Flathub is not planned.

On first start, click **Download JMdict** (about 22 MB). Wadoku, KANJIDIC2,
KanjiVG, the radical index, the JMnedict names, the JLPT lists and the
Tatoeba sentences are one click each on the Dictionaries page in Preferences.

## Dictionaries and licences

- [JMdict](https://www.edrdg.org/wiki/index.php/JMdict-EDICT_Dictionary_Project),
  [JMnedict](https://www.edrdg.org/enamdict/enamdict_doc.html),
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
- [Tatoeba](https://tatoeba.org/) sentences and the Tanaka corpus index are
  released under Creative Commons Attribution 2.0 France.
- The JLPT lists are [Jonathan Waller's](http://www.tanos.co.uk/jlpt/)
  (CC BY), with JMdict numbers added by Stephen Kraus in
  [yomitan-jlpt-vocab](https://github.com/stephenmk/yomitan-jlpt-vocab)
  (CC BY-SA 4.0). They are unofficial; there are no official lists.

Everything is downloaded on request from the Dictionaries page; nothing is
bundled with the app.

The 単語 calligraphy on the start screen is set in
[Yuji Syuku](https://github.com/Kinutafontfactory/Yuji) (SIL Open Font
License 1.1); only the two glyph outlines ship (`data/calligraphy.svg`).

## Roadmap

External links, and the WaniKani and MaruMori integrations. Tracked as
[milestones](https://github.com/felsenuboot/tango/milestones); the order and
the reasoning are in [docs/ROADMAP.md](docs/ROADMAP.md).

## Development

See [docs/DEVELOPMENT.md](docs/DEVELOPMENT.md) for the code layout, tests,
the autopilot for scripted UI runs and screenshots, the branch-and-release
process, and a reading order for the Rust newcomer.

## Manual checks

What the headless runs cannot verify and Felix still has to try on a real
desktop. Tick and delete when done; a bug goes into a new issue.

- [ ] **#37 narrow sidebar.** Resize the window to about 830 px wide: the
  Search / Lists / Kanji switcher should show icons with the label
  underneath instead of "Se… / Li… / Ka…"; below 640 px the sidebar takes
  the whole window and the switcher stays narrow.
- [ ] **#36 background queue.** On the Dictionaries page click Download on
  three sources in a row: the rows show "Queued…" then the progress line, a
  spinner turns in the sidebar header, the search keeps working, and
  closing the window asks "Keep running / Quit anyway".
- [ ] **#36 rebuild.** After the next schema bump a toast says the
  dictionaries are being imported again and every cached source comes back
  by itself; "Import downloaded copy" appears on sources with a cached file.
- [ ] **#38 hide parts.** Kanji page: pick 田, press the eye toggle: the
  parts that no longer fit disappear and their stroke groups collapse; the
  setting survives a restart.
- [ ] **#39 spacing.** The search entry, the Kanji page and the "Add to list"
  popover keep 12 px from the edges on a normal and on a HiDPI screen.
- [ ] **#20 links.** The "Open on another site" button offers Jisho, Wadoku,
  Wikipedia and Wiktionary for a Wadoku entry and adds Takoboto for a JMdict
  entry; a search that finds nothing offers "Search on jisho.org", and both
  open the browser.
- [ ] **#19 JLPT.** Download "JLPT" on the Dictionaries page (about 400 kB),
  then search `#jlpt-n5` and open 食べる: the green N5 chips show.
- [ ] **#6 sentences.** Open 猫, scroll to the example sentences, press
  "Show all", then try `#sentences 猫が` and a word button on the sentence page.
- [ ] **#17 details.** Open パソコン ("abbreviation" chip, "See also"
  button) and アルバイト ("from German: Arbeit").
- [ ] **#18 names.** Search 田中: the surname rows come after the word;
  `#names さとう` lists names only.
- [ ] **#58 themes.** Preferences → General → Theme: every entry of the list
  is legible on the entry page, the sidebar rows, the Dictionaries page and
  the kanji diagram; WaniKani Pink and Wada 276 are light, the other Wada
  ones dark; a `~/.config/tango/style.css` with `.tango-headword { color: red; }`
  takes effect after a restart.
- [ ] **#72 back.** Open 猫, click a "See also", click a kanji, click a word on
  the kanji page: the back button (or Alt+Left) walks back through all of
  it; after an import the history is empty.
- [ ] **#69 pronunciation.** Open 猫 (Wadoku installed): a pitch graph under the
  reading; the speaker buttons on the reading and on example sentences read
  through your VOICEVOX / Open JTalk voice; with `TANGO_NO_TTS=1` they are gone.
- [ ] **#65 accessibility.** With Orca running, Tab through the header
  buttons: each is announced by name (Favourite, Add to a list, Open on
  another site, Main menu), and Escape in the search box clears it.
- [ ] **#11 WaniKani.** Preferences → Accounts: paste a read-only token and
  Connect; the row shows your username and level, the first sync runs in the
  background (about thirty requests, half a minute), then 食べる shows a
  purple "WaniKani 6 · Guru" chip (or whatever your stage is), the kanji page
  of 食 too, and `#known` / `#kanji-known` filter. Sync now and Disconnect
  work; after Disconnect the keyring item "Tango: wanikani API token" is gone.
- [ ] **#55 WaniKani list.** Lists → WaniKani: the three filters narrow the
  rows, a word row opens its entry, a kanji row its page, Export as… writes
  the filtered rows, and Rename / Delete refuse with a toast.

## Name and licence

単語 (*tango*) is the Japanese word for "word". MIT licence, see
[LICENSE](LICENSE).
