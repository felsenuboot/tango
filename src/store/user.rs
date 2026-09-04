//! The user's own data: word lists. Its own SQLite file, next to the dictionary but with the
//! opposite rule: this one is migrated forward and never dropped.
//!
//! A list entry remembers the entry's headword, reading and first gloss, so a list still reads
//! after the dictionary file was rebuilt or a source removed, and so exports need no lookup.

use std::path::Path;

use anyhow::Context;
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};

use crate::model::Entry;
use crate::store::now_iso8601;

/// One statement batch per version; `PRAGMA user_version` says how many have run.
const MIGRATIONS: &[&str] = &["
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
"];

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
