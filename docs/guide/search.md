# Search

## What a query can be

Type kana, kanji, romaji, English or German. Exact matches and common words
come first; entries in a word list carry a star. Everything answers in a
few milliseconds, from the local database.

![Searching 猫, the entry with German and English meanings](../../data/screenshots/entry.png)

Inflected forms find their dictionary form and say how: 書かれました is 書く,
passive, polite, past.

![書かれました resolves to 書く with the chain in the result row](../../data/screenshots/deinflect.png)

A pasted sentence is cut into words, each with its own group of results.

![猫が魚を食べました cut into 猫, が, 魚, を, 食べました](../../data/screenshots/sentence.png)

## Tags, quotes and wildcards

| Query | Meaning |
| --- | --- |
| `#common` `#verb` `#noun` `#adjective` … | only entries with that tag; several tags combine |
| `"cat"` | a whole gloss or an exact form, not a substring |
| `ta*` `t?ko` | wildcards: any run of characters, one character |
| `#sentences 猫が` | search the Tatoeba sentences instead of the entries |
| `#names さとう` | search JMnedict, the names file, and nothing else |
| `#jlpt-n5` … `#jlpt-n1` | only that JLPT level; alone, the whole level as a list |
| `#known` `#unknown` `#kanji-known` `#wk-level-12` | by what WaniKani says you have learned (see [WaniKani](wanikani.md)) |

A name also shows up, after the words, when a query matches one of its forms
exactly: people, places, companies.

![#names さとう](../../data/screenshots/names.png)

With the JLPT lists installed (an opt-in download; the lists are unofficial),
entries and rows carry a green level chip.

![#jlpt-n5](../../data/screenshots/jlpt.png)

## Cards and tiles

With "Search results too" on under Preferences → General → Lists and
results, the hits appear as tiles in the sidebar and as cards in the content
pane instead of opening the first hit; click one to open it.

## No results

A search that finds nothing offers the same query on jisho.org in the
browser, and says which of the tags in it needs a dictionary that is not
installed.

![No results, with the jisho.org button](../../data/screenshots/no-results.png)

## Back

Wherever you click through, the back button in the header (or `Alt+Left`)
returns to the previous view: the entry a "See also" came from, the sentence
a word was opened from, the kanji page behind one of its words. The history
holds a hundred steps and empties when a dictionary is imported.

---
[Guide index](README.md) · [Tour](../TOUR.md)
