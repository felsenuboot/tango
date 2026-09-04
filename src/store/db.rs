//! The dictionary database: one SQLite file, plain SQL.
//!
//! Schema (v1): entries, forms (kanji and readings), senses, glosses. Search is prefix/LIKE based;
//! an FTS5 index over the glosses is the next step once the tables settle.
//!
//! A `Connection` is not `Sync`, so each thread opens its own `Database` (the import worker does).

use std::collections::HashMap;
use std::path::Path;

use anyhow::Context;
use rusqlite::{Connection, OptionalExtension, params, params_from_iter};

use crate::model::{Entry, Gloss, Sense};

pub const SCHEMA_VERSION: i64 = 1;

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS meta (key TEXT PRIMARY KEY, value TEXT);
CREATE TABLE IF NOT EXISTS entries (
    id INTEGER PRIMARY KEY,
    common INTEGER NOT NULL DEFAULT 0
);
CREATE TABLE IF NOT EXISTS forms (
    entry_id INTEGER NOT NULL REFERENCES entries(id) ON DELETE CASCADE,
    kind TEXT NOT NULL,      -- 'k' kanji form, 'r' reading
    pos INTEGER NOT NULL,    -- order within the entry
    text TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS senses (
    id INTEGER PRIMARY KEY,
    entry_id INTEGER NOT NULL REFERENCES entries(id) ON DELETE CASCADE,
    pos INTEGER NOT NULL,
    parts TEXT NOT NULL DEFAULT '',   -- parts of speech, newline-separated
    misc TEXT NOT NULL DEFAULT '',
    fields TEXT NOT NULL DEFAULT ''
);
CREATE TABLE IF NOT EXISTS glosses (
    sense_id INTEGER NOT NULL REFERENCES senses(id) ON DELETE CASCADE,
    lang TEXT NOT NULL,
    pos INTEGER NOT NULL,
    text TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS forms_text ON forms(text);
CREATE INDEX IF NOT EXISTS forms_entry ON forms(entry_id);
CREATE INDEX IF NOT EXISTS senses_entry ON senses(entry_id);
CREATE INDEX IF NOT EXISTS glosses_sense ON glosses(sense_id);
CREATE INDEX IF NOT EXISTS glosses_text ON glosses(text COLLATE NOCASE);
";

const SEP: &str = "\n";

/// True if the text contains kana or CJK ideographs, i.e. should be looked up as a headword.
pub fn is_japanese(text: &str) -> bool {
    text.chars()
        .any(|c| matches!(c, '\u{3040}'..='\u{30FF}' | '\u{4E00}'..='\u{9FFF}' | '\u{FF66}'..='\u{FF9F}'))
}

fn like_escape(text: &str) -> String {
    text.replace('\\', "\\\\").replace('%', "\\%").replace('_', "\\_")
}

fn split(joined: &str) -> Vec<String> {
    if joined.is_empty() {
        Vec::new()
    } else {
        joined.split(SEP).map(String::from).collect()
    }
}

pub struct Database {
    conn: Connection,
}

impl Database {
    pub fn open(path: &Path) -> anyhow::Result<Self> {
        let conn = Connection::open(path).with_context(|| format!("opening {}", path.display()))?;
        conn.execute_batch("PRAGMA foreign_keys = ON; PRAGMA journal_mode = WAL;")?;
        conn.execute_batch(SCHEMA)?;
        conn.execute(
            "INSERT OR IGNORE INTO meta VALUES ('schema', ?1)",
            params![SCHEMA_VERSION.to_string()],
        )?;
        Ok(Self { conn })
    }

    #[cfg_attr(not(test), allow(dead_code))] // used by tests and by the coming kanji view
    pub fn open_in_memory() -> anyhow::Result<Self> {
        let conn = Connection::open_in_memory()?;
        conn.execute_batch("PRAGMA foreign_keys = ON;")?;
        conn.execute_batch(SCHEMA)?;
        Ok(Self { conn })
    }

    // -- meta -------------------------------------------------------------------------------

    #[cfg_attr(not(test), allow(dead_code))] // used by tests and by the coming kanji view
    pub fn meta(&self, key: &str) -> anyhow::Result<Option<String>> {
        Ok(self
            .conn
            .query_row("SELECT value FROM meta WHERE key = ?1", params![key], |r| {
                r.get(0)
            })
            .optional()?)
    }

    pub fn set_meta(&self, key: &str, value: &str) -> anyhow::Result<()> {
        self.conn
            .execute("INSERT OR REPLACE INTO meta VALUES (?1, ?2)", params![key, value])?;
        Ok(())
    }

    pub fn entry_count(&self) -> anyhow::Result<i64> {
        Ok(self
            .conn
            .query_row("SELECT count(*) FROM entries", [], |r| r.get(0))?)
    }

    // -- import -----------------------------------------------------------------------------

    /// Drops every entry (the cascades take forms, senses and glosses with them).
    pub fn clear(&self) -> anyhow::Result<()> {
        self.conn.execute("DELETE FROM entries", [])?;
        Ok(())
    }

    /// Inserts a batch of entries in one transaction.
    pub fn insert(&self, entries: &[Entry]) -> anyhow::Result<()> {
        let tx = self.conn.unchecked_transaction()?;
        {
            let mut ins_entry = tx.prepare_cached("INSERT INTO entries (id, common) VALUES (?1, ?2)")?;
            let mut ins_form = tx.prepare_cached("INSERT INTO forms VALUES (?1, ?2, ?3, ?4)")?;
            let mut ins_sense = tx.prepare_cached(
                "INSERT INTO senses (entry_id, pos, parts, misc, fields) VALUES (?1, ?2, ?3, ?4, ?5)",
            )?;
            let mut ins_gloss = tx.prepare_cached("INSERT INTO glosses VALUES (?1, ?2, ?3, ?4)")?;
            for e in entries {
                ins_entry.execute(params![e.id, e.common as i64])?;
                for (i, k) in e.kanji.iter().enumerate() {
                    ins_form.execute(params![e.id, "k", i as i64, k])?;
                }
                for (i, r) in e.readings.iter().enumerate() {
                    ins_form.execute(params![e.id, "r", i as i64, r])?;
                }
                for (i, s) in e.senses.iter().enumerate() {
                    ins_sense.execute(params![
                        e.id,
                        i as i64,
                        s.pos.join(SEP),
                        s.misc.join(SEP),
                        s.fields.join(SEP)
                    ])?;
                    let sense_id = tx.last_insert_rowid();
                    for (j, g) in s.glosses.iter().enumerate() {
                        ins_gloss.execute(params![sense_id, g.lang, j as i64, g.text])?;
                    }
                }
            }
        }
        tx.commit()?;
        Ok(())
    }

    // -- lookup -----------------------------------------------------------------------------

    #[cfg_attr(not(test), allow(dead_code))] // used by tests and by the coming kanji view
    pub fn get(&self, id: i64) -> anyhow::Result<Option<Entry>> {
        Ok(self.load(&[id])?.pop())
    }

    /// Entries for `ids`, in that order.
    pub fn load(&self, ids: &[i64]) -> anyhow::Result<Vec<Entry>> {
        if ids.is_empty() {
            return Ok(Vec::new());
        }
        let marks = vec!["?"; ids.len()].join(",");
        let mut entries: Vec<Entry> = Vec::with_capacity(ids.len());
        let mut stmt = self
            .conn
            .prepare(&format!("SELECT id, common FROM entries WHERE id IN ({marks})"))?;
        for row in stmt.query_map(params_from_iter(ids), |r| {
            Ok((r.get::<_, i64>(0)?, r.get::<_, bool>(1)?))
        })? {
            let (id, common) = row?;
            entries.push(Entry {
                id,
                common,
                ..Entry::default()
            });
        }
        let index: HashMap<i64, usize> = entries.iter().enumerate().map(|(i, e)| (e.id, i)).collect();

        let mut stmt = self.conn.prepare(&format!(
            "SELECT entry_id, kind, text FROM forms WHERE entry_id IN ({marks}) ORDER BY entry_id, kind, pos"
        ))?;
        for row in stmt.query_map(params_from_iter(ids), |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
            ))
        })? {
            let (id, kind, text) = row?;
            if let Some(&i) = index.get(&id) {
                let e = &mut entries[i];
                if kind == "k" {
                    e.kanji.push(text)
                } else {
                    e.readings.push(text)
                }
            }
        }

        // (sense rowid, index into entries, index into that entry's senses)
        let mut sense_ids: Vec<(i64, usize, usize)> = Vec::new();
        let mut stmt = self.conn.prepare(&format!(
            "SELECT id, entry_id, parts, misc, fields FROM senses WHERE entry_id IN ({marks}) ORDER BY entry_id, pos"
        ))?;
        for row in stmt.query_map(params_from_iter(ids), |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, i64>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, String>(4)?,
            ))
        })? {
            let (sid, eid, parts, misc, fields) = row?;
            if let Some(&i) = index.get(&eid) {
                entries[i].senses.push(Sense {
                    pos: split(&parts),
                    misc: split(&misc),
                    fields: split(&fields),
                    glosses: Vec::new(),
                });
                sense_ids.push((sid, i, entries[i].senses.len() - 1));
            }
        }
        if !sense_ids.is_empty() {
            let smarks = vec!["?"; sense_ids.len()].join(",");
            let mut stmt = self.conn.prepare(&format!(
                "SELECT sense_id, lang, text FROM glosses WHERE sense_id IN ({smarks}) ORDER BY sense_id, pos"
            ))?;
            let sids = sense_ids.iter().map(|s| s.0);
            for row in stmt.query_map(params_from_iter(sids), |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                ))
            })? {
                let (sid, lang, text) = row?;
                if let Some(&(_, ei, si)) = sense_ids.iter().find(|s| s.0 == sid) {
                    entries[ei].senses[si].glosses.push(Gloss { lang, text });
                }
            }
        }

        // Back into the caller's order.
        let mut ordered = Vec::with_capacity(ids.len());
        for id in ids {
            if let Some(&i) = index.get(id) {
                ordered.push(entries[i].clone());
            }
        }
        Ok(ordered)
    }

    /// Headword/reading search for Japanese input, gloss search otherwise. Exact and common first.
    pub fn search(&self, query: &str, limit: usize) -> anyhow::Result<Vec<Entry>> {
        let q = query.trim();
        if q.is_empty() {
            return Ok(Vec::new());
        }
        let ids: Vec<i64> = if is_japanese(q) {
            let mut stmt = self.conn.prepare_cached(
                "SELECT f.entry_id, max(f.text = ?1) AS exact, e.common, min(length(f.text)) AS len
                 FROM forms f JOIN entries e ON e.id = f.entry_id
                 WHERE f.text LIKE ?2 ESCAPE '\\'
                 GROUP BY f.entry_id ORDER BY exact DESC, e.common DESC, len, f.entry_id LIMIT ?3",
            )?;
            let rows = stmt.query_map(params![q, format!("{}%", like_escape(q)), limit as i64], |r| {
                r.get(0)
            })?;
            rows.collect::<Result<_, _>>()?
        } else {
            let mut stmt = self.conn.prepare_cached(
                "SELECT s.entry_id, max(lower(g.text) = ?1) AS exact, e.common, min(length(g.text)) AS len
                 FROM glosses g JOIN senses s ON s.id = g.sense_id JOIN entries e ON e.id = s.entry_id
                 WHERE g.text LIKE ?2 ESCAPE '\\' OR g.text LIKE ?3 ESCAPE '\\'
                 GROUP BY s.entry_id ORDER BY exact DESC, e.common DESC, len, s.entry_id LIMIT ?4",
            )?;
            let esc = like_escape(q);
            let rows = stmt.query_map(
                params![
                    q.to_lowercase(),
                    format!("{esc}%"),
                    format!("% {esc}%"),
                    limit as i64
                ],
                |r| r.get(0),
            )?;
            rows.collect::<Result<_, _>>()?
        };
        self.load(&ids)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dict::jmdict;

    const SAMPLE: &str = include_str!("../../tests/fixtures/jmdict-sample.xml");

    fn sample_db() -> Database {
        let db = Database::open_in_memory().unwrap();
        let mut batch = Vec::new();
        jmdict::for_each_entry(SAMPLE.as_bytes(), |e| {
            batch.push(e);
            Ok(())
        })
        .unwrap();
        db.insert(&batch).unwrap();
        db
    }

    fn ids(entries: &[Entry]) -> Vec<i64> {
        entries.iter().map(|e| e.id).collect()
    }

    #[test]
    fn roundtrip_preserves_entry() {
        let db = sample_db();
        let mut original = None;
        jmdict::for_each_entry(SAMPLE.as_bytes(), |e| {
            if e.id == 1467640 {
                original = Some(e);
            }
            Ok(())
        })
        .unwrap();
        assert_eq!(db.get(1467640).unwrap(), original);
    }

    #[test]
    fn japanese_detection() {
        assert!(is_japanese("猫"));
        assert!(is_japanese("ねこ"));
        assert!(is_japanese("ﾈｺ"));
        assert!(!is_japanese("cat"));
        assert!(!is_japanese("Katze 2"));
    }

    #[test]
    fn headword_search_exact_and_common_first() {
        let db = sample_db();
        let found = ids(&db.search("猫", 100).unwrap());
        assert_eq!(&found[..2], &[1467640, 2000003]); // exact matches, the common one first
        assert!(found.contains(&2000002)); // 猫背 by prefix
    }

    #[test]
    fn reading_search() {
        let db = sample_db();
        assert_eq!(ids(&db.search("ねこ", 100).unwrap())[0], 1467640);
        assert_eq!(ids(&db.search("ねこじゃらし", 100).unwrap()), [2000001]);
    }

    #[test]
    fn gloss_search_english_and_german() {
        let db = sample_db();
        assert_eq!(db.search("cat", 100).unwrap()[0].id, 1467640);
        assert_eq!(db.search("Katze", 100).unwrap()[0].id, 1467640);
        assert_eq!(ids(&db.search("schreiben", 100).unwrap()), [1236120]);
        assert_eq!(db.search("write", 100).unwrap()[0].id, 1236120); // "to write": word-start match
    }

    #[test]
    fn gloss_search_is_case_insensitive_and_escapes_like() {
        let db = sample_db();
        assert_eq!(db.search("KATZE", 100).unwrap()[0].id, 1467640);
        assert!(db.search("100%", 100).unwrap().is_empty());
        assert!(db.search("   ", 100).unwrap().is_empty());
    }

    #[test]
    fn clear_drops_dependent_rows() {
        let db = sample_db();
        assert_eq!(db.entry_count().unwrap(), 6);
        db.clear().unwrap();
        assert_eq!(db.entry_count().unwrap(), 0);
        let glosses: i64 = db
            .conn
            .query_row("SELECT count(*) FROM glosses", [], |r| r.get(0))
            .unwrap();
        assert_eq!(glosses, 0);
    }
}
