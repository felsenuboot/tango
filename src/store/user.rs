//! The user's own data: word lists. Its own SQLite file, next to the dictionary but with the
//! opposite rule: this one is migrated forward and never dropped.
//!
//! A list entry remembers the entry's headword, reading and first gloss, so a list still reads
//! after the dictionary file was rebuilt or a source removed, and so exports need no lookup.

use std::collections::HashMap;

use crate::accounts::wanikani::{Assignment, Subject};
use crate::accounts::{self, Kind, Learned};
use std::path::Path;

use anyhow::Context;
use rusqlite::{Connection, OptionalExtension, params, params_from_iter};
use serde::{Deserialize, Serialize};

use crate::model::Entry;
use crate::store::now_iso8601;

/// One statement batch per version; `PRAGMA user_version` says how many have run.
const MIGRATIONS: &[&str] = &[
    "
CREATE TABLE lists (
    id INTEGER PRIMARY KEY,
    name TEXT NOT NULL UNIQUE,
    position INTEGER NOT NULL,
    created TEXT NOT NULL
);
CREATE TABLE list_entries (
    list_id INTEGER NOT NULL REFERENCES lists(id) ON DELETE CASCADE,
    source TEXT NOT NULL,           -- dict::sources::Source::id
    seq INTEGER NOT NULL,           -- the source's own number, JMdict ent_seq
    headword TEXT NOT NULL,
    reading TEXT NOT NULL,
    gloss TEXT NOT NULL DEFAULT '', -- the first gloss in the preferred language when added
    note TEXT NOT NULL DEFAULT '',
    added TEXT NOT NULL,
    PRIMARY KEY (list_id, source, seq)
);
CREATE INDEX list_entries_word ON list_entries(source, seq);
",
    "
-- Accounts (issue #11): what WaniKani reports, and the provider-independent `learned` rows
-- built from it that the entry view and the #known filters read.
CREATE TABLE wk_subjects (
    id INTEGER PRIMARY KEY,         -- WaniKani's subject number
    kind TEXT NOT NULL,             -- 'kanji' | 'vocabulary'
    text TEXT NOT NULL,             -- the characters
    level INTEGER NOT NULL,
    hidden INTEGER NOT NULL DEFAULT 0
);
CREATE TABLE wk_assignments (
    subject_id INTEGER PRIMARY KEY,
    stage INTEGER NOT NULL,         -- SRS stage 0..9
    started TEXT,
    passed TEXT,
    burned TEXT,
    hidden INTEGER NOT NULL DEFAULT 0
);
CREATE TABLE learned (
    provider TEXT NOT NULL,         -- 'wanikani', later 'marumori'
    kind TEXT NOT NULL,
    text TEXT NOT NULL,
    level INTEGER NOT NULL,
    stage INTEGER NOT NULL,         -- on WaniKani's 0..9 scale
    PRIMARY KEY (provider, kind, text)
);
CREATE INDEX learned_text ON learned(text);
CREATE TABLE sync_state (
    provider TEXT NOT NULL,
    key TEXT NOT NULL,
    value TEXT NOT NULL,
    PRIMARY KEY (provider, key)
);
",
    "
-- #98: the provider's reading, so ふじ山 finds 富士山. Dropping the subjects cursor makes the
-- next sync fetch every subject again, with its readings.
ALTER TABLE wk_subjects ADD COLUMN reading TEXT NOT NULL DEFAULT '';
ALTER TABLE learned ADD COLUMN reading TEXT NOT NULL DEFAULT '';
DELETE FROM sync_state WHERE provider = 'wanikani' AND key = 'subjects';
",
];

/// One `learned` row as selected by `learned_of` and `learned_for`; `None` for an unknown kind.
fn learned_row(r: &rusqlite::Row) -> rusqlite::Result<Option<Learned>> {
    let kind: String = r.get(1)?;
    Ok(Kind::parse(&kind).map(|kind| Learned {
        provider: r.get(0).unwrap_or_default(),
        kind,
        text: r.get(2).unwrap_or_default(),
        reading: r.get(3).unwrap_or_default(),
        level: r.get::<_, i64>(4).unwrap_or(0) as u32,
        stage: r.get::<_, i64>(5).unwrap_or(0) as u8,
    }))
}

/// `(kind, text)` → `(level, stage)`; see `UserDb::learned_index`.
pub type LearnedIndex = HashMap<(Kind, String), (u32, u8)>;

pub const FAVOURITES: &str = "Favourites";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct List {
    pub id: i64,
    pub name: String,
    pub entries: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ListEntry {
    pub source: String,
    pub seq: i64,
    pub headword: String,
    pub reading: String,
    pub gloss: String,
    pub note: String,
    pub added: String,
}

/// The JSON backup: every list with its entries.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Backup {
    pub lists: Vec<BackupList>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BackupList {
    pub name: String,
    pub entries: Vec<ListEntry>,
}

pub struct UserDb {
    conn: Connection,
}

impl UserDb {
    pub fn open(path: &Path) -> anyhow::Result<Self> {
        let conn = Connection::open(path).with_context(|| format!("opening {}", path.display()))?;
        conn.execute_batch("PRAGMA foreign_keys = ON; PRAGMA journal_mode = WAL;")?;
        let db = Self { conn };
        db.migrate()?;
        Ok(db)
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub fn open_in_memory() -> anyhow::Result<Self> {
        let conn = Connection::open_in_memory()?;
        conn.execute_batch("PRAGMA foreign_keys = ON;")?;
        let db = Self { conn };
        db.migrate()?;
        Ok(db)
    }

    fn migrate(&self) -> anyhow::Result<()> {
        let done: i64 = self.conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
        for (i, sql) in MIGRATIONS.iter().enumerate().skip(done as usize) {
            let tx = self.conn.unchecked_transaction()?;
            tx.execute_batch(sql)?;
            tx.execute_batch(&format!("PRAGMA user_version = {}", i + 1))?;
            tx.commit()?;
            log::info!("user database migrated to version {}", i + 1);
        }
        if self.lists()?.is_empty() {
            self.create_list(FAVOURITES)?;
        }
        Ok(())
    }

    // -- accounts ---------------------------------------------------------------------------

    pub fn upsert_wk_subjects(&self, subjects: &[Subject]) -> anyhow::Result<()> {
        let tx = self.conn.unchecked_transaction()?;
        {
            let mut ins = tx.prepare_cached(
                "INSERT OR REPLACE INTO wk_subjects (id, kind, text, reading, level, hidden)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            )?;
            for s in subjects {
                ins.execute(params![
                    s.id,
                    s.kind.as_str(),
                    s.text,
                    s.reading,
                    s.level,
                    s.hidden
                ])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    pub fn upsert_wk_assignments(&self, assignments: &[Assignment]) -> anyhow::Result<()> {
        let tx = self.conn.unchecked_transaction()?;
        {
            let mut ins = tx.prepare_cached(
                "INSERT OR REPLACE INTO wk_assignments (subject_id, stage, started, passed, burned, hidden)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            )?;
            for a in assignments {
                ins.execute(params![
                    a.subject_id,
                    a.stage,
                    a.started,
                    a.passed,
                    a.burned,
                    a.hidden
                ])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    /// Rebuilds WaniKani's `learned` rows from the subjects and assignments: every visible
    /// subject, with its assignment's stage or 0 (locked) without one. Returns the row count.
    pub fn rebuild_learned_wanikani(&self) -> anyhow::Result<usize> {
        let tx = self.conn.unchecked_transaction()?;
        tx.execute("DELETE FROM learned WHERE provider = 'wanikani'", [])?;
        let n = tx.execute(
            "INSERT OR REPLACE INTO learned (provider, kind, text, reading, level, stage)
             SELECT 'wanikani', s.kind, s.text, s.reading, s.level, coalesce(a.stage, 0)
             FROM wk_subjects s LEFT JOIN wk_assignments a ON a.subject_id = s.id AND a.hidden = 0
             WHERE s.hidden = 0 ORDER BY coalesce(a.stage, 0)",
            [],
        )?;
        tx.commit()?;
        Ok(n)
    }

    pub fn sync_state(&self, provider: &str, key: &str) -> anyhow::Result<Option<String>> {
        Ok(self
            .conn
            .query_row(
                "SELECT value FROM sync_state WHERE provider = ?1 AND key = ?2",
                params![provider, key],
                |r| r.get(0),
            )
            .optional()?)
    }

    pub fn set_sync_state(&self, provider: &str, key: &str, value: &str) -> anyhow::Result<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO sync_state (provider, key, value) VALUES (?1, ?2, ?3)",
            params![provider, key, value],
        )?;
        Ok(())
    }

    /// Everything a provider stored: its learned rows, its raw tables, its sync cursors.
    pub fn clear_provider(&self, provider: &str) -> anyhow::Result<()> {
        let tx = self.conn.unchecked_transaction()?;
        tx.execute("DELETE FROM learned WHERE provider = ?1", params![provider])?;
        tx.execute("DELETE FROM sync_state WHERE provider = ?1", params![provider])?;
        if provider == "wanikani" {
            tx.execute_batch("DELETE FROM wk_assignments; DELETE FROM wk_subjects;")?;
        }
        tx.commit()?;
        Ok(())
    }

    /// Everything one provider knows, vocabulary before kanji, by level and text.
    pub fn learned_of(&self, provider: &str) -> anyhow::Result<Vec<Learned>> {
        let mut stmt = self.conn.prepare_cached(
            "SELECT provider, kind, text, reading, level, stage FROM learned WHERE provider = ?1
             ORDER BY kind DESC, level, text",
        )?;
        let rows: Vec<Option<Learned>> = stmt
            .query_map(params![provider], learned_row)?
            .collect::<Result<_, _>>()?;
        Ok(rows.into_iter().flatten().collect())
    }

    pub fn learned_count(&self, provider: &str) -> anyhow::Result<usize> {
        let n: i64 = self.conn.query_row(
            "SELECT count(*) FROM learned WHERE provider = ?1",
            params![provider],
            |r| r.get(0),
        )?;
        Ok(n as usize)
    }

    /// The learned rows for any of `texts` (a headword, its forms, single kanji), any provider.
    /// A word the provider spells differently (ふじ山) matches through its reading when its
    /// kanji all occur in one of `texts` (#98).
    pub fn learned_for(&self, texts: &[String]) -> anyhow::Result<Vec<Learned>> {
        if texts.is_empty() {
            return Ok(Vec::new());
        }
        let marks = vec!["?"; texts.len()].join(",");
        let mut stmt = self.conn.prepare_cached(&format!(
            "SELECT provider, kind, text, reading, level, stage FROM learned
             WHERE text IN ({marks}) OR (kind = 'vocabulary' AND reading != '' AND reading IN ({marks}))
             ORDER BY provider, kind, text"
        ))?;
        let rows = stmt.query_map(params_from_iter(texts.iter().chain(texts.iter())), learned_row)?;
        let mut out = Vec::new();
        for row in rows {
            let Some(l) = row? else { continue };
            if texts.contains(&l.text) || texts.iter().any(|t| accounts::spelled_like(&l.text, t)) {
                out.push(l);
            }
        }
        Ok(out)
    }

    /// Every learned item by kind and text, `(level, stage)`, for the search filters. The best
    /// stage wins when two providers know the same item.
    pub fn learned_index(&self) -> anyhow::Result<LearnedIndex> {
        let mut stmt = self
            .conn
            .prepare("SELECT kind, text, level, stage FROM learned ORDER BY stage")?;
        let rows = stmt.query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, i64>(2)?,
                r.get::<_, i64>(3)?,
            ))
        })?;
        let mut index = LearnedIndex::new();
        for row in rows {
            let (kind, text, level, stage) = row?;
            if let Some(kind) = Kind::parse(&kind) {
                index.insert((kind, text), (level as u32, stage as u8));
            }
        }
        Ok(index)
    }

    // -- lists ------------------------------------------------------------------------------

    /// All lists in their order, with entry counts.
    pub fn lists(&self) -> anyhow::Result<Vec<List>> {
        let mut stmt = self.conn.prepare(
            "SELECT l.id, l.name, (SELECT count(*) FROM list_entries e WHERE e.list_id = l.id)
             FROM lists l ORDER BY l.position, l.id",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok(List {
                id: r.get(0)?,
                name: r.get(1)?,
                entries: r.get(2)?,
            })
        })?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    /// The first list, which is Favourites unless the user reordered or renamed it.
    pub fn favourites(&self) -> anyhow::Result<List> {
        match self.lists()?.into_iter().next() {
            Some(list) => Ok(list),
            None => self.create_list(FAVOURITES),
        }
    }

    pub fn create_list(&self, name: &str) -> anyhow::Result<List> {
        let name = name.trim();
        anyhow::ensure!(!name.is_empty(), "a list needs a name");
        let position: i64 =
            self.conn
                .query_row("SELECT coalesce(max(position), -1) + 1 FROM lists", [], |r| {
                    r.get(0)
                })?;
        self.conn.execute(
            "INSERT INTO lists (name, position, created) VALUES (?1, ?2, ?3)",
            params![name, position, now_iso8601()],
        )?;
        Ok(List {
            id: self.conn.last_insert_rowid(),
            name: name.to_string(),
            entries: 0,
        })
    }

    pub fn list_by_name(&self, name: &str) -> anyhow::Result<Option<List>> {
        Ok(self.lists()?.into_iter().find(|l| l.name == name.trim()))
    }

    pub fn rename_list(&self, id: i64, name: &str) -> anyhow::Result<()> {
        let name = name.trim();
        anyhow::ensure!(!name.is_empty(), "a list needs a name");
        self.conn
            .execute("UPDATE lists SET name = ?2 WHERE id = ?1", params![id, name])?;
        Ok(())
    }

    pub fn delete_list(&self, id: i64) -> anyhow::Result<()> {
        self.conn
            .execute("DELETE FROM lists WHERE id = ?1", params![id])?;
        Ok(())
    }

    /// Swaps the list with the one before it.
    pub fn move_list_up(&self, id: i64) -> anyhow::Result<()> {
        let lists = self.lists()?;
        if let Some(i) = lists.iter().position(|l| l.id == id)
            && i > 0
        {
            let tx = self.conn.unchecked_transaction()?;
            for (position, list) in [(i - 1, &lists[i]), (i, &lists[i - 1])] {
                tx.execute(
                    "UPDATE lists SET position = ?2 WHERE id = ?1",
                    params![list.id, position as i64],
                )?;
            }
            // Positions may have had gaps; renumber the rest so the swap holds.
            for (position, list) in lists.iter().enumerate() {
                if position != i && position != i - 1 {
                    tx.execute(
                        "UPDATE lists SET position = ?2 WHERE id = ?1",
                        params![list.id, position as i64],
                    )?;
                }
            }
            tx.commit()?;
        }
        Ok(())
    }

    // -- entries ----------------------------------------------------------------------------

    /// Adds an entry to a list; `gloss` is what the list row shows, chosen by the caller in the
    /// preferred language. Adding again is a no-op.
    pub fn add(&self, list_id: i64, entry: &Entry, gloss: &str) -> anyhow::Result<()> {
        self.conn.execute(
            "INSERT OR IGNORE INTO list_entries (list_id, source, seq, headword, reading, gloss, added)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                list_id,
                entry.source,
                entry.id,
                entry.headword(),
                entry.reading(),
                gloss,
                now_iso8601()
            ],
        )?;
        Ok(())
    }

    pub fn remove(&self, list_id: i64, source: &str, seq: i64) -> anyhow::Result<()> {
        self.conn.execute(
            "DELETE FROM list_entries WHERE list_id = ?1 AND source = ?2 AND seq = ?3",
            params![list_id, source, seq],
        )?;
        Ok(())
    }

    pub fn contains(&self, list_id: i64, source: &str, seq: i64) -> anyhow::Result<bool> {
        let found: Option<i64> = self
            .conn
            .query_row(
                "SELECT 1 FROM list_entries WHERE list_id = ?1 AND source = ?2 AND seq = ?3",
                params![list_id, source, seq],
                |r| r.get(0),
            )
            .optional()?;
        Ok(found.is_some())
    }

    /// Ids of the lists that hold the entry.
    pub fn lists_with(&self, source: &str, seq: i64) -> anyhow::Result<Vec<i64>> {
        let mut stmt = self
            .conn
            .prepare("SELECT list_id FROM list_entries WHERE source = ?1 AND seq = ?2")?;
        let ids = stmt.query_map(params![source, seq], |r| r.get(0))?;
        Ok(ids.collect::<Result<_, _>>()?)
    }

    /// The entries of a list, newest first.
    pub fn entries(&self, list_id: i64) -> anyhow::Result<Vec<ListEntry>> {
        let mut stmt = self.conn.prepare(
            "SELECT source, seq, headword, reading, gloss, note, added FROM list_entries
             WHERE list_id = ?1 ORDER BY added DESC, rowid DESC",
        )?;
        let rows = stmt.query_map(params![list_id], |r| {
            Ok(ListEntry {
                source: r.get(0)?,
                seq: r.get(1)?,
                headword: r.get(2)?,
                reading: r.get(3)?,
                gloss: r.get(4)?,
                note: r.get(5)?,
                added: r.get(6)?,
            })
        })?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    pub fn set_note(&self, list_id: i64, source: &str, seq: i64, note: &str) -> anyhow::Result<()> {
        self.conn.execute(
            "UPDATE list_entries SET note = ?4 WHERE list_id = ?1 AND source = ?2 AND seq = ?3",
            params![list_id, source, seq, note.trim()],
        )?;
        Ok(())
    }

    // -- backup -----------------------------------------------------------------------------

    pub fn backup(&self) -> anyhow::Result<Backup> {
        let mut lists = Vec::new();
        for list in self.lists()? {
            lists.push(BackupList {
                name: list.name,
                entries: self.entries(list.id)?,
            });
        }
        Ok(Backup { lists })
    }

    /// Merges a backup in: lists are matched by name and created when missing, entries that are
    /// already there are left alone. Returns how many entries were added.
    pub fn restore(&self, backup: &Backup) -> anyhow::Result<usize> {
        let mut added = 0;
        for list in &backup.lists {
            let target = match self.list_by_name(&list.name)? {
                Some(l) => l,
                None => self.create_list(&list.name)?,
            };
            for e in &list.entries {
                let n = self.conn.execute(
                    "INSERT OR IGNORE INTO list_entries
                     (list_id, source, seq, headword, reading, gloss, note, added)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                    params![
                        target.id, e.source, e.seq, e.headword, e.reading, e.gloss, e.note, e.added
                    ],
                )?;
                added += n;
            }
        }
        Ok(added)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cat() -> Entry {
        Entry {
            source: "jmdict".into(),
            id: 1467640,
            kanji: vec!["猫".into()],
            readings: vec!["ねこ".into()],
            ..Entry::default()
        }
    }

    #[test]
    fn favourites_exist_and_lists_are_ordered() {
        let db = UserDb::open_in_memory().unwrap();
        let fav = db.favourites().unwrap();
        assert_eq!(fav.name, FAVOURITES);
        let verbs = db.create_list("Verbs").unwrap();
        assert_eq!(
            db.lists().unwrap().iter().map(|l| l.id).collect::<Vec<_>>(),
            [fav.id, verbs.id]
        );
        db.move_list_up(verbs.id).unwrap();
        assert_eq!(db.lists().unwrap()[0].name, "Verbs");
        db.move_list_up(verbs.id).unwrap(); // already first
        assert_eq!(db.lists().unwrap()[0].name, "Verbs");
        db.rename_list(verbs.id, " Godan ").unwrap();
        assert_eq!(db.list_by_name("Godan").unwrap().unwrap().id, verbs.id);
        assert!(db.create_list("  ").is_err());
        assert!(db.create_list("Godan").is_err()); // names are unique
    }

    #[test]
    fn entries_round_trip() {
        let db = UserDb::open_in_memory().unwrap();
        let fav = db.favourites().unwrap();
        db.add(fav.id, &cat(), "Katze").unwrap();
        db.add(fav.id, &cat(), "Katze").unwrap(); // twice is once
        assert!(db.contains(fav.id, "jmdict", 1467640).unwrap());
        assert_eq!(db.lists_with("jmdict", 1467640).unwrap(), [fav.id]);
        assert_eq!(db.favourites().unwrap().entries, 1);
        db.set_note(fav.id, "jmdict", 1467640, " has a tail ").unwrap();
        let entries = db.entries(fav.id).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(
            (entries[0].headword.as_str(), entries[0].reading.as_str()),
            ("猫", "ねこ")
        );
        assert_eq!(entries[0].note, "has a tail");
        db.remove(fav.id, "jmdict", 1467640).unwrap();
        assert!(!db.contains(fav.id, "jmdict", 1467640).unwrap());
        db.delete_list(fav.id).unwrap();
        assert_eq!(db.favourites().unwrap().name, FAVOURITES); // recreated when none is left
    }

    #[test]
    fn backup_and_restore_merge() {
        let db = UserDb::open_in_memory().unwrap();
        let fav = db.favourites().unwrap();
        db.add(fav.id, &cat(), "cat").unwrap();
        let backup = db.backup().unwrap();
        let json = serde_json::to_string(&backup).unwrap();
        let other = UserDb::open_in_memory().unwrap();
        let parsed: Backup = serde_json::from_str(&json).unwrap();
        assert_eq!(other.restore(&parsed).unwrap(), 1);
        assert_eq!(other.restore(&parsed).unwrap(), 0);
        assert_eq!(other.backup().unwrap(), backup);
    }

    #[test]
    fn learned_for_matches_wanikani_spelling() {
        let db = UserDb::open_in_memory().unwrap();
        db.upsert_wk_subjects(&[Subject {
            id: 1,
            kind: Kind::Vocabulary,
            text: "ふじ山".into(),
            reading: "ふじさん".into(),
            level: 31,
            hidden: false,
        }])
        .unwrap();
        db.rebuild_learned_wanikani().unwrap();
        let hits = |forms: &[&str]| {
            let forms: Vec<String> = forms.iter().map(|s| s.to_string()).collect();
            db.learned_for(&forms).unwrap().len()
        };
        assert_eq!(
            hits(&["富士山", "ふじさん"]),
            1,
            "the reading matches and 山 is in the form"
        );
        assert_eq!(hits(&["ふじ山"]), 1, "WaniKani's own spelling");
        assert_eq!(hits(&["藤さん", "ふじさん"]), 0, "same reading, but no 山");
        assert_eq!(hits(&["富士山"]), 0, "without the reading among the forms");
    }

    #[test]
    fn migrations_run_once() {
        let dir = std::env::temp_dir().join(format!("tango-user-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("user.sqlite");
        {
            let db = UserDb::open(&path).unwrap();
            db.create_list("Kept").unwrap();
        }
        let db = UserDb::open(&path).unwrap();
        assert!(db.list_by_name("Kept").unwrap().is_some());
        let version: i64 = db
            .conn
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .unwrap();
        assert_eq!(version as usize, MIGRATIONS.len());
        drop(db);
        std::fs::remove_dir_all(dir).unwrap();
    }
}
