//! The search pipeline: query → candidate forms (romaji to kana, deinflection) → database → hits.
//!
//! Japanese input: the prefix search as before, then every dictionary form the text could be an
//! inflection of, verified against the entries' parts of speech. Other input: the gloss search,
//! and if the text is romaji throughout, the same Japanese path for its kana readings, with
//! exact reading matches first (so "sake" finds 酒 before "for the sake of").

pub mod deinflect;
pub mod romaji;

use std::collections::HashSet;

use crate::model::Entry;
use crate::store::db::{Database, is_japanese};

/// One result: the entry, and how the query led to it when that is not obvious.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hit {
    pub entry: Entry,
    /// "食べました → 食べる: polite, past", for deinflected matches.
    pub note: Option<String>,
}

struct Hits {
    list: Vec<Hit>,
    seen: HashSet<(String, i64)>,
}

impl Hits {
    fn push(&mut self, entry: Entry, note: Option<String>) {
        if self.seen.insert((entry.source.clone(), entry.id)) {
            self.list.push(Hit { entry, note });
        }
    }
}

pub fn run(db: &Database, query: &str, limit: usize, sources: &[String]) -> anyhow::Result<Vec<Hit>> {
    let q = query.trim();
    let mut hits = Hits {
        list: Vec::new(),
        seen: HashSet::new(),
    };
    if q.is_empty() || sources.is_empty() {
        return Ok(hits.list);
    }
    if is_japanese(q) {
        japanese(db, q, limit, sources, &mut hits)?;
    } else {
        let kana: Vec<String> = [romaji::to_hiragana(q), romaji::to_katakana(q)]
            .into_iter()
            .flatten()
            .collect();
        if !kana.is_empty() {
            for e in db.lookup(&kana, sources, limit)? {
                hits.push(e, None);
            }
        }
        for e in db.search(q, limit, sources)? {
            hits.push(e, None);
        }
        for k in &kana {
            japanese(db, k, limit, sources, &mut hits)?;
        }
    }
    hits.list.truncate(limit);
    Ok(hits.list)
}

fn japanese(
    db: &Database,
    text: &str,
    limit: usize,
    sources: &[String],
    hits: &mut Hits,
) -> anyhow::Result<()> {
    for e in db.search(text, limit, sources)? {
        hits.push(e, None);
    }
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
    use crate::dict::sources::JMDICT;
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

    #[test]
    fn inflected_verb_finds_its_entry_with_the_chain() {
        let db = sample_db();
        let hits = run(&db, "書きました", 10, &only_jmdict()).unwrap();
        assert_eq!(hits[0].entry.id, 1236120);
        assert_eq!(hits[0].note.as_deref(), Some("書きました → 書く: polite, past"));
        let hits = run(&db, "書かない", 10, &only_jmdict()).unwrap();
        assert_eq!(hits[0].note.as_deref(), Some("書かない → 書く: negative"));
    }

    #[test]
    fn romaji_finds_readings_and_english_stays_a_gloss_search() {
        let db = sample_db();
        let hits = run(&db, "neko", 10, &only_jmdict()).unwrap();
        assert_eq!(hits[0].entry.id, 1467640);
        assert_eq!(hits[0].note, None);
        let hits = run(&db, "kakimashita", 10, &only_jmdict()).unwrap();
        assert_eq!(hits[0].entry.id, 1236120);
        assert_eq!(hits[0].note.as_deref(), Some("かきました → 書く: polite, past"));
        let hits = run(&db, "cat", 10, &only_jmdict()).unwrap();
        assert_eq!(hits[0].entry.id, 1467640);
        assert!(hits.iter().all(|h| h.note.is_none()));
    }

    #[test]
    fn a_noun_is_not_offered_as_a_verb() {
        let db = sample_db();
        // 猫 + "る" would be a stem candidate; no ichidan entry 猫る exists, so nothing is added.
        let hits = run(&db, "猫", 10, &only_jmdict()).unwrap();
        assert!(hits.iter().all(|h| h.note.is_none()));
        assert!(run(&db, "", 10, &only_jmdict()).unwrap().is_empty());
    }
}
