# Dictionaries

Nothing is bundled. On first start the app asks for JMdict (about 22 MB);
everything else is a click on the Dictionaries page in Preferences:

| Source | What it adds |
| --- | --- |
| JMdict | the dictionary: English, German, Dutch, French and other glosses |
| Wadoku | Japanese–German with pitch accent |
| KANJIDIC2, KanjiVG, RADKFILE | the kanji pages, stroke order, search by parts |
| Tatoeba | example sentences with English and German translations |
| JMnedict | names: people, places, companies |
| JLPT lists | the unofficial N5–N1 lists as chips and filters |

Every source shows its version, import date and entry count. Update
downloads today's file; the switch takes a dictionary out of the search;
the order of the rows is the order of the results. Downloads and imports
queue up and run in the background: click Download on three sources and
keep searching while the rows show their progress. Closing the window with
a job running asks whether to keep it running.

![The Dictionaries page while JMdict imports and Tatoeba waits](../../data/screenshots/queue.png)

When a new version of Tango changes the database layout, the dictionaries
are imported again from the downloaded copies on the next start, by
themselves. The licences of all sources are listed in the
[README](../../README.md#dictionaries-and-licences).

---
[Guide index](README.md) · [Tour](../TOUR.md)
