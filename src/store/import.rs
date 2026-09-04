//! Download-and-import jobs, meant to run on a worker thread and report progress to the UI.

use std::path::Path;

use crate::dict::{jmdict, sources};
use crate::store::db::Database;

/// `(message, fraction 0..1, or None for "busy, unknown how far")`
pub type Report<'a> = &'a mut dyn FnMut(String, Option<f64>);

const BATCH: usize = 2000;

/// Replaces the database contents with the entries from a JMdict file; returns the entry count.
pub fn import_jmdict(db: &Database, path: &Path, report: Report) -> anyhow::Result<usize> {
    report("Reading JMdict…".into(), None);
    db.clear()?;
    let mut batch: Vec<crate::model::Entry> = Vec::with_capacity(BATCH);
    let mut imported = 0usize;
    let count = jmdict::for_each_entry(jmdict::open(path)?, |entry| {
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
    db.set_meta("jmdict_imported", &now_iso8601())?;
    db.set_meta("jmdict_entries", &count.to_string())?;
    report(format!("Imported {count} entries."), Some(1.0));
    log::info!("imported {count} JMdict entries from {}", path.display());
    Ok(count)
}

pub fn download_and_import_jmdict(db: &Database, cache: &Path, report: Report) -> anyhow::Result<usize> {
    let dest = cache.join(sources::JMDICT_FILENAME);
    report("Downloading JMdict…".into(), None);
    sources::download(sources::JMDICT_URL, &dest, &mut |done, total| match total {
        Some(total) => report(
            format!(
                "Downloading JMdict… {} of {} MB",
                done / 1_000_000,
                total / 1_000_000
            ),
            Some(done as f64 / total as f64),
        ),
        None => report(format!("Downloading JMdict… {} MB", done / 1_000_000), None),
    })?;
    import_jmdict(db, &dest, report)
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
    fn importer_records_meta_and_reports() {
        let db = Database::open_in_memory().unwrap();
        let sample = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/jmdict-sample.xml");
        let mut messages = Vec::new();
        let n = import_jmdict(&db, &sample, &mut |m, f| messages.push((m, f))).unwrap();
        assert_eq!(n, 6);
        assert_eq!(db.entry_count().unwrap(), 6);
        assert_eq!(db.meta("jmdict_entries").unwrap().as_deref(), Some("6"));
        assert!(db.meta("jmdict_imported").unwrap().is_some());
        assert_eq!(
            messages.last().unwrap(),
            &("Imported 6 entries.".to_string(), Some(1.0))
        );
    }
}
