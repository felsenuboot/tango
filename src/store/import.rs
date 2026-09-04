//! Import, download and remove jobs, meant to run on a worker thread and report progress to the UI.

use std::path::Path;

use anyhow::bail;

use crate::dict::sources::{self, Source};
use crate::dict::{jmdict, wadoku};
use crate::store::db::Database;
use crate::store::now_iso8601;

/// `(message, fraction 0..1, or None for "busy, unknown how far")`
pub type Report<'a> = &'a mut dyn FnMut(String, Option<f64>);

const BATCH: usize = 2000;

/// Replaces what the database has of `source` with the entries in `path`; returns the entry count.
/// Batches are committed as they go, so a failure halfway removes the source again rather than
/// leaving half of it behind a row that says it is installed.
pub fn import_file(db: &Database, source: &Source, path: &Path, report: Report) -> anyhow::Result<usize> {
    let result = match source.id {
        "jmdict" => import_jmdict(db, source, path, report),
        "wadoku" => import_wadoku(db, source, path, report),
        other => bail!("no reader for the {other} source"),
    };
    if result.is_err()
        && let Err(e) = db.remove_source(source.id)
    {
        log::warn!("could not clean up the failed {} import: {e:#}", source.name);
    }
    result
}

fn import_wadoku(db: &Database, source: &Source, path: &Path, report: Report) -> anyhow::Result<usize> {
    report(format!("Unpacking {}…", source.name), None);
    let xml = wadoku::unpack(path)?;
    report(format!("Reading {}…", source.name), None);
    let version = wadoku::created(wadoku::open(&xml)?)?;
    db.begin_source(source.id, &now_iso8601())?;
    let mut batch: Vec<crate::model::Entry> = Vec::with_capacity(BATCH);
    let mut imported = 0usize;
    let count = wadoku::for_each_entry(wadoku::open(&xml)?, |mut entry| {
        entry.source = source.id.to_string();
        batch.push(entry);
        if batch.len() >= BATCH {
            db.insert(&batch)?;
            imported += batch.len();
            batch.clear();
            report(format!("Imported {imported} entries…"), None);
        }
        Ok(())
    })?;
    db.insert(&batch)?;
    db.finish_source(source.id, version.as_deref(), &now_iso8601(), count as i64)?;
    report("Indexing the glosses…".into(), None);
    db.rebuild_gloss_index()?;
    report(format!("Imported {count} entries."), Some(1.0));
    log::info!(
        "imported {count} {} entries (version {}) from {}",
        source.name,
        version.as_deref().unwrap_or("unknown"),
        xml.display()
    );
    Ok(count)
}

fn import_jmdict(db: &Database, source: &Source, path: &Path, report: Report) -> anyhow::Result<usize> {
    report(format!("Reading {}…", source.name), None);
    let version = jmdict::created(jmdict::open(path)?)?;
    db.begin_source(source.id, &now_iso8601())?;
    let mut batch: Vec<crate::model::Entry> = Vec::with_capacity(BATCH);
    let mut imported = 0usize;
    let count = jmdict::for_each_entry(jmdict::open(path)?, |mut entry| {
        entry.source = source.id.to_string();
        batch.push(entry);
        if batch.len() >= BATCH {
            db.insert(&batch)?;
            imported += batch.len();
            batch.clear();
            report(format!("Imported {imported} entries…"), None);
        }
        Ok(())
    })?;
    db.insert(&batch)?;
    db.finish_source(source.id, version.as_deref(), &now_iso8601(), count as i64)?;
    report("Indexing the glosses…".into(), None);
    db.rebuild_gloss_index()?;
    report(format!("Imported {count} entries."), Some(1.0));
    log::info!(
        "imported {count} {} entries (version {}) from {}",
        source.name,
        version.as_deref().unwrap_or("unknown"),
        path.display()
    );
    Ok(count)
}

/// Downloads `source` into `cache` and imports it.
pub fn download_and_import(
    db: &Database,
    source: &Source,
    cache: &Path,
    report: Report,
) -> anyhow::Result<usize> {
    let dest = cache.join(source.filename);
    report(format!("Looking up {}…", source.name), None);
    let url = sources::download_url(source)?;
    report(format!("Downloading {}…", source.name), None);
    sources::download(&url, &dest, &mut |done, total| match total {
        Some(total) => report(
            format!(
                "Downloading {}… {} of {} MB",
                source.name,
                done / 1_000_000,
                total / 1_000_000
            ),
            Some(done as f64 / total as f64),
        ),
        None => report(
            format!("Downloading {}… {} MB", source.name, done / 1_000_000),
            None,
        ),
    })?;
    import_file(db, source, &dest, report)
}

/// Drops a source's entries and its cached download, then compacts the file.
pub fn remove(db: &Database, source: &Source, cache: &Path, report: Report) -> anyhow::Result<()> {
    report(format!("Removing {}…", source.name), None);
    db.remove_source(source.id)?;
    match std::fs::remove_file(cache.join(source.filename)) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => log::warn!("could not delete the cached {}: {e}", source.filename),
    }
    db.rebuild_gloss_index()?;
    report("Compacting the database…".into(), None);
    if let Err(e) = db.vacuum() {
        log::warn!(
            "could not compact the database after removing {}: {e:#}",
            source.name
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn importer_records_the_source_and_reports() {
        let db = Database::open_in_memory().unwrap();
        let sample = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/jmdict-sample.xml");
        let mut messages = Vec::new();
        let n = import_file(&db, &sources::JMDICT, &sample, &mut |m, f| messages.push((m, f))).unwrap();
        assert_eq!(n, 7);
        assert_eq!(db.entry_count().unwrap(), 7);
        let status = db.sources().unwrap();
        assert_eq!(status.len(), 1);
        assert_eq!(status[0].id, "jmdict");
        assert_eq!(status[0].version.as_deref(), Some("2024-01-01"));
        assert_eq!(status[0].entries, 7);
        assert!(!status[0].imported.is_empty());
        assert_eq!(
            messages.last().unwrap(),
            &("Imported 7 entries.".to_string(), Some(1.0))
        );
    }

    #[test]
    fn wadoku_imports_next_to_jmdict() {
        let db = Database::open_in_memory().unwrap();
        let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
        import_file(
            &db,
            &sources::JMDICT,
            &fixtures.join("jmdict-sample.xml"),
            &mut |_, _| {},
        )
        .unwrap();
        let n = import_file(
            &db,
            &sources::WADOKU,
            &fixtures.join("wadoku-sample.xml"),
            &mut |_, _| {},
        )
        .unwrap();
        assert_eq!(n, 5);
        assert_eq!(db.entry_count().unwrap(), 12);
        let both = vec!["jmdict".to_string(), "wadoku".to_string()];
        let found = db.search("Völlerei", 10, &both).unwrap();
        assert_eq!((found[0].source.as_str(), found[0].id), ("wadoku", 273));
        assert_eq!(found[0].pitch, [0]);
        let status = db.sources().unwrap();
        assert_eq!(
            status.iter().map(|s| s.id.as_str()).collect::<Vec<_>>(),
            ["jmdict", "wadoku"]
        );
        assert_eq!(status[1].version.as_deref(), Some("2026-07-05"));
    }

    #[test]
    fn a_failed_import_leaves_nothing_behind() {
        let db = Database::open_in_memory().unwrap();
        let dir = std::env::temp_dir().join(format!("tango-broken-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let broken = dir.join("broken.xml");
        // Two good entries, then garbage.
        let sample = std::fs::read_to_string(
            Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/jmdict-sample.xml"),
        )
        .unwrap();
        let cut = sample.find("<ent_seq>1236120</ent_seq>").unwrap();
        std::fs::write(
            &broken,
            format!("{}<entry><ent_seq>x</ent_seq></entry>", &sample[..cut]),
        )
        .unwrap();
        assert!(import_file(&db, &sources::JMDICT, &broken, &mut |_, _| {}).is_err());
        assert_eq!(db.entry_count().unwrap(), 0);
        assert!(db.sources().unwrap().is_empty());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn remove_takes_the_cached_file_with_it() {
        let db = Database::open_in_memory().unwrap();
        let sample = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/jmdict-sample.xml");
        import_file(&db, &sources::JMDICT, &sample, &mut |_, _| {}).unwrap();
        let cache = std::env::temp_dir().join(format!("tango-cache-{}", std::process::id()));
        std::fs::create_dir_all(&cache).unwrap();
        std::fs::write(cache.join(sources::JMDICT.filename), b"stale").unwrap();
        remove(&db, &sources::JMDICT, &cache, &mut |_, _| {}).unwrap();
        assert_eq!(db.entry_count().unwrap(), 0);
        assert!(!cache.join(sources::JMDICT.filename).exists());
        std::fs::remove_dir_all(cache).unwrap();
    }
}
