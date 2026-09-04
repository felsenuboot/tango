//! Import, download and remove jobs, meant to run on a worker thread and report progress to the UI.

use std::collections::{HashMap, HashSet};
use std::path::Path;

use anyhow::bail;

use crate::dict::sources::{self, Source};
use crate::dict::{jmdict, kanjidic, kanjivg, radkfile, tatoeba, wadoku};
use crate::model::SentenceWord;
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
        "kanjidic" => import_kanjidic(db, source, path, report),
        "kanjivg" => import_kanjivg(db, source, path, report),
        "radkfile" => import_radkfile(db, source, path, report),
        "tatoeba" => import_tatoeba(db, source, path, report),
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

fn import_kanjidic(db: &Database, source: &Source, path: &Path, report: Report) -> anyhow::Result<usize> {
    report(format!("Reading {}…", source.name), None);
    let version = kanjidic::created(kanjidic::open(path)?)?;
    db.begin_source(source.id, &now_iso8601())?;
    let mut batch = Vec::with_capacity(BATCH);
    let count = kanjidic::for_each_kanji(kanjidic::open(path)?, |k| {
        batch.push(k);
        if batch.len() >= BATCH {
            db.insert_kanji(&batch)?;
            batch.clear();
        }
        Ok(())
    })?;
    db.insert_kanji(&batch)?;
    db.finish_source(source.id, version.as_deref(), &now_iso8601(), count as i64)?;
    report(format!("Imported {count} kanji."), Some(1.0));
    Ok(count)
}

fn import_kanjivg(db: &Database, source: &Source, path: &Path, report: Report) -> anyhow::Result<usize> {
    report(format!("Reading {}…", source.name), None);
    // The release date is only in the file name of the download; the cache copy has a fixed name,
    // so the version is remembered next to it when downloading.
    let version = std::fs::read_to_string(path.with_extension("version"))
        .ok()
        .map(|v| v.trim().to_string())
        .or_else(|| kanjivg::version_from_name(&path.to_string_lossy()));
    db.begin_source(source.id, &now_iso8601())?;
    let mut batch = Vec::with_capacity(BATCH);
    let count = kanjivg::for_each_kanji(kanjivg::open(path)?, |s| {
        batch.push(s);
        if batch.len() >= BATCH {
            db.insert_strokes(&batch)?;
            batch.clear();
        }
        Ok(())
    })?;
    db.insert_strokes(&batch)?;
    db.finish_source(source.id, version.as_deref(), &now_iso8601(), count as i64)?;
    report(format!("Imported stroke order for {count} kanji."), Some(1.0));
    Ok(count)
}

fn import_radkfile(db: &Database, source: &Source, path: &Path, report: Report) -> anyhow::Result<usize> {
    report(format!("Reading {}…", source.name), None);
    let radicals = radkfile::parse(&radkfile::read(path)?);
    db.begin_source(source.id, &now_iso8601())?;
    db.insert_radicals(&radicals)?;
    let count = radicals.len();
    db.finish_source(source.id, None, &now_iso8601(), count as i64)?;
    report(format!("Imported {count} radicals."), Some(1.0));
    Ok(count)
}

/// Tatoeba: `path` is any one of the export files, the others are looked up next to it. A
/// Japanese sentence is kept when it has a translation or an index line; translations are kept
/// when a kept sentence links to them. The count is the number of Japanese sentences.
fn import_tatoeba(db: &Database, source: &Source, path: &Path, report: Report) -> anyhow::Result<usize> {
    let dir = path.parent().unwrap_or(Path::new("."));
    let file = |name: &str| dir.join(name);
    let version = std::fs::read_to_string(file(source.filename).with_extension("version"))
        .ok()
        .map(|v| v.trim().to_string());
    // Optional files are skipped with a note; only the Japanese sentences are required.
    let optional = |name: &str| -> anyhow::Result<Option<Box<dyn std::io::BufRead>>> {
        let p = file(name);
        if p.exists() {
            Ok(Some(tatoeba::open(&p)?))
        } else {
            log::warn!("no {name} next to {}: skipping it", path.display());
            Ok(None)
        }
    };

    report("Reading the sentence index…".into(), None);
    let mut words: HashMap<i64, Vec<SentenceWord>> = HashMap::new();
    let mut links: Vec<(i64, i64)> = Vec::new();
    if let Some(reader) = optional(tatoeba::JPN_INDICES)? {
        tatoeba::for_each_index(reader, |jpn, eng, w| {
            links.push((jpn, eng));
            words.insert(jpn, w);
            Ok(())
        })?;
    }
    report("Reading the translation links…".into(), None);
    for name in [tatoeba::JPN_ENG_LINKS, tatoeba::JPN_DEU_LINKS] {
        if let Some(reader) = optional(name)? {
            tatoeba::for_each_link(reader, |jpn, other| {
                links.push((jpn, other));
                Ok(())
            })?;
        }
    }
    let translated: HashSet<i64> = links.iter().map(|(jpn, _)| *jpn).collect();

    db.begin_source(source.id, &now_iso8601())?;
    report("Reading the Japanese sentences…".into(), None);
    let mut kept: HashSet<i64> = HashSet::new();
    let mut batch: Vec<(i64, &str, String)> = Vec::with_capacity(BATCH);
    tatoeba::for_each_sentence(tatoeba::open(&file(tatoeba::JPN_SENTENCES))?, |id, text| {
        if translated.contains(&id) || words.contains_key(&id) {
            kept.insert(id);
            batch.push((id, "jpn", text.to_string()));
            if batch.len() >= BATCH {
                db.insert_sentences(&batch)?;
                batch.clear();
            }
        }
        Ok(())
    })?;
    db.insert_sentences(&batch)?;
    batch.clear();
    let count = kept.len();

    let wanted: HashSet<i64> = links
        .iter()
        .filter(|(jpn, _)| kept.contains(jpn))
        .map(|(_, other)| *other)
        .collect();
    let mut stored: HashSet<i64> = HashSet::new();
    for (name, lang, label) in [
        (tatoeba::ENG_SENTENCES, "eng", "English"),
        (tatoeba::DEU_SENTENCES, "deu", "German"),
    ] {
        report(format!("Reading the {label} translations…"), None);
        let Some(reader) = optional(name)? else { continue };
        tatoeba::for_each_sentence(reader, |id, text| {
            if wanted.contains(&id) {
                stored.insert(id);
                batch.push((id, lang, text.to_string()));
                if batch.len() >= BATCH {
                    db.insert_sentences(&batch)?;
                    batch.clear();
                }
            }
            Ok(())
        })?;
        db.insert_sentences(&batch)?;
        batch.clear();
    }

    report("Linking sentences and words…".into(), None);
    links.retain(|(jpn, other)| kept.contains(jpn) && stored.contains(other));
    for chunk in links.chunks(BATCH * 10) {
        db.insert_sentence_links(chunk)?;
    }
    let mut word_rows: Vec<(i64, SentenceWord)> = Vec::with_capacity(BATCH * 10);
    for (jpn, list) in words {
        if !kept.contains(&jpn) {
            continue;
        }
        for w in list {
            word_rows.push((jpn, w));
            if word_rows.len() >= BATCH * 10 {
                db.insert_sentence_words(&word_rows)?;
                word_rows.clear();
            }
        }
    }
    db.insert_sentence_words(&word_rows)?;
    report("Indexing the sentences…".into(), None);
    db.rebuild_sentence_index()?;
    db.finish_source(source.id, version.as_deref(), &now_iso8601(), count as i64)?;
    report(format!("Imported {count} sentences."), Some(1.0));
    Ok(count)
}

/// Downloads `source` (every file it is made of) into `cache` and imports it.
pub fn download_and_import(
    db: &Database,
    source: &Source,
    cache: &Path,
    report: Report,
) -> anyhow::Result<usize> {
    let dest = cache.join(source.filename);
    report(format!("Looking up {}…", source.name), None);
    let url = sources::download_url(source)?;
    // Dated downloads keep their date next to the fixed-name cache copy, as the version.
    if let Some(version) = kanjivg::version_from_name(&url) {
        let _ = std::fs::write(dest.with_extension("version"), version);
    }
    // Exports rebuilt under a fixed name are versioned by their download date.
    if source.id == "tatoeba" {
        let _ = std::fs::write(dest.with_extension("version"), &now_iso8601()[..10]);
    }
    let files = std::iter::once((url.as_str(), dest.clone()))
        .chain(source.extra_files.iter().map(|(u, name)| (*u, cache.join(name))));
    let total_files = 1 + source.extra_files.len();
    for (i, (url, dest)) in files.enumerate() {
        let what = if total_files == 1 {
            source.name.to_string()
        } else {
            format!("{} ({} of {total_files})", source.name, i + 1)
        };
        report(format!("Downloading {what}…"), None);
        sources::download(url, &dest, &mut |done, total| match total {
            Some(total) => report(
                format!(
                    "Downloading {what}… {} of {} MB",
                    done / 1_000_000,
                    total / 1_000_000
                ),
                Some(done as f64 / total as f64),
            ),
            None => report(format!("Downloading {what}… {} MB", done / 1_000_000), None),
        })?;
    }
    import_file(db, source, &dest, report)
}

/// Drops a source's entries and its cached download, then compacts the file.
pub fn remove(db: &Database, source: &Source, cache: &Path, report: Report) -> anyhow::Result<()> {
    report(format!("Removing {}…", source.name), None);
    db.remove_source(source.id)?;
    let names = std::iter::once(source.filename).chain(source.extra_files.iter().map(|(_, n)| *n));
    for name in names {
        match std::fs::remove_file(cache.join(name)) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => log::warn!("could not delete the cached {name}: {e}"),
        }
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
    fn kanji_sources_import() {
        let db = Database::open_in_memory().unwrap();
        let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
        assert_eq!(
            import_file(
                &db,
                &sources::KANJIDIC,
                &fixtures.join("kanjidic2-sample.xml"),
                &mut |_, _| {}
            )
            .unwrap(),
            3
        );
        assert_eq!(
            import_file(
                &db,
                &sources::KANJIVG,
                &fixtures.join("kanjivg-sample.xml"),
                &mut |_, _| {}
            )
            .unwrap(),
            2
        );
        assert_eq!(
            import_file(
                &db,
                &sources::RADKFILE,
                &fixtures.join("radkfile-sample.txt"),
                &mut |_, _| {}
            )
            .unwrap(),
            1
        );
        assert_eq!(db.kanji('猫').unwrap().unwrap().meanings_in("eng"), ["cat"]);
        assert_eq!(db.strokes('猫').unwrap().unwrap().len(), 11);
        assert!(
            db.kanji_with_radicals(&["一".into()], None)
                .unwrap()
                .contains(&'一')
        );
        let status = db.sources().unwrap();
        assert_eq!(status.len(), 3);
        assert_eq!(
            status
                .iter()
                .find(|s| s.id == "kanjidic")
                .unwrap()
                .version
                .as_deref(),
            Some("2026-09-04")
        );
    }

    #[test]
    fn tatoeba_imports_sentences_words_and_translations() {
        let db = Database::open_in_memory().unwrap();
        let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
        import_file(
            &db,
            &sources::JMDICT,
            &fixtures.join("jmdict-sample.xml"),
            &mut |_, _| {},
        )
        .unwrap();
        let mut messages = Vec::new();
        let n = import_file(
            &db,
            &sources::TATOEBA,
            &fixtures.join("tatoeba/jpn_sentences.tsv.bz2"),
            &mut |m, f| messages.push((m, f)),
        )
        .unwrap();
        // 犬が走る。 has neither a translation nor an index line and is left out.
        assert_eq!(n, 3);
        assert_eq!(
            messages.last().unwrap(),
            &("Imported 3 sentences.".to_string(), Some(1.0))
        );
        assert!(db.has_sentences().unwrap());
        let langs = vec!["deu".to_string(), "eng".to_string()];
        let cat = db.get("jmdict", 1467640).unwrap().unwrap();
        let (examples, total) = db.examples(&cat, &langs, 10).unwrap();
        assert_eq!(total, 2);
        // The good example comes first, with the word as it appears in the sentence.
        assert_eq!(examples[0].text, "猫が魚を食べました。");
        assert!(examples[0].good);
        assert_eq!(examples[0].surface.as_deref(), Some("猫"));
        assert_eq!(
            examples[0].translations,
            [("eng".to_string(), "The cat ate a fish.".to_string())]
        );
        assert_eq!(
            examples[1].translations,
            [
                ("deu".to_string(), "Ich mag Katzen.".to_string()),
                ("eng".to_string(), "I like cats.".to_string())
            ]
        );
        // 猫 read ねこま is another entry; the index says ねこ, so it gets nothing.
        let other = db.get("jmdict", 2000003).unwrap().unwrap();
        assert_eq!(db.examples(&other, &langs, 10).unwrap().1, 0);
        let words = db.sentence_words(1).unwrap();
        assert_eq!(words.len(), 5);
        assert_eq!(words[1].seq, Some(2028930));
        assert_eq!(words[4].surface.as_deref(), Some("食べました"));
        // Search: trigram over Japanese and the translations, a scan for short text.
        let found = db.search_sentences("cat", &langs, 10).unwrap();
        assert_eq!(found.len(), 2);
        assert_eq!(found[0].text, "私は猫が好きです。");
        let found = db.search_sentences("読む", &langs, 10).unwrap();
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].translations[0].1, "He reads a book.");
        assert!(db.search_sentences("Nobody", &langs, 10).unwrap().is_empty());
        let status = db.sources().unwrap();
        assert_eq!(status[1].id, "tatoeba");
        assert_eq!(status[1].entries, 3);
        // Removal takes every sentence table with it.
        db.remove_source("tatoeba").unwrap();
        assert!(!db.has_sentences().unwrap());
        assert!(db.search_sentences("cat", &langs, 10).unwrap().is_empty());
        assert_eq!(db.examples(&cat, &langs, 10).unwrap().1, 0);
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
