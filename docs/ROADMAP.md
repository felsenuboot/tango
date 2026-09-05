# Roadmap

The issues are grouped into GitHub milestones. The order comes from two
foundations that almost everything else builds on, and from what a learner
uses every day.

## The two foundations

**Data model.** Every dictionary entry belongs to a source, and the dictionary
database is derived and disposable: a schema change drops it and the cached
downloads are imported again. User data (word lists, notes, account caches)
lives in its own file with real migrations. This is what #2 sets up and what
#4, #5, #6 and #18 build on.

**Search pipeline.** One query parser (tags, quotes, wildcards), then candidate
generation (romaji to kana, deinflection), then the database query, then
ranking. #7, #16 and #8 are three steps of one piece of work.

## Milestones

### 0.2 Solid JMdict
- #2 source model and the Dictionaries page (version, import date, update,
  remove, search toggle and order)
- #3 merge the per-language senses JMdict ships
- #9 Arch package (PKGBUILD) instead of `install.sh`

### 0.3 Search
- #7 romaji input and deinflection
- #16 search syntax: tags, wildcards, exact quotes, mixed sentence queries
- #8 FTS5 for the gloss search, before the big data sets land

### 0.4 Lists
- #10 word lists in the user database
- #14 Takoboto and #15 Kitsun/Anki: one export module, three column layouts

### 0.5 More dictionaries
- #4 Wadoku with pitch accent (the largest reader; not the first source added)
- #5 kanji view: KANJIDIC2, KanjiVG, radicals
- #6 Tatoeba sentences: examples under the entry, `#sentences` search, a sentence page
- #17 remaining JMdict entry details (chips, notes, origins, references)
- #18 JMnedict as a names source: exact matches and `#names` only
- #19 JLPT lists: opt-in download of Waller's lists with JMdict numbers
  (stephenmk/yomitan-jlpt-vocab, CC BY-SA), chips and `#jlpt-nX`
- #20 external links and the optional online fallback

### 0.6 Accounts
- #11 WaniKani: token in the keyring (libsecret), incremental sync into the
  provider-independent `learned` table of the user database, chips and
  `#known` filters, the WaniKani list, themes, the tour and the guide

### 0.7 Robustness
The 2026-09-06 review of the whole code base: #100–#114 and #134. Timeouts on
every download and API call, the searches on a worker thread (#104) and the
kanji of each form indexed at import (#105), transactional list imports,
friendlier errors, an AppStream file, Dependabot and `cargo audit`.

### 0.8 Entry page and UX
- #96 sections of the entry page as collapsible cards
- #95 example sentences coloured by what the user knows, optional furigana
- #79 the UI/UX pass from Felix's walk-through
- #66 `Window::new` split into builders
- #69 the rest of pronunciation: Tatoeba sentence audio

### 0.9 macOS
- #115 the keyring behind a platform layer (libsecret is Linux-only)
- #116 TTS through `say`
- #117 Homebrew build, bundled SQLite, a CI job, an app bundle

### Later
Blocked on third parties or not scheduled: #12 MaruMori (its API details are
not public), Kitsun sync (#15, no API yet), #118 translations.

## Decisions taken
- #3: JMdict lists Dutch, French and German glosses as separate senses after
  the English ones and aligns nothing. Tango pairs a language with the English
  meanings by position when the sense counts are equal and lists it as its own
  block otherwise (details and numbers in `docs/DEVELOPMENT.md`).
- #19: the JLPT lists are downloaded only when the user asks, never by
  default.
- #12 sits in *Later* rather than holding 0.6 open: MaruMori's endpoints and
  key format are only visible in its app settings, so the work cannot start
  without them.
- Performance (2026-09-06, on the real database of 665,201 entries): the
  indexed searches answer in 2–6 ms, so there is nothing to gain from another
  language. What stalled the window were SQL scans on the UI thread: the
  wildcard gloss search (0.8 s) and the kanji page's word list (157 ms). The
  searches moved to a worker thread with their own connection (#104) and the
  kanji of every form are indexed at import (#105, 2–26 ms).
