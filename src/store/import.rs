//! Import, download and remove jobs, meant to run on a worker thread and report progress to the UI.

use std::path::Path;

use anyhow::bail;

use crate::dict::jmdict;
use crate::dict::sources::{self, Source};
use crate::store::db::Database;

/// `(message, fraction 0..1, or None for "busy, unknown how far")`
pub type Report<'a> = &'a mut dyn FnMut(String, Option<f64>);

const BATCH: usize = 2000;

/// Replaces what the database has of `source` with the entries in `path`; returns the entry count.
pub fn import_file(db: &Database, source: &Source, path: &Path, report: Report) -> anyhow::Result<usize> {
    match source.id {
        "jmdict" => import_jmdict(db, source, path, report),
        other => bail!("no reader for the {other} source"),
    }
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
    report(format!("Downloading {}…", source.name), None);
    sources::download(source.url, &dest, &mut |done, total| match total {
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

fn now_iso8601() -> String {
    gtk::glib::DateTime::now_utc()
        .and_then(|t| t.format_iso8601())
        .map(|s| s.to_string())
        .unwrap_or_default()
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
