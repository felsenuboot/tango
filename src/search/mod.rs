//! The search pipeline: query → candidate forms (romaji to kana, deinflection) → database → hits.
//!
//! Japanese input: the prefix search as before, then every dictionary form the text could be an
//! inflection of, verified against the entries' parts of speech. Other input: the gloss search,
//! and if the text is romaji throughout, the same Japanese path for its kana readings, with
//! exact reading matches first (so "sake" finds 酒 before "for the sake of").
//!
//! `query` takes `#tags`, quotes and wildcards off the text first. Japanese text that matches
//! nothing as a whole is cut into words (longest dictionary match from the left, inflections
//! included) and every word gets its own group of hits.

pub mod deinflect;
pub mod query;
pub mod romaji;

use std::collections::HashSet;

use crate::accounts::{Kind, is_known};
use crate::model::{Entry, Sentence};
use crate::store::db::{Database, is_japanese};
use crate::store::user::LearnedIndex;

/// One result: the entry, and how the query led to it when that is not obvious.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hit {
    pub entry: Entry,
    /// "食べました → 食べる: polite, past", for deinflected matches.
    pub note: Option<String>,
    /// The word of a sentence this hit belongs to, when the query was cut into words.
    pub group: Option<String>,
}

/// What a search produced: the hits, and a hint for the empty state.
#[derive(Debug, Default)]
pub struct Outcome {
    pub hits: Vec<Hit>,
    /// `#sentences`: example sentences instead of entries.
    pub sentences: Vec<Sentence>,
    /// Why there may be nothing, e.g. an unknown `#tag`.
    pub hint: Option<String>,
}

struct Hits<'a> {
    list: Vec<Hit>,
    seen: HashSet<(String, i64)>,
    tags: Vec<query::Tag>,
    group: Option<String>,
    /// What the learning accounts know, for `#known` and friends.
    learned: &'a LearnedIndex,
}

impl Hits<'_> {
    fn push(&mut self, entry: Entry, note: Option<String>) {
        if !self.tags.iter().all(|t| matches(&entry, *t, self.learned)) {
            return;
        }
        if self.seen.insert((entry.source.clone(), entry.id)) {
            self.list.push(Hit {
                entry,
                note,
                group: self.group.clone(),
            });
        }
    }
}

/// The best `(level, stage)` a learning account has for the word: any of its forms as
/// vocabulary.
fn learned_word(entry: &Entry, learned: &LearnedIndex) -> Option<(u32, u8)> {
    entry
        .kanji
        .iter()
        .chain(&entry.readings)
        .filter_map(|form| learned.get(&(Kind::Vocabulary, form.clone())).copied())
        .max_by_key(|(_, stage)| *stage)
}

fn matches(entry: &Entry, tag: query::Tag, learned: &LearnedIndex) -> bool {
    match tag {
        query::Tag::Common => entry.common,
        query::Tag::Pos(part) => entry.senses.iter().flat_map(|s| &s.pos).any(|p| p.contains(part)),
        query::Tag::Misc(part) => entry
            .senses
            .iter()
            .flat_map(|s| &s.misc)
            .any(|m| m.contains(part)),
        query::Tag::Sentences | query::Tag::Names => true,
        query::Tag::Jlpt(level) => entry.jlpt == Some(level),
        query::Tag::Known(wanted) => {
            learned_word(entry, learned).is_some_and(|(_, stage)| is_known(stage)) == wanted
        }
        query::Tag::WkLevel(level) => learned_word(entry, learned).is_some_and(|(l, _)| l == level),
        query::Tag::KanjiKnown => {
            let kanji: Vec<char> = entry
                .headword()
                .chars()
                .filter(|c| crate::ui::entry_view::is_kanji(*c))
                .collect();
            !kanji.is_empty()
                && kanji.iter().all(|c| {
                    learned
                        .get(&(Kind::Kanji, c.to_string()))
                        .is_some_and(|(_, stage)| is_known(*stage))
                })
        }
    }
}

/// `langs` are the Tatoeba language codes for the translations of sentence hits, preferred
/// first; `learned` is what the learning accounts know, for `#known` and friends.
pub fn run(
    db: &Database,
    input: &str,
    limit: usize,
    sources: &[String],
    langs: &[String],
    learned: &LearnedIndex,
) -> anyhow::Result<Outcome> {
    let q = query::parse(input);
    if q.sentences {
        let available = sources.iter().any(|s| s == "tatoeba") && db.has_sentences()?;
        let sentences = if q.text.is_empty() || !available {
            Vec::new()
        } else {
            db.search_sentences(&q.text, langs, limit)?
        };
        let hint = if !available {
            Some("Example sentences need Tatoeba, see the Dictionaries page in Preferences.".into())
        } else if q.text.is_empty() {
            Some("Type a word after #sentences to search the example sentences.".into())
        } else {
            None
        };
        return Ok(Outcome {
            hits: Vec::new(),
            sentences,
            hint,
        });
    }
    let mut hits = Hits {
        list: Vec::new(),
        seen: HashSet::new(),
        tags: q.tags.clone(),
        group: None,
        learned,
    };
    let hint = (!q.unknown_tags.is_empty()).then(|| {
        format!(
            "Unknown tag #{}. Tags: {}.",
            q.unknown_tags.join(", #"),
            query::TAGS
                .iter()
                .map(|(n, _)| format!("#{n}"))
                .collect::<Vec<_>>()
                .join(" ")
        )
    });
    // Names (JMnedict) come up for exact matches and with `#names`; prefix and gloss searches
    // leave them out, or every "さ" would drown in surnames.
    let names_only: Vec<String> = sources.iter().filter(|s| *s == "jmnedict").cloned().collect();
    let words: Vec<String> = sources.iter().filter(|s| *s != "jmnedict").cloned().collect();
    let (sources, words): (&[String], &[String]) = if q.names {
        (&names_only, &names_only)
    } else {
        (sources, &words)
    };
    let hint = hint.or_else(|| {
        (q.names && names_only.is_empty())
            .then(|| "Names need JMnedict, see the Dictionaries page in Preferences.".to_string())
    });
    let text = q.text.as_str();
    // `#jlpt-n5` on its own lists that level, common words first.
    let level = q.tags.iter().find_map(|t| match t {
        query::Tag::Jlpt(level) => Some(*level),
        _ => None,
    });
    if text.is_empty()
        && let Some(level) = level
    {
        for e in db.jlpt_words(level, limit * 4, sources)? {
            hits.push(e, None);
        }
        hits.list.truncate(limit);
        return Ok(Outcome {
            hits: hits.list,
            sentences: Vec::new(),
            hint,
        });
    }
    // `#known` or `#wk-level-12` on their own list the learned words themselves.
    let listing: Option<Vec<String>> = q.tags.iter().find_map(|t| match t {
        query::Tag::Known(true) => Some(
            learned
                .iter()
                .filter(|((kind, _), (_, stage))| *kind == Kind::Vocabulary && is_known(*stage))
                .map(|((_, text), _)| text.clone())
                .collect(),
        ),
        query::Tag::WkLevel(level) => Some(
            learned
                .iter()
                .filter(|((kind, _), (l, _))| *kind == Kind::Vocabulary && l == level)
                .map(|((_, text), _)| text.clone())
                .collect(),
        ),
        _ => None,
    });
    if text.is_empty()
        && let Some(mut texts) = listing
    {
        texts.sort();
        for e in db.lookup(&texts, sources, limit * 4)? {
            hits.push(e, None);
        }
        hits.list.truncate(limit);
        return Ok(Outcome {
            hits: hits.list,
            sentences: Vec::new(),
            hint,
        });
    }
    if text.is_empty() || sources.is_empty() {
        return Ok(Outcome {
            hits: hits.list,
            sentences: Vec::new(),
            hint,
        });
    }
    // Tags thin the hits out, so fetch more before filtering.
    let fetch = if q.tags.is_empty() { limit } else { limit * 4 };
    if q.wildcard {
        for e in db.search_pattern(text, fetch, words)? {
            hits.push(e, None);
        }
    } else if q.exact {
        if is_japanese(text) {
            for e in db.lookup(&[text.to_string()], sources, fetch)? {
                hits.push(e, None);
            }
        } else {
            for e in db.search_gloss_exact(text, fetch, words)? {
                hits.push(e, None);
            }
        }
    } else if is_japanese(text) {
        japanese(db, text, fetch, sources, words, &mut hits)?;
        if hits.list.is_empty() && text.chars().count() >= 2 {
            sentence(db, text, fetch, words, &mut hits)?;
        }
    } else {
        let kana: Vec<String> = [romaji::to_hiragana(text), romaji::to_katakana(text)]
            .into_iter()
            .flatten()
            .collect();
        if !kana.is_empty() {
            for e in db.lookup(&kana, sources, fetch)? {
                hits.push(e, None);
            }
        }
        for e in db.search(text, fetch, words)? {
            hits.push(e, None);
        }
        for k in &kana {
            japanese(db, k, fetch, sources, words, &mut hits)?;
        }
    }
    hits.list.truncate(limit);
    Ok(Outcome {
        hits: hits.list,
        sentences: Vec::new(),
        hint,
    })
}

/// Longest word per group when a sentence is cut up.
const PER_WORD: usize = 5;
/// Longest word tried when cutting a sentence up, in characters.
const LONGEST_WORD: usize = 8;

/// Cuts Japanese text into dictionary words, longest match from the left (inflected forms
/// count), and searches each word into its own group. Characters no word starts with are
/// skipped one at a time.
fn sentence(
    db: &Database,
    text: &str,
    limit: usize,
    sources: &[String],
    hits: &mut Hits,
) -> anyhow::Result<()> {
    let chars: Vec<char> = text.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let mut taken = 0;
        for len in (1..=LONGEST_WORD.min(chars.len() - i)).rev() {
            let word: String = chars[i..i + len].iter().collect();
            let mut probe = Hits {
                list: Vec::new(),
                seen: HashSet::new(),
                tags: Vec::new(),
                group: Some(word.clone()),
                learned: hits.learned,
            };
            for e in db.lookup(std::slice::from_ref(&word), sources, PER_WORD)? {
                probe.push(e, None);
            }
            deinflected(db, &word, PER_WORD, sources, &mut probe)?;
            if !probe.list.is_empty() {
                hits.group = Some(word);
                for hit in probe.list {
                    hits.push(hit.entry, hit.note);
                }
                taken = len;
                break;
            }
        }
        i += taken.max(1);
        if hits.list.len() >= limit {
            break;
        }
    }
    hits.group = None;
    Ok(())
}

/// `sources` for exact lookups (names included), `words` for the prefix search.
fn japanese(
    db: &Database,
    text: &str,
    limit: usize,
    sources: &[String],
    words: &[String],
    hits: &mut Hits,
) -> anyhow::Result<()> {
    for e in db.lookup(std::slice::from_ref(&text.to_string()), sources, limit)? {
        hits.push(e, None);
    }
    for e in db.search(text, limit, words)? {
        hits.push(e, None);
    }
    deinflected(db, text, limit, words, hits)
}

/// The dictionary forms `text` could be an inflection of, verified, with their notes.
fn deinflected(
    db: &Database,
    text: &str,
    limit: usize,
    sources: &[String],
    hits: &mut Hits,
) -> anyhow::Result<()> {
    let candidates = deinflect::deinflect(text);
    if candidates.is_empty() {
        return Ok(());
    }
    // One lookup for every candidate word (and the noun behind a する verb), then match up.
    let mut words: Vec<String> = candidates.iter().map(|c| c.word.clone()).collect();
    words.extend(
        candidates
            .iter()
            .filter(|c| c.kinds & deinflect::VS != 0)
            .filter_map(|c| c.word.strip_suffix("する").map(str::to_string)),
    );
    words.sort();
    words.dedup();
    let found = db.lookup(&words, sources, limit * 4)?;
    for c in &candidates {
        let noun = (c.kinds & deinflect::VS != 0)
            .then(|| c.word.strip_suffix("する"))
            .flatten();
        for e in &found {
            let has_form = |w: &str| e.kanji.iter().any(|k| k == w) || e.readings.iter().any(|r| r == w);
            let fits = (has_form(&c.word) || noun.is_some_and(has_form))
                && e.senses
                    .iter()
                    .flat_map(|s| &s.pos)
                    .any(|p| deinflect::pos_matches(p, c.kinds));
            if fits {
                let mut reasons = c.reasons.clone();
                reasons.reverse();
                let note = format!("{text} → {}: {}", e.headword(), reasons.join(", "));
                hits.push(e.clone(), Some(note));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dict::sources::{JMDICT, JMNEDICT};
    use crate::store::import;
    use std::path::Path;

    fn sample_db() -> Database {
        let db = Database::open_in_memory().unwrap();
        let sample = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/jmdict-sample.xml");
        import::import_file(&db, &JMDICT, &sample, &mut |_, _| {}).unwrap();
        db
    }

    fn only_jmdict() -> Vec<String> {
        vec!["jmdict".into()]
    }

    fn none() -> LearnedIndex {
        LearnedIndex::new()
    }

    #[test]
    fn known_filters_read_the_learned_index() {
        let db = sample_db();
        let mut learned = LearnedIndex::new();
        learned.insert((Kind::Vocabulary, "猫".into()), (6, 7)); // Master
        learned.insert((Kind::Vocabulary, "書く".into()), (4, 2)); // Apprentice
        learned.insert((Kind::Kanji, "猫".into()), (6, 9));
        let ids = |q: &str| ids_of(&run(&db, q, 10, &only_jmdict(), &[], &learned).unwrap());
        assert_eq!(ids("#known 猫"), [1467640, 2000003]); // both 猫 entries share the form
        assert!(ids("#known 書く").is_empty());
        assert_eq!(ids("#unknown 書く"), [1236120]);
        assert_eq!(ids("#wk-level-6 猫"), [1467640, 2000003]);
        assert!(ids("#wk-level-5 猫").is_empty());
        assert_eq!(ids("#kanji-known 猫"), [1467640, 2000003]); // 猫背 needs 背 too
        assert!(ids("#kanji-known 書く").is_empty());
        // On their own the tags list the learned words.
        assert_eq!(ids("#known"), [1467640, 2000003]);
        assert_eq!(ids("#wk-level-4"), [1236120]);
        assert!(ids("#wk-level-9").is_empty());
    }

    fn with_names() -> (Database, Vec<String>) {
        let db = sample_db();
        let names = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/jmnedict-sample.xml");
        import::import_file(&db, &JMNEDICT, &names, &mut |_, _| {}).unwrap();
        (db, vec!["jmdict".into(), "jmnedict".into()])
    }

    fn ids_of(outcome: &Outcome) -> Vec<i64> {
        outcome.hits.iter().map(|h| h.entry.id).collect()
    }

    #[test]
    fn jlpt_tag_filters_and_lists() {
        let db = sample_db();
        let lists = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/jlpt/jlpt-n5.csv");
        import::import_file(&db, &crate::dict::sources::JLPT, &lists, &mut |_, _| {}).unwrap();
        assert_eq!(
            ids_of(&run(&db, "#jlpt-n5", 10, &only_jmdict(), &[], &none()).unwrap()),
            [1236120, 1467640]
        );
        assert_eq!(
            ids_of(&run(&db, "#jlpt-n4", 10, &only_jmdict(), &[], &none()).unwrap()),
            [1000225]
        );
        assert_eq!(
            ids_of(&run(&db, "#jlpt-n5 猫", 10, &only_jmdict(), &[], &none()).unwrap()),
            [1467640]
        );
        assert!(
            run(&db, "#jlpt-n1 猫", 10, &only_jmdict(), &[], &none())
                .unwrap()
                .hits
                .is_empty()
        );
    }

    #[test]
    fn names_show_for_exact_matches_and_with_the_tag() {
        let (db, both) = with_names();
        // A prefix search stays free of names, an exact form finds them after the words.
        assert!(!ids_of(&run(&db, "さ", 10, &both, &[], &none()).unwrap()).contains(&5000001));
        assert_eq!(
            ids_of(&run(&db, "佐藤", 10, &both, &[], &none()).unwrap()),
            [5000001]
        );
        let cat = ids_of(&run(&db, "猫", 10, &both, &[], &none()).unwrap());
        assert_eq!(cat[0], 1467640);
        assert!(cat.contains(&5000004)); // the given name 猫, after every JMdict 猫
        // `#names` searches the names alone, prefixes included.
        assert_eq!(
            ids_of(&run(&db, "#names さ", 10, &both, &[], &none()).unwrap()),
            [5000001]
        );
        assert_eq!(
            ids_of(&run(&db, "#names tokyo", 10, &both, &[], &none()).unwrap()),
            [5000002]
        );
        // Without JMnedict enabled the tag says what is missing.
        let outcome = run(&db, "#names さ", 10, &only_jmdict(), &[], &none()).unwrap();
        assert!(outcome.hits.is_empty());
        assert!(outcome.hint.unwrap().contains("JMnedict"));
    }

    #[test]
    fn inflected_verb_finds_its_entry_with_the_chain() {
        let db = sample_db();
        let hits = run(&db, "書きました", 10, &only_jmdict(), &[], &none())
            .unwrap()
            .hits;
        assert_eq!(hits[0].entry.id, 1236120);
        assert_eq!(hits[0].note.as_deref(), Some("書きました → 書く: polite, past"));
        let hits = run(&db, "書かない", 10, &only_jmdict(), &[], &none())
            .unwrap()
            .hits;
        assert_eq!(hits[0].note.as_deref(), Some("書かない → 書く: negative"));
    }

    #[test]
    fn romaji_finds_readings_and_english_stays_a_gloss_search() {
        let db = sample_db();
        let hits = run(&db, "neko", 10, &only_jmdict(), &[], &none()).unwrap().hits;
        assert_eq!(hits[0].entry.id, 1467640);
        assert_eq!(hits[0].note, None);
        let hits = run(&db, "kakimashita", 10, &only_jmdict(), &[], &none())
            .unwrap()
            .hits;
        assert_eq!(hits[0].entry.id, 1236120);
        assert_eq!(hits[0].note.as_deref(), Some("かきました → 書く: polite, past"));
        let hits = run(&db, "cat", 10, &only_jmdict(), &[], &none()).unwrap().hits;
        assert_eq!(hits[0].entry.id, 1467640);
        assert!(hits.iter().all(|h| h.note.is_none()));
    }

    #[test]
    fn a_noun_is_not_offered_as_a_verb() {
        let db = sample_db();
        // 猫 + "る" would be a stem candidate; no ichidan entry 猫る exists, so nothing is added.
        let hits = run(&db, "猫", 10, &only_jmdict(), &[], &none()).unwrap().hits;
        assert!(hits.iter().all(|h| h.note.is_none()));
        assert!(
            run(&db, "", 10, &only_jmdict(), &[], &none())
                .unwrap()
                .hits
                .is_empty()
        );
    }

    fn ids(hits: &[Hit]) -> Vec<i64> {
        hits.iter().map(|h| h.entry.id).collect()
    }

    #[test]
    fn tags_filter_and_unknown_tags_hint() {
        let db = sample_db();
        let common = run(&db, "猫 #common", 10, &only_jmdict(), &[], &none()).unwrap();
        assert!(common.hits.iter().all(|h| h.entry.common));
        assert!(!common.hits.is_empty());
        let verbs = run(&db, "#verb 書", 10, &only_jmdict(), &[], &none())
            .unwrap()
            .hits;
        assert_eq!(ids(&verbs), [1236120]);
        assert!(
            run(&db, "#noun 書く", 10, &only_jmdict(), &[], &none())
                .unwrap()
                .hits
                .is_empty()
        );
        let kana = run(&db, "#kana cat", 10, &only_jmdict(), &[], &none())
            .unwrap()
            .hits;
        assert!(kana.iter().all(|h| {
            h.entry
                .senses
                .iter()
                .any(|s| s.misc.iter().any(|m| m.contains("kana")))
        }));
        let unknown = run(&db, "#bogus cat", 10, &only_jmdict(), &[], &none()).unwrap();
        assert!(
            unknown
                .hint
                .as_deref()
                .unwrap()
                .starts_with("Unknown tag #bogus.")
        );
        assert!(!unknown.hits.is_empty()); // the unknown tag is ignored, not a filter
    }

    #[test]
    fn quotes_and_wildcards() {
        let db = sample_db();
        assert_eq!(
            ids(&run(&db, "\"obvious\"", 10, &only_jmdict(), &[], &none())
                .unwrap()
                .hits),
            [1000225]
        );
        assert!(
            run(&db, "\"obvi\"", 10, &only_jmdict(), &[], &none())
                .unwrap()
                .hits
                .is_empty()
        );
        let exact = run(&db, "\"猫\"", 10, &only_jmdict(), &[], &none()).unwrap().hits;
        assert_eq!(exact[0].entry.id, 1467640);
        assert!(exact.iter().all(|h| h.entry.kanji.iter().any(|k| k == "猫")));
        assert_eq!(
            ids(&run(&db, "猫?", 10, &only_jmdict(), &[], &none()).unwrap().hits),
            [2000002]
        );
        let wild = run(&db, "c?t", 10, &only_jmdict(), &[], &none()).unwrap().hits;
        assert!(wild.iter().any(|h| h.entry.id == 1467640)); // "cat" inside a gloss
    }

    #[test]
    fn a_sentence_is_cut_into_words_with_groups() {
        let db = sample_db();
        let hits = run(&db, "猫背を書きました", 20, &only_jmdict(), &[], &none())
            .unwrap()
            .hits;
        let groups: Vec<&str> = hits.iter().filter_map(|h| h.group.as_deref()).collect();
        assert_eq!(groups, ["猫背", "書きました"]); // を is not in the fixture, so it is skipped
        assert_eq!(ids(&hits), [2000002, 1236120]);
        assert_eq!(hits[1].note.as_deref(), Some("書きました → 書く: polite, past"));
    }
}
