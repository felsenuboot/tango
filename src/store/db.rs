//! The dictionary database: one SQLite file, plain SQL.
//!
//! Schema v3: `sources` (what is installed), `entries` (one row per dictionary entry, keyed by the
//! source and the source's own number), `forms` (kanji and readings), `senses`, `glosses`, and
//! `gloss_fts`, an FTS5 index over the gloss text. Headwords and readings are prefix searches on
//! the `forms_text` index; glosses go through FTS5 (a LIKE scan took half a second per keystroke
//! on the full JMdict, FTS5 answers in a few milliseconds).
//!
//! Everything here is derived data that can be imported again from the cached downloads. So a
//! schema version bump does not migrate: the tables are dropped and the app asks for an import.
//! User data (word lists, issue #10) will live in its own file for exactly that reason.
//!
//! A `Connection` is not `Sync`, so each thread opens its own `Database` (the import worker does).

use std::collections::HashMap;
use std::path::Path;

use anyhow::Context;
use rusqlite::types::Value;
use rusqlite::{Connection, OptionalExtension, params, params_from_iter};

use crate::model::{Entry, Gloss, Sense};

pub const SCHEMA_VERSION: i64 = 3;

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS meta (key TEXT PRIMARY KEY, value TEXT);
CREATE TABLE IF NOT EXISTS sources (
    id TEXT PRIMARY KEY,           -- dict::sources::Source::id
    version TEXT,                  -- what the file says about itself, e.g. the JMdict creation date
    imported TEXT NOT NULL,        -- ISO 8601, UTC
    entries INTEGER NOT NULL DEFAULT 0
);
CREATE TABLE IF NOT EXISTS entries (
    id INTEGER PRIMARY KEY,        -- internal; the source's own number is `seq`
    source TEXT NOT NULL REFERENCES sources(id) ON DELETE CASCADE,
    seq INTEGER NOT NULL,
    common INTEGER NOT NULL DEFAULT 0,
    UNIQUE (source, seq)
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
-- External-content index over glosses.text; `rebuild_gloss_index` fills it after an import.
-- remove_diacritics 2: \"uber\" finds \"über\".
CREATE VIRTUAL TABLE IF NOT EXISTS gloss_fts USING fts5(
    text, content='glosses', content_rowid='rowid', tokenize='unicode61 remove_diacritics 2'
);
CREATE INDEX IF NOT EXISTS entries_source ON entries(source);
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

/// An FTS5 query for typed text: every word quoted, the last one as a prefix, so "domestic ca"
/// matches "domestic cat". `None` when there is no word in it.
fn fts_query(text: &str) -> Option<String> {
    let words: Vec<String> = text
        .split_whitespace()
        .map(|w| format!("\"{}\"", w.replace('"', "\"\"")))
        .collect();
    if words.is_empty() {
        return None;
    }
    Some(format!("{}*", words.join(" ")))
}

fn split(joined: &str) -> Vec<String> {
    if joined.is_empty() {
        Vec::new()
    } else {
        joined.split(SEP).map(String::from).collect()
    }
}

/// What the database holds of one source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceStatus {
    pub id: String,
    /// What the file said about itself, e.g. the JMdict creation date.
    pub version: Option<String>,
    /// ISO 8601, UTC.
    pub imported: String,
    pub entries: i64,
}

pub struct Database {
    conn: Connection,
}

impl Database {
    pub fn open(path: &Path) -> anyhow::Result<Self> {
        let conn = Connection::open(path).with_context(|| format!("opening {}", path.display()))?;
        conn.execute_batch("PRAGMA foreign_keys = ON; PRAGMA journal_mode = WAL;")?;
        let db = Self { conn };
        db.prepare_schema()?;
        Ok(db)
    }

    #[cfg_attr(not(test), allow(dead_code))] // used by tests and by the coming kanji view
    pub fn open_in_memory() -> anyhow::Result<Self> {
        let conn = Connection::open_in_memory()?;
        conn.execute_batch("PRAGMA foreign_keys = ON;")?;
        let db = Self { conn };
        db.prepare_schema()?;
        Ok(db)
    }

    /// Creates the tables, or recreates them when the file is from another schema version
    /// (see the module docs for why that is a rebuild and not a migration). A file that has our
    /// tables but no valid schema row is a rebuild that was interrupted; it is wiped as well.
    fn prepare_schema(&self) -> anyhow::Result<()> {
        let tables = self.our_tables()?;
        if !tables.is_empty() {
            let stored = if tables.iter().any(|t| t == "meta") {
                self.meta("schema")?
            } else {
                None
            };
            if stored.as_deref() != Some(SCHEMA_VERSION.to_string().as_str()) {
                log::warn!(
                    "dictionary database has schema {}, this build uses {SCHEMA_VERSION}: rebuilding it, \
                     the dictionaries need importing again",
                    stored.as_deref().unwrap_or("?")
                );
                self.drop_everything(&tables)?;
            }
        }
        self.conn.execute_batch(SCHEMA)?;
        self.conn.execute(
            "INSERT OR IGNORE INTO meta VALUES ('schema', ?1)",
            params![SCHEMA_VERSION.to_string()],
        )?;
        Ok(())
    }

    /// Every table in the file, sorted so a virtual table comes before its shadow tables.
    fn our_tables(&self) -> anyhow::Result<Vec<String>> {
        let mut stmt = self.conn.prepare(
            "SELECT name FROM sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite_%' ORDER BY name",
        )?;
        let names = stmt.query_map([], |r| r.get(0))?.collect::<Result<_, _>>()?;
        Ok(names)
    }

    /// Drops `tables` in one transaction, so an interrupted rebuild leaves either everything or
    /// nothing. Foreign keys are off for it (a PRAGMA inside a transaction is a no-op).
    fn drop_everything(&self, tables: &[String]) -> anyhow::Result<()> {
        self.conn.execute_batch("PRAGMA foreign_keys = OFF")?;
        let result = (|| -> anyhow::Result<()> {
            let tx = self.conn.unchecked_transaction()?;
            for name in tables {
                tx.execute_batch(&format!("DROP TABLE IF EXISTS \"{name}\""))?;
            }
            tx.commit()?;
            Ok(())
        })();
        self.conn.execute_batch("PRAGMA foreign_keys = ON")?;
        result?;
        // The tables are gone either way; a VACUUM failing (no temp space) only costs disk.
        if let Err(e) = self.vacuum() {
            log::warn!("could not compact the rebuilt database: {e:#}");
        }
        Ok(())
    }

    /// Fills the FTS5 index from the glosses table. Imports and removals change the glosses in
    /// bulk, so the index is rebuilt afterwards instead of maintained row by row (11 s for the
    /// full JMdict).
    pub fn rebuild_gloss_index(&self) -> anyhow::Result<()> {
        self.conn
            .execute_batch("INSERT INTO gloss_fts(gloss_fts) VALUES('rebuild')")?;
        Ok(())
    }

    /// Gives the space of deleted rows back to the file system. Not inside a transaction.
    pub fn vacuum(&self) -> anyhow::Result<()> {
        self.conn.execute_batch("VACUUM")?;
        Ok(())
    }

    // -- meta and sources -------------------------------------------------------------------

    pub fn meta(&self, key: &str) -> anyhow::Result<Option<String>> {
        Ok(self
            .conn
            .query_row("SELECT value FROM meta WHERE key = ?1", params![key], |r| {
                r.get(0)
            })
            .optional()?)
    }

    pub fn entry_count(&self) -> anyhow::Result<i64> {
        Ok(self
            .conn
            .query_row("SELECT count(*) FROM entries", [], |r| r.get(0))?)
    }

    /// The installed sources, by id.
    pub fn sources(&self) -> anyhow::Result<Vec<SourceStatus>> {
        let mut stmt = self
            .conn
            .prepare("SELECT id, version, imported, entries FROM sources ORDER BY id")?;
        let rows = stmt.query_map([], |r| {
            Ok(SourceStatus {
                id: r.get(0)?,
                version: r.get(1)?,
                imported: r.get(2)?,
                entries: r.get(3)?,
            })
        })?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    /// Registers `id` for a fresh import, dropping whatever it had. Entries reference their
    /// source, so this comes before the first `insert`.
    pub fn begin_source(&self, id: &str, imported: &str) -> anyhow::Result<()> {
        self.conn
            .execute("DELETE FROM sources WHERE id = ?1", params![id])?;
        self.conn.execute(
            "INSERT INTO sources (id, imported, entries) VALUES (?1, ?2, 0)",
            params![id, imported],
        )?;
        Ok(())
    }

    pub fn finish_source(
        &self,
        id: &str,
        version: Option<&str>,
        imported: &str,
        entries: i64,
    ) -> anyhow::Result<()> {
        self.conn.execute(
            "UPDATE sources SET version = ?2, imported = ?3, entries = ?4 WHERE id = ?1",
            params![id, version, imported, entries],
        )?;
        Ok(())
    }

    /// Drops a source and, through the cascades, its entries, forms, senses and glosses.
    pub fn remove_source(&self, id: &str) -> anyhow::Result<()> {
        self.conn
            .execute("DELETE FROM sources WHERE id = ?1", params![id])?;
        Ok(())
    }

    // -- import -----------------------------------------------------------------------------

    /// Inserts a batch of entries in one transaction. Their `source` must be registered.
    pub fn insert(&self, entries: &[Entry]) -> anyhow::Result<()> {
        let tx = self.conn.unchecked_transaction()?;
        {
            let mut ins_entry =
                tx.prepare_cached("INSERT INTO entries (source, seq, common) VALUES (?1, ?2, ?3)")?;
            let mut ins_form = tx.prepare_cached("INSERT INTO forms VALUES (?1, ?2, ?3, ?4)")?;
            let mut ins_sense = tx.prepare_cached(
                "INSERT INTO senses (entry_id, pos, parts, misc, fields) VALUES (?1, ?2, ?3, ?4, ?5)",
            )?;
            let mut ins_gloss = tx.prepare_cached("INSERT INTO glosses VALUES (?1, ?2, ?3, ?4)")?;
            for e in entries {
                ins_entry.execute(params![e.source, e.id, e.common as i64])?;
                let entry_id = tx.last_insert_rowid();
                for (i, k) in e.kanji.iter().enumerate() {
                    ins_form.execute(params![entry_id, "k", i as i64, k])?;
                }
                for (i, r) in e.readings.iter().enumerate() {
                    ins_form.execute(params![entry_id, "r", i as i64, r])?;
                }
                for (i, s) in e.senses.iter().enumerate() {
                    ins_sense.execute(params![
                        entry_id,
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

    /// One entry by its source and the source's own number.
    #[cfg_attr(not(test), allow(dead_code))] // used by tests and by the coming kanji view
    pub fn get(&self, source: &str, seq: i64) -> anyhow::Result<Option<Entry>> {
        let id: Option<i64> = self
            .conn
            .query_row(
                "SELECT id FROM entries WHERE source = ?1 AND seq = ?2",
                params![source, seq],
                |r| r.get(0),
            )
            .optional()?;
        match id {
            Some(id) => Ok(self.load(&[id])?.pop()),
            None => Ok(None),
        }
    }

    /// Entries for the internal `ids`, in that order.
    fn load(&self, ids: &[i64]) -> anyhow::Result<Vec<Entry>> {
        if ids.is_empty() {
            return Ok(Vec::new());
        }
        let marks = vec!["?"; ids.len()].join(",");
        // (internal id, entry) in query order; `index` maps the internal id to the position.
        let mut entries: Vec<(i64, Entry)> = Vec::with_capacity(ids.len());
        let mut stmt = self.conn.prepare(&format!(
            "SELECT id, source, seq, common FROM entries WHERE id IN ({marks})"
        ))?;
        for row in stmt.query_map(params_from_iter(ids), |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, i64>(2)?,
                r.get::<_, bool>(3)?,
            ))
        })? {
            let (id, source, seq, common) = row?;
            entries.push((
                id,
                Entry {
                    source,
                    id: seq,
                    common,
                    ..Entry::default()
                },
            ));
        }
        let index: HashMap<i64, usize> = entries.iter().enumerate().map(|(i, e)| (e.0, i)).collect();

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
                let e = &mut entries[i].1;
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
                let e = &mut entries[i].1;
                e.senses.push(Sense {
                    pos: split(&parts),
                    misc: split(&misc),
                    fields: split(&fields),
                    glosses: Vec::new(),
                });
                sense_ids.push((sid, i, e.senses.len() - 1));
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
                    entries[ei].1.senses[si].glosses.push(Gloss { lang, text });
                }
            }
        }

        // Back into the caller's order.
        let mut ordered = Vec::with_capacity(ids.len());
        for id in ids {
            if let Some(&i) = index.get(id) {
                ordered.push(entries[i].1.clone());
            }
        }
        Ok(ordered)
    }

    /// Entries with a kanji form or reading equal to one of `texts`, common first, then by source
    /// order. Used to verify deinflection candidates and romaji readings.
    pub fn lookup(&self, texts: &[String], sources: &[String], limit: usize) -> anyhow::Result<Vec<Entry>> {
        if texts.is_empty() || sources.is_empty() {
            return Ok(Vec::new());
        }
        let priority = format!(
            "CASE e.source {} ELSE 99 END",
            (0..sources.len())
                .map(|i| format!("WHEN ? THEN {i}"))
                .collect::<Vec<_>>()
                .join(" ")
        );
        let mut values: Vec<Value> = sources.iter().map(|s| Value::Text(s.clone())).collect();
        values.extend(texts.iter().map(|t| Value::Text(t.clone())));
        values.extend(sources.iter().map(|s| Value::Text(s.clone())));
        values.push(Value::Integer(limit as i64));
        let sql = format!(
            "SELECT f.entry_id, e.common, {priority} AS prio
             FROM forms f JOIN entries e ON e.id = f.entry_id
             WHERE f.text IN ({}) AND e.source IN ({})
             GROUP BY f.entry_id ORDER BY e.common DESC, prio, f.entry_id LIMIT ?",
            vec!["?"; texts.len()].join(","),
            vec!["?"; sources.len()].join(",")
        );
        let mut stmt = self.conn.prepare_cached(&sql)?;
        let ids: Vec<i64> = stmt
            .query_map(params_from_iter(values), |r| r.get(0))?
            .collect::<Result<_, _>>()?;
        self.load(&ids)
    }

    /// Headword/reading search for Japanese input, gloss search otherwise, limited to `sources`.
    /// Exact matches first, then common words, then the sources in the order given, then short.
    pub fn search(&self, query: &str, limit: usize, sources: &[String]) -> anyhow::Result<Vec<Entry>> {
        let q = query.trim();
        if q.is_empty() || sources.is_empty() {
            return Ok(Vec::new());
        }
        // Positional `?` parameters fill in query order: first the ones in the SELECT list.
        let priority = format!(
            "CASE e.source {} ELSE 99 END",
            (0..sources.len())
                .map(|i| format!("WHEN ? THEN {i}"))
                .collect::<Vec<_>>()
                .join(" ")
        );
        let members = vec!["?"; sources.len()].join(",");
        let source_params: Vec<Value> = sources.iter().map(|s| Value::Text(s.clone())).collect();
        let mut values: Vec<Value> = Vec::new();
        let sql = if is_japanese(q) {
            values.push(Value::Text(q.to_string()));
            values.extend(source_params.iter().cloned());
            values.push(Value::Text(format!("{}%", like_escape(q))));
            values.extend(source_params.iter().cloned());
            values.push(Value::Integer(limit as i64));
            format!(
                "SELECT f.entry_id, max(f.text = ?) AS exact, e.common, {priority} AS prio,
                        min(length(f.text)) AS len
                 FROM forms f JOIN entries e ON e.id = f.entry_id
                 WHERE f.text LIKE ? ESCAPE '\\' AND e.source IN ({members})
                 GROUP BY f.entry_id ORDER BY exact DESC, e.common DESC, prio, len, f.entry_id LIMIT ?"
            )
        } else {
            let Some(fts) = fts_query(q) else {
                return Ok(Vec::new());
            };
            values.push(Value::Text(q.to_lowercase()));
            values.extend(source_params.iter().cloned());
            values.push(Value::Text(fts));
            values.extend(source_params.iter().cloned());
            values.push(Value::Integer(limit as i64));
            format!(
                "SELECT s.entry_id, max(lower(g.text) = ?) AS exact, e.common, {priority} AS prio,
                        min(length(g.text)) AS len
                 FROM gloss_fts f JOIN glosses g ON g.rowid = f.rowid
                      JOIN senses s ON s.id = g.sense_id JOIN entries e ON e.id = s.entry_id
                 WHERE gloss_fts MATCH ? AND e.source IN ({members})
                 GROUP BY s.entry_id ORDER BY exact DESC, e.common DESC, prio, len, s.entry_id LIMIT ?"
            )
        };
        let mut stmt = self.conn.prepare_cached(&sql)?;
        let ids: Vec<i64> = stmt
            .query_map(params_from_iter(values), |r| r.get(0))?
            .collect::<Result<_, _>>()?;
        self.load(&ids)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dict::jmdict;

    const SAMPLE: &str = include_str!("../../tests/fixtures/jmdict-sample.xml");
    const WHEN: &str = "2026-01-01T00:00:00Z";

    fn jmdict_only() -> Vec<String> {
        vec!["jmdict".into()]
    }

    fn sample_entries() -> Vec<Entry> {
        let mut out = Vec::new();
        jmdict::for_each_entry(SAMPLE.as_bytes(), |mut e| {
            e.source = "jmdict".into();
            out.push(e);
            Ok(())
        })
        .unwrap();
        out
    }

    fn sample_db() -> Database {
        let db = Database::open_in_memory().unwrap();
        db.begin_source("jmdict", WHEN).unwrap();
        db.insert(&sample_entries()).unwrap();
        db.finish_source("jmdict", Some("2024-01-01"), WHEN, 7).unwrap();
        db.rebuild_gloss_index().unwrap();
        db
    }

    fn ids(entries: &[Entry]) -> Vec<i64> {
        entries.iter().map(|e| e.id).collect()
    }

    #[test]
    fn roundtrip_preserves_entry() {
        let db = sample_db();
        let original = sample_entries().into_iter().find(|e| e.id == 1467640).unwrap();
        assert_eq!(db.get("jmdict", 1467640).unwrap(), Some(original));
        assert_eq!(db.get("jmdict", 1).unwrap(), None);
        assert_eq!(db.get("other", 1467640).unwrap(), None);
    }

    #[test]
    fn japanese_detection() {
        assert!(is_japanese("猫"));
        assert!(is_japanese("ねこ"));
        assert!(is_japanese("ﾈｺ"));
        assert!(!is_japanese("cat"));
    }

    #[test]
    fn headword_search_exact_and_common_first() {
        let db = sample_db();
        let found = db.search("猫", 100, &jmdict_only()).unwrap();
        assert_eq!(found[0].id, 1467640);
        assert!(ids(&found).contains(&2000002)); // 猫背, a prefix match
    }

    #[test]
    fn reading_search() {
        let db = sample_db();
        assert_eq!(db.search("ねこ", 100, &jmdict_only()).unwrap()[0].id, 1467640);
    }

    #[test]
    fn gloss_search_english_and_german() {
        let db = sample_db();
        assert_eq!(db.search("cat", 100, &jmdict_only()).unwrap()[0].id, 1467640);
        assert_eq!(db.search("Katze", 100, &jmdict_only()).unwrap()[0].id, 1467640);
        assert_eq!(db.search("write", 100, &jmdict_only()).unwrap()[0].id, 1236120); // "to write": word-start match
    }

    #[test]
    fn gloss_search_is_case_insensitive_and_takes_any_text() {
        let db = sample_db();
        assert_eq!(db.search("KATZE", 100, &jmdict_only()).unwrap()[0].id, 1467640);
        assert!(db.search("100%", 100, &jmdict_only()).unwrap().is_empty());
        assert!(
            db.search("\"quoted\" (parens)", 100, &jmdict_only())
                .unwrap()
                .is_empty()
        );
        assert_eq!(
            db.search("grüne borsten", 100, &jmdict_only()).unwrap()[0].id,
            2000001
        );
        assert_eq!(db.search("grune", 100, &jmdict_only()).unwrap()[0].id, 2000001); // diacritics folded
    }

    #[test]
    fn fts_queries_are_quoted() {
        assert_eq!(fts_query("cat").as_deref(), Some("\"cat\"*"));
        assert_eq!(fts_query("domestic ca").as_deref(), Some("\"domestic\" \"ca\"*"));
        assert_eq!(fts_query("a\"b").as_deref(), Some("\"a\"\"b\"*"));
        assert_eq!(fts_query("   "), None);
    }

    #[test]
    fn search_honours_the_source_list_and_its_order() {
        let db = sample_db();
        // A second source with a copy of the cat entry under its own numbering.
        db.begin_source("other", WHEN).unwrap();
        let mut cat = sample_entries().into_iter().find(|e| e.id == 1467640).unwrap();
        cat.source = "other".into();
        cat.id = 7;
        db.insert(&[cat]).unwrap();
        db.finish_source("other", None, WHEN, 1).unwrap();
        db.rebuild_gloss_index().unwrap();

        assert!(db.search("cat", 100, &[]).unwrap().is_empty());
        let only_other = db.search("cat", 100, &["other".into()]).unwrap();
        assert_eq!(
            only_other.iter().map(|e| e.source.as_str()).collect::<Vec<_>>(),
            ["other"]
        );
        let both = db.search("cat", 100, &["other".into(), "jmdict".into()]).unwrap();
        assert_eq!((both[0].source.as_str(), both[0].id), ("other", 7));
        let both = db.search("cat", 100, &["jmdict".into(), "other".into()]).unwrap();
        assert_eq!((both[0].source.as_str(), both[0].id), ("jmdict", 1467640));
    }

    #[test]
    fn lookup_is_exact() {
        let db = sample_db();
        let found = db
            .lookup(&["ねこ".into(), "書く".into()], &jmdict_only(), 10)
            .unwrap();
        assert_eq!(ids(&found), [1467640, 1236120]); // both common; source order, then id
        assert!(db.lookup(&["ね".into()], &jmdict_only(), 10).unwrap().is_empty());
    }

    #[test]
    fn sources_listing() {
        let db = sample_db();
        assert_eq!(
            db.sources().unwrap(),
            [SourceStatus {
                id: "jmdict".into(),
                version: Some("2024-01-01".into()),
                imported: WHEN.into(),
                entries: 7,
            }]
        );
    }

    #[test]
    fn remove_source_drops_dependent_rows() {
        let db = sample_db();
        assert_eq!(db.entry_count().unwrap(), 7);
        db.remove_source("jmdict").unwrap();
        assert_eq!(db.entry_count().unwrap(), 0);
        assert!(db.sources().unwrap().is_empty());
        let glosses: i64 = db
            .conn
            .query_row("SELECT count(*) FROM glosses", [], |r| r.get(0))
            .unwrap();
        assert_eq!(glosses, 0);
        db.vacuum().unwrap();
    }

    #[test]
    fn leftovers_of_an_interrupted_rebuild_are_wiped() {
        let dir = std::env::temp_dir().join(format!("tango-db-partial-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("partial.sqlite");
        {
            // meta and entries already dropped, glosses still there with stale rows.
            let conn = Connection::open(&path).unwrap();
            conn.execute_batch(
                "CREATE TABLE glosses (sense_id INTEGER, lang TEXT, pos INTEGER, text TEXT);
                 INSERT INTO glosses VALUES (1, 'eng', 0, 'stale');",
            )
            .unwrap();
        }
        let db = Database::open(&path).unwrap();
        let stale: i64 = db
            .conn
            .query_row("SELECT count(*) FROM glosses", [], |r| r.get(0))
            .unwrap();
        assert_eq!(stale, 0);
        assert_eq!(db.meta("schema").unwrap().as_deref(), Some("3"));
        drop(db);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn older_schema_is_rebuilt() {
        let dir = std::env::temp_dir().join(format!("tango-db-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("v1.sqlite");
        {
            let conn = Connection::open(&path).unwrap();
            conn.execute_batch(
                "CREATE TABLE meta (key TEXT PRIMARY KEY, value TEXT);
                 INSERT INTO meta VALUES ('schema', '1');
                 CREATE TABLE entries (id INTEGER PRIMARY KEY, common INTEGER NOT NULL DEFAULT 0);
                 INSERT INTO entries VALUES (1, 0);",
            )
            .unwrap();
        }
        let db = Database::open(&path).unwrap();
        assert_eq!(db.entry_count().unwrap(), 0);
        assert!(db.sources().unwrap().is_empty());
        db.begin_source("jmdict", WHEN).unwrap();
        db.insert(&sample_entries()).unwrap(); // the new columns exist
        assert_eq!(db.entry_count().unwrap(), 7);
        assert_eq!(db.meta("schema").unwrap().as_deref(), Some("3"));
        drop(db);
        std::fs::remove_dir_all(dir).unwrap();
    }
}
