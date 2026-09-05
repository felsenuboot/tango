//! The dictionary database: one SQLite file, plain SQL.
//!
//! Schema v8: `sources` (what is installed), `entries` (one row per dictionary entry, keyed by the
//! source and the source's own number, with its pitch accent), `forms` (kanji and readings),
//! `senses`, `glosses`, the kanji tables (`kanji` from KANJIDIC2, `kanji_strokes` from KanjiVG,
//! `kanji_radicals` from RADKFILE), the Tatoeba tables (`sentences`, `sentence_links`,
//! `sentence_words` and the trigram index `sentence_fts`), and
//! `gloss_fts`, an FTS5 index over the gloss text. Headwords and readings are prefix searches on
//! the `forms_text` index; glosses go through FTS5 (a LIKE scan took half a second per keystroke
//! on the full JMdict, FTS5 answers in a few milliseconds).
//!
//! Everything here is derived data that can be imported again from the cached downloads. So a
//! schema version bump does not migrate: the tables are dropped and the app asks for an import.
//! User data (word lists, issue #10) will live in its own file for exactly that reason.
//!
//! A `Connection` is not `Sync`, so each thread opens its own `Database` (the import worker does).

use std::cell::Cell;
use std::collections::HashMap;
use std::path::Path;

use anyhow::Context;
use rusqlite::types::Value;
use rusqlite::{Connection, OptionalExtension, params, params_from_iter};

use crate::model::{Entry, Gloss, Kanji, Radical, Sense, Sentence, SentenceWord, Strokes};

pub const SCHEMA_VERSION: i64 = 8;

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
    pitch TEXT NOT NULL DEFAULT '',   -- accent numbers of the first reading, comma-separated
    UNIQUE (source, seq)
);
CREATE TABLE IF NOT EXISTS forms (
    entry_id INTEGER NOT NULL REFERENCES entries(id) ON DELETE CASCADE,
    kind TEXT NOT NULL,      -- 'k' kanji form, 'r' reading
    pos INTEGER NOT NULL,    -- order within the entry
    text TEXT NOT NULL,
    info TEXT NOT NULL DEFAULT '',    -- ke_inf / re_inf tags, newline-separated
    restr TEXT NOT NULL DEFAULT ''    -- re_restr: the kanji forms a reading goes with
);
CREATE TABLE IF NOT EXISTS senses (
    id INTEGER PRIMARY KEY,
    entry_id INTEGER NOT NULL REFERENCES entries(id) ON DELETE CASCADE,
    pos INTEGER NOT NULL,
    parts TEXT NOT NULL DEFAULT '',   -- parts of speech, newline-separated
    misc TEXT NOT NULL DEFAULT '',
    fields TEXT NOT NULL DEFAULT '',
    info TEXT NOT NULL DEFAULT '',    -- s_inf notes
    dial TEXT NOT NULL DEFAULT '',
    origin TEXT NOT NULL DEFAULT '',  -- lsource, already readable
    xref TEXT NOT NULL DEFAULT '',
    ant TEXT NOT NULL DEFAULT '',
    stag TEXT NOT NULL DEFAULT ''     -- stagk / stagr
);
CREATE TABLE IF NOT EXISTS glosses (
    sense_id INTEGER NOT NULL REFERENCES senses(id) ON DELETE CASCADE,
    lang TEXT NOT NULL,
    pos INTEGER NOT NULL,
    text TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS kanji (
    literal TEXT PRIMARY KEY,
    grade INTEGER, strokes INTEGER NOT NULL, freq INTEGER, jlpt INTEGER, radical INTEGER NOT NULL,
    onyomi TEXT NOT NULL DEFAULT '',   -- newline-separated, like the sense columns
    kunyomi TEXT NOT NULL DEFAULT '',
    nanori TEXT NOT NULL DEFAULT '',
    meanings TEXT NOT NULL DEFAULT ''  -- one line per meaning: language, tab, text
);
CREATE TABLE IF NOT EXISTS kanji_strokes (
    literal TEXT PRIMARY KEY,
    paths TEXT NOT NULL               -- SVG path data in stroke order, newline-separated
);
CREATE TABLE IF NOT EXISTS kanji_radicals (
    radical TEXT NOT NULL,
    strokes INTEGER NOT NULL,
    literal TEXT NOT NULL,
    PRIMARY KEY (radical, literal)
);
CREATE INDEX IF NOT EXISTS kanji_radicals_literal ON kanji_radicals(literal);
-- JLPT level per JMdict number, from the optional lists.
CREATE TABLE IF NOT EXISTS jlpt (
    seq INTEGER PRIMARY KEY,
    level INTEGER NOT NULL
);
-- External-content index over glosses.text; `rebuild_gloss_index` fills it after an import.
-- remove_diacritics 2: \"uber\" finds \"über\".
CREATE VIRTUAL TABLE IF NOT EXISTS gloss_fts USING fts5(
    text, content='glosses', content_rowid='rowid', tokenize='unicode61 remove_diacritics 2'
);
-- Tatoeba: Japanese sentences and their translations in one table, the pairs in `sentence_links`,
-- and per Japanese sentence the JMdict words of the Tanaka corpus index. The trigram index finds
-- substrings, which is what a search in text without spaces needs.
CREATE TABLE IF NOT EXISTS sentences (
    id INTEGER PRIMARY KEY,           -- Tatoeba's number
    lang TEXT NOT NULL,               -- ISO 639-3: jpn, eng, deu
    text TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS sentence_links (
    jpn INTEGER NOT NULL,
    other INTEGER NOT NULL,
    PRIMARY KEY (jpn, other)
) WITHOUT ROWID;
CREATE INDEX IF NOT EXISTS sentence_links_other ON sentence_links(other);
CREATE TABLE IF NOT EXISTS sentence_words (
    sentence INTEGER NOT NULL,
    headword TEXT NOT NULL,
    reading TEXT,
    sense INTEGER,
    seq INTEGER,                      -- JMdict ent_seq when the index gives one
    surface TEXT,
    good INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX IF NOT EXISTS sentence_words_headword ON sentence_words(headword);
CREATE INDEX IF NOT EXISTS sentence_words_seq ON sentence_words(seq);
CREATE INDEX IF NOT EXISTS sentence_words_sentence ON sentence_words(sentence);
CREATE VIRTUAL TABLE IF NOT EXISTS sentence_fts USING fts5(
    text, content='sentences', content_rowid='id', tokenize='trigram'
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

/// GLOB is case-sensitive, so a `text GLOB 'x*'` prefix search uses the plain index (LIKE would
/// need a NOCASE column and otherwise scans). Its metacharacters are bracketed away.
/// The form queries also say `INDEXED BY forms_text`: without statistics the planner prefers
/// walking `entries` by source and probing forms per entry, 100 ms instead of under one.
fn glob_escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '*' | '?' | '[' | ']' => {
                out.push('[');
                out.push(c);
                out.push(']');
            }
            c => out.push(c),
        }
    }
    out
}

/// `CASE e.source WHEN ? THEN 0 WHEN ? THEN 1 … END`: ranks by the order of the source list.
fn source_priority(n: usize) -> String {
    format!(
        "CASE e.source {} ELSE 99 END",
        (0..n)
            .map(|i| format!("WHEN ? THEN {i}"))
            .collect::<Vec<_>>()
            .join(" ")
    )
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
    /// The file had an older schema and was emptied on open; the cached downloads want importing.
    rebuilt: Cell<bool>,
}

impl Database {
    pub fn open(path: &Path) -> anyhow::Result<Self> {
        let conn = Connection::open(path).with_context(|| format!("opening {}", path.display()))?;
        // A second writer (two import jobs, say) waits up to five seconds instead of failing at once.
        conn.execute_batch(
            "PRAGMA foreign_keys = ON; PRAGMA journal_mode = WAL; PRAGMA busy_timeout = 5000;",
        )?;
        let db = Self {
            conn,
            rebuilt: Cell::new(false),
        };
        db.prepare_schema()?;
        db.drop_interrupted_imports()?;
        Ok(db)
    }

    #[cfg_attr(not(test), allow(dead_code))] // used by tests and by the coming kanji view
    pub fn open_in_memory() -> anyhow::Result<Self> {
        let conn = Connection::open_in_memory()?;
        conn.execute_batch("PRAGMA foreign_keys = ON;")?;
        let db = Self {
            conn,
            rebuilt: Cell::new(false),
        };
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
                self.rebuilt.set(true);
            }
        }
        self.conn.execute_batch(SCHEMA)?;
        self.conn.execute(
            "INSERT OR IGNORE INTO meta VALUES ('schema', ?1)",
            params![SCHEMA_VERSION.to_string()],
        )?;
        Ok(())
    }

    /// Whether `open` found an older schema and emptied the file.
    pub fn was_rebuilt(&self) -> bool {
        self.rebuilt.get()
    }

    /// A source row with no entries is an import the app was closed in the middle of (the count
    /// is written last). Its leftovers go, so it shows as not installed rather than empty.
    fn drop_interrupted_imports(&self) -> anyhow::Result<()> {
        let ids: Vec<String> = self
            .conn
            .prepare("SELECT id FROM sources WHERE entries = 0")?
            .query_map([], |r| r.get(0))?
            .collect::<Result<_, _>>()?;
        for id in ids {
            log::warn!("the {id} import was interrupted: dropping what it left behind");
            self.remove_source(&id)?;
        }
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

    /// Drops a source and, through the cascades, its entries, forms, senses and glosses; the
    /// kanji and sentence sources own their tables instead.
    pub fn remove_source(&self, id: &str) -> anyhow::Result<()> {
        let tables: &[&str] = match id {
            "kanjidic" => &["kanji"],
            "kanjivg" => &["kanji_strokes"],
            "radkfile" => &["kanji_radicals"],
            "jlpt" => &["jlpt"],
            "tatoeba" => &["sentence_words", "sentence_links", "sentences"],
            _ => &[],
        };
        for table in tables {
            self.conn.execute_batch(&format!("DELETE FROM {table}"))?;
        }
        if id == "tatoeba" {
            self.rebuild_sentence_index()?;
        }
        self.conn
            .execute("DELETE FROM sources WHERE id = ?1", params![id])?;
        Ok(())
    }

    // -- kanji ------------------------------------------------------------------------------

    pub fn insert_kanji(&self, kanji: &[Kanji]) -> anyhow::Result<()> {
        let tx = self.conn.unchecked_transaction()?;
        {
            let mut ins = tx.prepare_cached(
                "INSERT OR REPLACE INTO kanji
                 (literal, grade, strokes, freq, jlpt, radical, onyomi, kunyomi, nanori, meanings)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            )?;
            for k in kanji {
                let meanings = k
                    .meanings
                    .iter()
                    .map(|m| format!("{}\t{}", m.lang, m.text))
                    .collect::<Vec<_>>()
                    .join(SEP);
                ins.execute(params![
                    k.literal.to_string(),
                    k.grade,
                    k.strokes,
                    k.freq,
                    k.jlpt,
                    k.radical,
                    k.on.join(SEP),
                    k.kun.join(SEP),
                    k.nanori.join(SEP),
                    meanings
                ])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    pub fn insert_strokes(&self, strokes: &[Strokes]) -> anyhow::Result<()> {
        let tx = self.conn.unchecked_transaction()?;
        {
            let mut ins =
                tx.prepare_cached("INSERT OR REPLACE INTO kanji_strokes (literal, paths) VALUES (?1, ?2)")?;
            for s in strokes {
                ins.execute(params![s.literal.to_string(), s.paths.join(SEP)])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    pub fn insert_radicals(&self, radicals: &[Radical]) -> anyhow::Result<()> {
        let tx = self.conn.unchecked_transaction()?;
        {
            let mut ins = tx.prepare_cached(
                "INSERT OR REPLACE INTO kanji_radicals (radical, strokes, literal) VALUES (?1, ?2, ?3)",
            )?;
            for r in radicals {
                for k in &r.kanji {
                    ins.execute(params![r.radical, r.strokes, k.to_string()])?;
                }
            }
        }
        tx.commit()?;
        Ok(())
    }

    pub fn kanji(&self, literal: char) -> anyhow::Result<Option<Kanji>> {
        let row = self
            .conn
            .query_row(
                "SELECT grade, strokes, freq, jlpt, radical, onyomi, kunyomi, nanori, meanings
                 FROM kanji WHERE literal = ?1",
                params![literal.to_string()],
                |r| {
                    Ok(Kanji {
                        literal,
                        grade: r.get(0)?,
                        strokes: r.get(1)?,
                        freq: r.get(2)?,
                        jlpt: r.get(3)?,
                        radical: r.get(4)?,
                        on: split(&r.get::<_, String>(5)?),
                        kun: split(&r.get::<_, String>(6)?),
                        nanori: split(&r.get::<_, String>(7)?),
                        meanings: split(&r.get::<_, String>(8)?)
                            .into_iter()
                            .filter_map(|line| {
                                let (lang, text) = line.split_once('\t')?;
                                Some(Gloss {
                                    lang: lang.to_string(),
                                    text: text.to_string(),
                                })
                            })
                            .collect(),
                    })
                },
            )
            .optional()?;
        Ok(row)
    }

    /// SVG path data in stroke order, if KanjiVG has the character.
    pub fn strokes(&self, literal: char) -> anyhow::Result<Option<Vec<String>>> {
        let paths: Option<String> = self
            .conn
            .query_row(
                "SELECT paths FROM kanji_strokes WHERE literal = ?1",
                params![literal.to_string()],
                |r| r.get(0),
            )
            .optional()?;
        Ok(paths.map(|p| split(&p)))
    }

    /// The radicals a kanji is made of, by stroke count.
    pub fn radicals_of(&self, literal: char) -> anyhow::Result<Vec<String>> {
        let mut stmt = self.conn.prepare_cached(
            "SELECT radical FROM kanji_radicals WHERE literal = ?1 ORDER BY strokes, radical",
        )?;
        let rows = stmt.query_map(params![literal.to_string()], |r| r.get(0))?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    /// Every radical with its stroke count and how many kanji contain it.
    pub fn radicals(&self) -> anyhow::Result<Vec<(String, u8, i64)>> {
        let mut stmt = self.conn.prepare_cached(
            "SELECT radical, strokes, count(*) FROM kanji_radicals GROUP BY radical, strokes ORDER BY strokes, radical",
        )?;
        let rows = stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    /// Kanji that contain every one of `radicals` (and have `strokes` strokes, if given), fewest
    /// strokes and most frequent first.
    pub fn kanji_with_radicals(&self, radicals: &[String], strokes: Option<u8>) -> anyhow::Result<Vec<char>> {
        if radicals.is_empty() {
            return Ok(Vec::new());
        }
        let marks = vec!["?"; radicals.len()].join(",");
        let stroke_filter = if strokes.is_some() {
            "AND k.strokes = ?"
        } else {
            ""
        };
        let sql = format!(
            "SELECT r.literal FROM kanji_radicals r LEFT JOIN kanji k ON k.literal = r.literal
             WHERE r.radical IN ({marks}) {stroke_filter}
             GROUP BY r.literal HAVING count(DISTINCT r.radical) = ?
             ORDER BY coalesce(k.strokes, 99), coalesce(k.freq, 9999), r.literal"
        );
        let mut values: Vec<Value> = radicals.iter().map(|r| Value::Text(r.clone())).collect();
        if let Some(n) = strokes {
            values.push(Value::Integer(i64::from(n)));
        }
        values.push(Value::Integer(radicals.len() as i64));
        let mut stmt = self.conn.prepare_cached(&sql)?;
        let rows = stmt.query_map(params_from_iter(values), |r| r.get::<_, String>(0))?;
        Ok(rows
            .collect::<Result<Vec<_>, _>>()?
            .into_iter()
            .filter_map(|s| s.chars().next())
            .collect())
    }

    /// The radicals that occur together with all of `radicals` in some kanji: the ones still
    /// worth clicking. Every radical when nothing is selected.
    pub fn radicals_compatible(&self, radicals: &[String]) -> anyhow::Result<Vec<String>> {
        if radicals.is_empty() {
            return Ok(self.radicals()?.into_iter().map(|(r, _, _)| r).collect());
        }
        let marks = vec!["?"; radicals.len()].join(",");
        let sql = format!(
            "SELECT DISTINCT other.radical FROM kanji_radicals other
             WHERE other.literal IN (
                 SELECT r.literal FROM kanji_radicals r WHERE r.radical IN ({marks})
                 GROUP BY r.literal HAVING count(DISTINCT r.radical) = ?)
             ORDER BY other.radical"
        );
        let mut values: Vec<Value> = radicals.iter().map(|r| Value::Text(r.clone())).collect();
        values.push(Value::Integer(radicals.len() as i64));
        let mut stmt = self.conn.prepare_cached(&sql)?;
        let rows = stmt.query_map(params_from_iter(values), |r| r.get(0))?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    /// Entries whose kanji forms contain `literal`, common first: "words containing this kanji".
    /// A scan over the forms (no substring index), so it is used on demand, not while typing.
    pub fn words_with(&self, literal: char, limit: usize, sources: &[String]) -> anyhow::Result<Vec<Entry>> {
        if sources.is_empty() {
            return Ok(Vec::new());
        }
        let priority = source_priority(sources.len());
        let members = vec!["?"; sources.len()].join(",");
        let mut values: Vec<Value> = sources.iter().map(|s| Value::Text(s.clone())).collect();
        values.push(Value::Text(format!("*{}*", glob_escape(&literal.to_string()))));
        values.extend(sources.iter().map(|s| Value::Text(s.clone())));
        values.push(Value::Integer(limit as i64));
        let sql = format!(
            "SELECT f.entry_id, e.common, {priority} AS prio, min(length(f.text)) AS len
             FROM forms f INDEXED BY forms_text JOIN entries e ON e.id = f.entry_id
             WHERE f.kind = 'k' AND f.text GLOB ? AND e.source IN ({members})
             GROUP BY f.entry_id ORDER BY e.common DESC, prio, len, f.entry_id LIMIT ?"
        );
        let mut stmt = self.conn.prepare_cached(&sql)?;
        let ids: Vec<i64> = stmt
            .query_map(params_from_iter(values), |r| r.get(0))?
            .collect::<Result<_, _>>()?;
        self.load(&ids)
    }

    #[cfg_attr(not(test), allow(dead_code))] // tests, and the coming JLPT chips
    pub fn has_kanji_data(&self) -> anyhow::Result<bool> {
        let n: i64 = self
            .conn
            .query_row("SELECT count(*) FROM kanji", [], |r| r.get(0))?;
        Ok(n > 0)
    }

    // -- JLPT -------------------------------------------------------------------------------

    /// Inserts `(JMdict number, level)` rows; a word on several lists keeps the easiest level.
    pub fn insert_jlpt(&self, rows: &[(i64, u8)]) -> anyhow::Result<()> {
        let tx = self.conn.unchecked_transaction()?;
        {
            let mut ins = tx.prepare_cached(
                "INSERT INTO jlpt (seq, level) VALUES (?1, ?2)
                 ON CONFLICT(seq) DO UPDATE SET level = max(level, excluded.level)",
            )?;
            for (seq, level) in rows {
                ins.execute(params![seq, i64::from(*level)])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    /// The JMdict words of one JLPT level, common first.
    pub fn jlpt_words(&self, level: u8, limit: usize, sources: &[String]) -> anyhow::Result<Vec<Entry>> {
        if !sources.iter().any(|s| s == "jmdict") {
            return Ok(Vec::new());
        }
        let mut stmt = self.conn.prepare_cached(
            "SELECT e.id FROM jlpt j JOIN entries e ON e.seq = j.seq AND e.source = 'jmdict'
             WHERE j.level = ?1 ORDER BY e.common DESC, e.seq LIMIT ?2",
        )?;
        let ids: Vec<i64> = stmt
            .query_map(params![i64::from(level), limit as i64], |r| r.get(0))?
            .collect::<Result<_, _>>()?;
        self.load(&ids)
    }

    // -- sentences --------------------------------------------------------------------------

    /// Inserts `(id, language, text)` rows in one transaction.
    pub fn insert_sentences(&self, rows: &[(i64, &str, String)]) -> anyhow::Result<()> {
        let tx = self.conn.unchecked_transaction()?;
        {
            let mut ins =
                tx.prepare_cached("INSERT OR REPLACE INTO sentences (id, lang, text) VALUES (?1, ?2, ?3)")?;
            for (id, lang, text) in rows {
                ins.execute(params![id, lang, text])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    /// Inserts `(Japanese id, translation id)` pairs in one transaction.
    pub fn insert_sentence_links(&self, links: &[(i64, i64)]) -> anyhow::Result<()> {
        let tx = self.conn.unchecked_transaction()?;
        {
            let mut ins =
                tx.prepare_cached("INSERT OR IGNORE INTO sentence_links (jpn, other) VALUES (?1, ?2)")?;
            for (jpn, other) in links {
                ins.execute(params![jpn, other])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    /// Inserts the indexed words of sentences, `(sentence id, word)`, in one transaction.
    pub fn insert_sentence_words(&self, words: &[(i64, SentenceWord)]) -> anyhow::Result<()> {
        let tx = self.conn.unchecked_transaction()?;
        {
            let mut ins = tx.prepare_cached(
                "INSERT INTO sentence_words (sentence, headword, reading, sense, seq, surface, good)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            )?;
            for (sentence, w) in words {
                ins.execute(params![
                    sentence, w.headword, w.reading, w.sense, w.seq, w.surface, w.good
                ])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    /// Fills the trigram index from the sentences table, after an import or a removal.
    pub fn rebuild_sentence_index(&self) -> anyhow::Result<()> {
        self.conn
            .execute_batch("INSERT INTO sentence_fts(sentence_fts) VALUES('rebuild')")?;
        Ok(())
    }

    pub fn has_sentences(&self) -> anyhow::Result<bool> {
        let n: i64 = self
            .conn
            .query_row("SELECT count(*) FROM sentences WHERE lang = 'jpn'", [], |r| {
                r.get(0)
            })?;
        Ok(n > 0)
    }

    /// Example sentences for `entry`: the ones the corpus index ties to its JMdict number, or to
    /// one of its forms (with the reading, when the index gives one). Good examples first, then
    /// short ones. `surface` is the word as it stands in the sentence, the headword when the index
    /// gives nothing else. `langs` are Tatoeba language codes, preferred first, for the translations.
    /// Returns the sentences and the total count.
    pub fn examples(
        &self,
        entry: &Entry,
        langs: &[String],
        limit: usize,
    ) -> anyhow::Result<(Vec<Sentence>, usize)> {
        let heads: Vec<&String> = entry.kanji.iter().chain(&entry.readings).collect();
        if heads.is_empty() {
            return Ok((Vec::new(), 0));
        }
        let seq = if entry.source == "jmdict" { entry.id } else { -1 };
        let readings = if entry.readings.is_empty() {
            "1".to_string()
        } else {
            format!("w.reading IN ({})", vec!["?"; entry.readings.len()].join(","))
        };
        // Two index lookups joined by OR (seq, headword). A `seq IS NULL` guard on the second
        // branch would make SQLite walk the seq index over the million rows without one.
        let condition = format!(
            "(w.seq = ? OR (w.headword IN ({}) AND (w.reading IS NULL OR {readings})))",
            vec!["?"; heads.len()].join(",")
        );
        let mut values: Vec<Value> = vec![Value::Integer(seq)];
        values.extend(heads.iter().map(|h| Value::Text(h.to_string())));
        values.extend(entry.readings.iter().map(|r| Value::Text(r.clone())));
        let total: i64 = self.conn.query_row(
            &format!("SELECT count(DISTINCT w.sentence) FROM sentence_words w WHERE {condition}"),
            params_from_iter(values.iter()),
            |r| r.get(0),
        )?;
        values.push(Value::Integer(limit as i64));
        let sql = format!(
            "SELECT w.sentence, s.text, max(w.good), coalesce(w.surface, w.headword)
             FROM sentence_words w JOIN sentences s ON s.id = w.sentence
             WHERE {condition}
             GROUP BY w.sentence ORDER BY max(w.good) DESC, length(s.text), w.sentence LIMIT ?"
        );
        let mut stmt = self.conn.prepare_cached(&sql)?;
        let mut sentences: Vec<Sentence> = stmt
            .query_map(params_from_iter(values), |r| {
                Ok(Sentence {
                    id: r.get(0)?,
                    text: r.get(1)?,
                    good: r.get::<_, i64>(2)? != 0,
                    surface: r.get(3)?,
                    translations: Vec::new(),
                })
            })?
            .collect::<Result<_, _>>()?;
        self.attach_translations(&mut sentences, langs)?;
        Ok((sentences, total as usize))
    }

    /// Sentences containing `text`, in Japanese or in a translation: the Japanese sentence either
    /// way, shortest first. Three characters and more go through the trigram index, less is a scan.
    pub fn search_sentences(
        &self,
        text: &str,
        langs: &[String],
        limit: usize,
    ) -> anyhow::Result<Vec<Sentence>> {
        let text = text.trim();
        if text.is_empty() {
            return Ok(Vec::new());
        }
        let (filter, value) = if text.chars().count() >= 3 {
            (
                "s.id IN (SELECT rowid FROM sentence_fts WHERE sentence_fts MATCH ?)",
                format!("\"{}\"", text.replace('"', "\"\"")),
            )
        } else {
            ("s.text LIKE ? ESCAPE '\\'", format!("%{}%", like_escape(text)))
        };
        let sql = format!(
            "SELECT DISTINCT j.id, j.text FROM sentences s
             LEFT JOIN sentence_links l ON l.other = s.id
             JOIN sentences j ON j.id = CASE WHEN s.lang = 'jpn' THEN s.id ELSE l.jpn END
             WHERE {filter} ORDER BY length(j.text), j.id LIMIT ?"
        );
        let mut stmt = self.conn.prepare_cached(&sql)?;
        let mut sentences: Vec<Sentence> = stmt
            .query_map(params![value, limit as i64], |r| {
                Ok(Sentence {
                    id: r.get(0)?,
                    text: r.get(1)?,
                    ..Default::default()
                })
            })?
            .collect::<Result<_, _>>()?;
        self.attach_translations(&mut sentences, langs)?;
        Ok(sentences)
    }

    /// The indexed words of a Japanese sentence, in sentence order.
    pub fn sentence_words(&self, id: i64) -> anyhow::Result<Vec<SentenceWord>> {
        let mut stmt = self.conn.prepare_cached(
            "SELECT headword, reading, sense, seq, surface, good FROM sentence_words
             WHERE sentence = ?1 ORDER BY rowid",
        )?;
        let words = stmt.query_map(params![id], |r| {
            Ok(SentenceWord {
                headword: r.get(0)?,
                reading: r.get(1)?,
                sense: r.get(2)?,
                seq: r.get(3)?,
                surface: r.get(4)?,
                good: r.get::<_, i64>(5)? != 0,
            })
        })?;
        Ok(words.collect::<Result<_, _>>()?)
    }

    /// One translation per language in `langs`, in that order; the first there is when none of
    /// those languages has one.
    fn attach_translations(&self, sentences: &mut [Sentence], langs: &[String]) -> anyhow::Result<()> {
        if sentences.is_empty() {
            return Ok(());
        }
        let members = vec!["?"; sentences.len()].join(",");
        let sql = format!(
            "SELECT l.jpn, t.lang, t.text FROM sentence_links l JOIN sentences t ON t.id = l.other
             WHERE l.jpn IN ({members}) ORDER BY l.jpn, t.id"
        );
        let mut stmt = self.conn.prepare_cached(&sql)?;
        let ids = sentences.iter().map(|s| Value::Integer(s.id));
        let mut all: HashMap<i64, Vec<(String, String)>> = HashMap::new();
        for row in stmt.query_map(params_from_iter(ids), |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
            ))
        })? {
            let (jpn, lang, text) = row?;
            all.entry(jpn).or_default().push((lang, text));
        }
        for s in sentences {
            let Some(found) = all.remove(&s.id) else { continue };
            for lang in langs {
                if let Some(t) = found.iter().find(|(l, _)| l == lang) {
                    s.translations.push(t.clone());
                }
            }
            if s.translations.is_empty()
                && let Some(first) = found.into_iter().next()
            {
                s.translations.push(first);
            }
        }
        Ok(())
    }

    // -- import -----------------------------------------------------------------------------

    /// Inserts a batch of entries in one transaction. Their `source` must be registered.
    pub fn insert(&self, entries: &[Entry]) -> anyhow::Result<()> {
        let tx = self.conn.unchecked_transaction()?;
        {
            let mut ins_entry = tx
                .prepare_cached("INSERT INTO entries (source, seq, common, pitch) VALUES (?1, ?2, ?3, ?4)")?;
            let mut ins_form = tx.prepare_cached("INSERT INTO forms VALUES (?1, ?2, ?3, ?4, ?5, ?6)")?;
            let mut ins_sense = tx.prepare_cached(
                "INSERT INTO senses (entry_id, pos, parts, misc, fields, info, dial, origin, xref, ant, stag)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            )?;
            let mut ins_gloss = tx.prepare_cached("INSERT INTO glosses VALUES (?1, ?2, ?3, ?4)")?;
            for e in entries {
                let pitch = e.pitch.iter().map(u8::to_string).collect::<Vec<_>>().join(",");
                ins_entry.execute(params![e.source, e.id, e.common as i64, pitch])?;
                let entry_id = tx.last_insert_rowid();
                let joined =
                    |lists: &[Vec<String>], i: usize| lists.get(i).map(|l| l.join(SEP)).unwrap_or_default();
                for (i, k) in e.kanji.iter().enumerate() {
                    ins_form.execute(params![entry_id, "k", i as i64, k, joined(&e.kanji_info, i), ""])?;
                }
                for (i, r) in e.readings.iter().enumerate() {
                    ins_form.execute(params![
                        entry_id,
                        "r",
                        i as i64,
                        r,
                        joined(&e.reading_info, i),
                        joined(&e.reading_for, i)
                    ])?;
                }
                for (i, s) in e.senses.iter().enumerate() {
                    ins_sense.execute(params![
                        entry_id,
                        i as i64,
                        s.pos.join(SEP),
                        s.misc.join(SEP),
                        s.fields.join(SEP),
                        s.info.join(SEP),
                        s.dialects.join(SEP),
                        s.origins.join(SEP),
                        s.see_also.join(SEP),
                        s.antonyms.join(SEP),
                        s.only_for.join(SEP)
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
            "SELECT id, source, seq, common, pitch FROM entries WHERE id IN ({marks})"
        ))?;
        for row in stmt.query_map(params_from_iter(ids), |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, i64>(2)?,
                r.get::<_, bool>(3)?,
                r.get::<_, String>(4)?,
            ))
        })? {
            let (id, source, seq, common, pitch) = row?;
            entries.push((
                id,
                Entry {
                    source,
                    id: seq,
                    common,
                    pitch: pitch.split(',').filter_map(|n| n.parse().ok()).collect(),
                    ..Entry::default()
                },
            ));
        }
        let index: HashMap<i64, usize> = entries.iter().enumerate().map(|(i, e)| (e.0, i)).collect();

        // JLPT levels sit on JMdict numbers, not internal ids.
        let seqs: Vec<i64> = entries
            .iter()
            .filter(|(_, e)| e.source == "jmdict")
            .map(|(_, e)| e.id)
            .collect();
        if !seqs.is_empty() {
            let seq_marks = vec!["?"; seqs.len()].join(",");
            let mut stmt = self
                .conn
                .prepare(&format!("SELECT seq, level FROM jlpt WHERE seq IN ({seq_marks})"))?;
            let levels: HashMap<i64, u8> = stmt
                .query_map(params_from_iter(seqs.iter()), |r| {
                    Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)? as u8))
                })?
                .collect::<Result<_, _>>()?;
            for (_, e) in entries.iter_mut() {
                if e.source == "jmdict" {
                    e.jlpt = levels.get(&e.id).copied();
                }
            }
        }

        let mut stmt = self.conn.prepare(&format!(
            "SELECT entry_id, kind, text, info, restr FROM forms WHERE entry_id IN ({marks})
             ORDER BY entry_id, kind, pos"
        ))?;
        for row in stmt.query_map(params_from_iter(ids), |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, String>(4)?,
            ))
        })? {
            let (id, kind, text, info, restr) = row?;
            if let Some(&i) = index.get(&id) {
                let e = &mut entries[i].1;
                if kind == "k" {
                    e.kanji.push(text);
                    e.kanji_info.push(split(&info));
                } else {
                    e.readings.push(text);
                    e.reading_info.push(split(&info));
                    e.reading_for.push(split(&restr));
                }
            }
        }

        // Sense rowid → (index into entries, index into that entry's senses), so a gloss finds
        // its sense in one lookup rather than a search over every sense loaded (#108).
        let mut sense_ids: HashMap<i64, (usize, usize)> = HashMap::new();
        let mut stmt = self.conn.prepare(&format!(
            "SELECT id, entry_id, parts, misc, fields, info, dial, origin, xref, ant, stag FROM senses
             WHERE entry_id IN ({marks}) ORDER BY entry_id, pos"
        ))?;
        for row in stmt.query_map(params_from_iter(ids), |r| {
            let text = |i: usize| r.get::<_, String>(i);
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, i64>(1)?,
                [
                    text(2)?,
                    text(3)?,
                    text(4)?,
                    text(5)?,
                    text(6)?,
                    text(7)?,
                    text(8)?,
                    text(9)?,
                    text(10)?,
                ],
            ))
        })? {
            let (sid, eid, [parts, misc, fields, info, dial, origin, xref, ant, stag]) = row?;
            if let Some(&i) = index.get(&eid) {
                let e = &mut entries[i].1;
                e.senses.push(Sense {
                    pos: split(&parts),
                    misc: split(&misc),
                    fields: split(&fields),
                    glosses: Vec::new(),
                    info: split(&info),
                    dialects: split(&dial),
                    origins: split(&origin),
                    see_also: split(&xref),
                    antonyms: split(&ant),
                    only_for: split(&stag),
                });
                sense_ids.insert(sid, (i, e.senses.len() - 1));
            }
        }
        if !sense_ids.is_empty() {
            let smarks = vec!["?"; sense_ids.len()].join(",");
            let mut stmt = self.conn.prepare(&format!(
                "SELECT sense_id, lang, text FROM glosses WHERE sense_id IN ({smarks}) ORDER BY sense_id, pos"
            ))?;
            let sids = sense_ids.keys().copied();
            for row in stmt.query_map(params_from_iter(sids), |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                ))
            })? {
                let (sid, lang, text) = row?;
                if let Some(&(ei, si)) = sense_ids.get(&sid) {
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
             FROM forms f INDEXED BY forms_text JOIN entries e ON e.id = f.entry_id
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

    /// Wildcard search: `*` is any run of characters, `?` one character. Japanese text is matched
    /// against whole kanji forms and readings, other text anywhere inside a gloss. A LIKE scan,
    /// so slower than the indexed searches; only used when the user typed a wildcard.
    pub fn search_pattern(&self, text: &str, limit: usize, sources: &[String]) -> anyhow::Result<Vec<Entry>> {
        if text.is_empty() || sources.is_empty() {
            return Ok(Vec::new());
        }
        let mut pattern: String = like_escape(text).replace('*', "%").replace('?', "_");
        if !is_japanese(text) {
            pattern = format!("%{pattern}%");
        }
        let priority = source_priority(sources.len());
        let members = vec!["?"; sources.len()].join(",");
        let mut values: Vec<Value> = sources.iter().map(|s| Value::Text(s.clone())).collect();
        values.push(Value::Text(pattern));
        values.extend(sources.iter().map(|s| Value::Text(s.clone())));
        values.push(Value::Integer(limit as i64));
        let sql = if is_japanese(text) {
            format!(
                "SELECT f.entry_id, e.common, {priority} AS prio, min(length(f.text)) AS len
                 FROM forms f INDEXED BY forms_text JOIN entries e ON e.id = f.entry_id
                 WHERE f.text LIKE ? ESCAPE '\\' AND e.source IN ({members})
                 GROUP BY f.entry_id ORDER BY e.common DESC, prio, len, f.entry_id LIMIT ?"
            )
        } else {
            format!(
                "SELECT s.entry_id, e.common, {priority} AS prio, min(length(g.text)) AS len
                 FROM glosses g JOIN senses s ON s.id = g.sense_id JOIN entries e ON e.id = s.entry_id
                 WHERE g.text LIKE ? ESCAPE '\\' AND e.source IN ({members})
                 GROUP BY s.entry_id ORDER BY e.common DESC, prio, len, s.entry_id LIMIT ?"
            )
        };
        let mut stmt = self.conn.prepare_cached(&sql)?;
        let ids: Vec<i64> = stmt
            .query_map(params_from_iter(values), |r| r.get(0))?
            .collect::<Result<_, _>>()?;
        self.load(&ids)
    }

    /// Entries with a gloss equal to `text` (case-insensitive), common first.
    pub fn search_gloss_exact(
        &self,
        text: &str,
        limit: usize,
        sources: &[String],
    ) -> anyhow::Result<Vec<Entry>> {
        let Some(fts) = fts_query(text).map(|q| q.trim_end_matches('*').to_string()) else {
            return Ok(Vec::new());
        };
        if sources.is_empty() {
            return Ok(Vec::new());
        }
        let priority = source_priority(sources.len());
        let members = vec!["?"; sources.len()].join(",");
        let mut values: Vec<Value> = sources.iter().map(|s| Value::Text(s.clone())).collect();
        values.push(Value::Text(fts));
        values.extend(sources.iter().map(|s| Value::Text(s.clone())));
        values.push(Value::Text(text.to_lowercase()));
        values.push(Value::Integer(limit as i64));
        let sql = format!(
            "SELECT s.entry_id, e.common, {priority} AS prio
             FROM gloss_fts f JOIN glosses g ON g.rowid = f.rowid
                  JOIN senses s ON s.id = g.sense_id JOIN entries e ON e.id = s.entry_id
             WHERE gloss_fts MATCH ? AND e.source IN ({members})
             GROUP BY s.entry_id HAVING max(lower(g.text) = ?) = 1
             ORDER BY e.common DESC, prio, s.entry_id LIMIT ?"
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
        let priority = source_priority(sources.len());
        let members = vec!["?"; sources.len()].join(",");
        let source_params: Vec<Value> = sources.iter().map(|s| Value::Text(s.clone())).collect();
        let mut values: Vec<Value> = Vec::new();
        let sql = if is_japanese(q) {
            values.push(Value::Text(q.to_string()));
            values.extend(source_params.iter().cloned());
            values.push(Value::Text(format!("{}*", glob_escape(q))));
            values.extend(source_params.iter().cloned());
            values.push(Value::Integer(limit as i64));
            format!(
                "SELECT f.entry_id, max(f.text = ?) AS exact, e.common, {priority} AS prio,
                        min(length(f.text)) AS len
                 FROM forms f INDEXED BY forms_text JOIN entries e ON e.id = f.entry_id
                 WHERE f.text GLOB ? AND e.source IN ({members})
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
        db.begin_source("wadoku", WHEN).unwrap();
        let mut with_pitch = sample_entries().into_iter().find(|e| e.id == 1467640).unwrap();
        with_pitch.source = "wadoku".into();
        with_pitch.pitch = vec![0, 3];
        db.insert(std::slice::from_ref(&with_pitch)).unwrap();
        assert_eq!(db.get("wadoku", 1467640).unwrap(), Some(with_pitch));
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
    fn pattern_and_exact_searches() {
        let db = sample_db();
        assert_eq!(
            ids(&db.search_pattern("猫?", 100, &jmdict_only()).unwrap()),
            [2000002]
        ); // 猫背
        assert_eq!(
            db.search_pattern("*じゃらし", 100, &jmdict_only()).unwrap()[0].id,
            2000001
        );
        let wild = ids(&db.search_pattern("c?t", 100, &jmdict_only()).unwrap());
        assert!(wild.contains(&1467640)); // anywhere in a gloss: "cat (esp. …)", "offensichtlich"
        assert!(db.search_pattern("100%", 100, &jmdict_only()).unwrap().is_empty());
        assert_eq!(
            db.search_gloss_exact("Katze", 100, &jmdict_only()).unwrap()[0].id,
            1467640
        );
        assert_eq!(
            db.search_gloss_exact("obvious", 100, &jmdict_only()).unwrap()[0].id,
            1000225
        );
        assert!(
            db.search_gloss_exact("cat", 100, &jmdict_only())
                .unwrap()
                .is_empty()
        ); // only "cat (esp. …)"
        assert!(
            db.search_gloss_exact("ca", 100, &jmdict_only())
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn glob_metacharacters_are_literal() {
        assert_eq!(glob_escape("猫*[x]?"), "猫[*][[]x[]][?]");
        let db = sample_db();
        assert!(db.search("猫*", 100, &jmdict_only()).unwrap().is_empty());
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
    fn kanji_tables_round_trip() {
        let db = sample_db();
        let cat = Kanji {
            literal: '猫',
            grade: Some(8),
            strokes: 11,
            freq: Some(1702),
            jlpt: Some(2),
            radical: 94,
            on: vec!["ビョウ".into()],
            kun: vec!["ねこ".into()],
            nanori: Vec::new(),
            meanings: vec![
                Gloss {
                    lang: "eng".into(),
                    text: "cat".into(),
                },
                Gloss {
                    lang: "fre".into(),
                    text: "chat".into(),
                },
            ],
        };
        db.begin_source("kanjidic", WHEN).unwrap();
        db.insert_kanji(std::slice::from_ref(&cat)).unwrap();
        assert_eq!(db.kanji('猫').unwrap(), Some(cat));
        assert_eq!(db.kanji('犬').unwrap(), None);
        assert!(db.has_kanji_data().unwrap());

        db.begin_source("kanjivg", WHEN).unwrap();
        db.insert_strokes(&[Strokes {
            literal: '猫',
            paths: vec!["M1,1".into(), "M2,2".into()],
        }])
        .unwrap();
        assert_eq!(db.strokes('猫').unwrap().unwrap(), ["M1,1", "M2,2"]);

        db.begin_source("radkfile", WHEN).unwrap();
        db.insert_radicals(&[
            Radical {
                radical: "犭".into(),
                strokes: 3,
                kanji: vec!['猫', '犬'],
            },
            Radical {
                radical: "田".into(),
                strokes: 5,
                kanji: vec!['猫'],
            },
        ])
        .unwrap();
        assert_eq!(db.radicals_of('猫').unwrap(), ["犭", "田"]);
        assert_eq!(
            db.kanji_with_radicals(&["犭".into(), "田".into()], None).unwrap(),
            ['猫']
        );
        assert_eq!(db.kanji_with_radicals(&["犭".into()], None).unwrap().len(), 2);
        assert_eq!(db.radicals().unwrap().len(), 2);
        assert_eq!(db.kanji_with_radicals(&["犭".into()], Some(11)).unwrap(), ['猫']);
        assert_eq!(db.radicals_compatible(&["田".into()]).unwrap(), ["犭", "田"]);
        assert_eq!(db.radicals_compatible(&[]).unwrap().len(), 2);

        let words = db.words_with('猫', 10, &jmdict_only()).unwrap();
        assert!(words.iter().all(|e| e.kanji.iter().any(|k| k.contains('猫'))));
        assert!(words.len() >= 2);

        db.remove_source("kanjidic").unwrap();
        assert!(!db.has_kanji_data().unwrap());
        assert!(db.strokes('猫').unwrap().is_some()); // other kanji sources untouched
    }

    #[test]
    fn an_interrupted_import_is_dropped_on_open() {
        let db = sample_db();
        db.begin_source("wadoku", WHEN).unwrap();
        db.drop_interrupted_imports().unwrap();
        let ids: Vec<String> = db.sources().unwrap().into_iter().map(|s| s.id).collect();
        assert_eq!(ids, ["jmdict"]);
        assert!(!db.was_rebuilt());
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
        assert_eq!(db.meta("schema").unwrap().as_deref(), Some("8"));
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
        assert_eq!(db.meta("schema").unwrap().as_deref(), Some("8"));
        drop(db);
        std::fs::remove_dir_all(dir).unwrap();
    }
}
