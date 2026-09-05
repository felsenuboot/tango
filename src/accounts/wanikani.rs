//! WaniKani API v2 (<https://docs.api.wanikani.com/20170710/>): a bearer token, JSON, paged
//! collections with `pages.next_url`, `updated_after` for incremental syncs, sixty requests a
//! minute. Subjects (kanji, vocabulary, kana vocabulary) and assignments (the SRS stage per
//! subject) land in the user database; `learned` is rebuilt from them after every sync.
//!
//! The HTTP side is a thin layer over `fetch`; everything after the JSON is parsed is plain
//! functions over the data, which the tests drive with fixtures.

use std::time::Duration;

use anyhow::{Context, bail};
use serde::Deserialize;
use serde_json::Value;

use super::Kind;
use crate::store::import::Report;
use crate::store::user::UserDb;

pub const PROVIDER: &str = "wanikani";
const API: &str = "https://api.wanikani.com/v2";
const REVISION: &str = "20170710";

/// `/user`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Account {
    pub username: String,
    pub level: u32,
}

/// A page of a collection, or a single resource wrapped the same way.
#[derive(Debug, Deserialize)]
pub struct Collection {
    #[serde(default)]
    pub pages: Pages,
    pub data_updated_at: Option<String>,
    #[serde(default)]
    pub data: Vec<Resource>,
}

#[derive(Debug, Default, Deserialize)]
pub struct Pages {
    pub next_url: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct Resource {
    pub id: i64,
    pub object: String,
    pub data: Value,
}

/// A kanji or vocabulary subject as stored: WaniKani's number, its kind, the characters, level.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Subject {
    pub id: i64,
    pub kind: Kind,
    pub text: String,
    /// The primary reading, empty when WaniKani lists none.
    pub reading: String,
    pub level: u32,
    pub hidden: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Assignment {
    pub subject_id: i64,
    pub stage: u8,
    pub started: Option<String>,
    pub passed: Option<String>,
    pub burned: Option<String>,
    pub hidden: bool,
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct SyncStats {
    pub subjects: usize,
    pub assignments: usize,
    pub learned: usize,
}

/// The subjects on a page; radicals and anything without characters are skipped.
pub fn subjects(page: &Collection) -> Vec<Subject> {
    page.data
        .iter()
        .filter_map(|r| {
            let kind = match r.object.as_str() {
                "kanji" => Kind::Kanji,
                "vocabulary" | "kana_vocabulary" => Kind::Vocabulary,
                _ => return None,
            };
            let text = r.data.get("characters")?.as_str()?.to_string();
            let readings = r.data.get("readings").and_then(Value::as_array);
            let reading = readings
                .and_then(|rs| {
                    rs.iter()
                        .find(|x| x.get("primary").and_then(Value::as_bool).unwrap_or(false))
                        .or(rs.first())
                })
                .and_then(|x| x.get("reading"))
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();
            Some(Subject {
                id: r.id,
                kind,
                text,
                reading,
                level: r.data.get("level").and_then(Value::as_u64).unwrap_or(0) as u32,
                hidden: r.data.get("hidden_at").is_some_and(|h| !h.is_null()),
            })
        })
        .collect()
}

pub fn assignments(page: &Collection) -> Vec<Assignment> {
    let date = |v: &Value, key: &str| v.get(key).and_then(Value::as_str).map(str::to_string);
    page.data
        .iter()
        .filter(|r| r.object == "assignment")
        .filter_map(|r| {
            Some(Assignment {
                subject_id: r.data.get("subject_id")?.as_i64()?,
                stage: r
                    .data
                    .get("srs_stage")
                    .and_then(Value::as_u64)
                    .unwrap_or(0)
                    .min(9) as u8,
                started: date(&r.data, "started_at"),
                passed: date(&r.data, "passed_at"),
                burned: date(&r.data, "burned_at"),
                hidden: r.data.get("hidden").and_then(Value::as_bool).unwrap_or(false),
            })
        })
        .collect()
}

/// How long to wait after a 429, from its `Retry-After` header (seconds; WaniKani sends it):
/// a minute when it is missing or unreadable, five at most.
fn retry_after(header: Option<&str>) -> Duration {
    let seconds = header
        .and_then(|v| v.trim().parse::<u64>().ok())
        .unwrap_or(60)
        .clamp(1, 300);
    Duration::from_secs(seconds)
}

/// Sixty requests a minute; a full first sync is about twenty, so two waits cover a user who
/// connects twice in a row or syncs right after another WaniKani client (#109).
const RETRIES: usize = 2;

fn get(token: &str, url: &str, report: Report) -> anyhow::Result<Value> {
    for attempt in 0..=RETRIES {
        // A page is a megabyte at most: one minute for the whole request (#100). Statuses are
        // not errors here, so a 429's Retry-After can be read.
        let mut response = crate::dict::sources::agent()
            .get(url)
            .config()
            .timeout_global(Some(Duration::from_secs(60)))
            .http_status_as_error(false)
            .build()
            .header("Authorization", &format!("Bearer {token}"))
            .header("Wanikani-Revision", REVISION)
            .call()
            .with_context(|| format!("GET {url}"))?;
        match response.status().as_u16() {
            200 => {
                let text = response
                    .body_mut()
                    .read_to_string()
                    .with_context(|| format!("reading {url}"))?;
                return serde_json::from_str(&text).with_context(|| format!("parsing the reply from {url}"));
            }
            401 => bail!("WaniKani rejected the token (401)"),
            429 if attempt < RETRIES => {
                let wait = retry_after(
                    response
                        .headers()
                        .get("retry-after")
                        .and_then(|v| v.to_str().ok()),
                );
                report(
                    format!("WaniKani: rate limit, waiting {} s…", wait.as_secs()),
                    None,
                );
                log::info!("wanikani: 429 for {url}, waiting {} s", wait.as_secs());
                std::thread::sleep(wait);
            }
            429 => bail!("WaniKani rate limit hit (429) three times; try again in a minute"),
            status => bail!("WaniKani answered {status} for {url}"),
        }
    }
    unreachable!("the loop returns or bails")
}

/// Checks the token and tells who it belongs to.
pub fn user(token: &str, report: Report) -> anyhow::Result<Account> {
    let value = get(token, &format!("{API}/user"), report)?;
    let data = value.get("data").context("no user data in the reply")?;
    Ok(Account {
        username: data
            .get("username")
            .and_then(Value::as_str)
            .unwrap_or("?")
            .to_string(),
        level: data.get("level").and_then(Value::as_u64).unwrap_or(0) as u32,
    })
}

/// Fetches every page from `url` on, calling `f` with each; returns the newest
/// `data_updated_at` seen, the next sync's `updated_after`.
fn each_page(
    token: &str,
    url: &str,
    report: Report,
    mut f: impl FnMut(&Collection, Report) -> anyhow::Result<()>,
) -> anyhow::Result<Option<String>> {
    let mut next = Some(url.to_string());
    let mut newest: Option<String> = None;
    while let Some(url) = next.take() {
        let page: Collection = serde_json::from_value(get(token, &url, report)?)
            .with_context(|| format!("unexpected reply from {url}"))?;
        if let Some(stamp) = &page.data_updated_at
            && newest.as_ref().is_none_or(|n| stamp > n)
        {
            newest = Some(stamp.clone());
        }
        f(&page, report)?;
        next = page.pages.next_url;
    }
    Ok(newest)
}

/// A full or incremental sync into `db`: subjects and assignments changed since the last run,
/// then the `learned` rows rebuilt. Progress goes to `report`.
pub fn sync(token: &str, db: &UserDb, report: Report) -> anyhow::Result<SyncStats> {
    let mut stats = SyncStats::default();
    let since = |key: &str| -> anyhow::Result<String> {
        Ok(db
            .sync_state(PROVIDER, key)?
            .map(|s| format!("&updated_after={s}"))
            .unwrap_or_default())
    };
    report("WaniKani: reading the subjects…".into(), None);
    let url = format!(
        "{API}/subjects?types=kanji,vocabulary,kana_vocabulary{}",
        since("subjects")?
    );
    let newest = each_page(token, &url, report, |page, report| {
        let batch = subjects(page);
        stats.subjects += batch.len();
        db.upsert_wk_subjects(&batch)?;
        report(format!("WaniKani: {} subjects…", stats.subjects), None);
        Ok(())
    })?;
    if let Some(stamp) = newest {
        db.set_sync_state(PROVIDER, "subjects", &stamp)?;
    }
    report("WaniKani: reading the assignments…".into(), None);
    let url = format!(
        "{API}/assignments?subject_types=kanji,vocabulary,kana_vocabulary{}",
        since("assignments")?
    );
    let newest = each_page(token, &url, report, |page, report| {
        let batch = assignments(page);
        stats.assignments += batch.len();
        db.upsert_wk_assignments(&batch)?;
        report(format!("WaniKani: {} assignments…", stats.assignments), None);
        Ok(())
    })?;
    if let Some(stamp) = newest {
        db.set_sync_state(PROVIDER, "assignments", &stamp)?;
    }
    stats.learned = db.rebuild_learned_wanikani()?;
    if stats.learned == 0 && stats.subjects == 0 {
        bail!("WaniKani returned nothing; is the token allowed to read subjects?");
    }
    report(format!("WaniKani: {} items synced.", stats.learned), Some(1.0));
    Ok(stats)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SUBJECTS: &str = include_str!("../../tests/fixtures/wanikani-subjects.json");
    const ASSIGNMENTS: &str = include_str!("../../tests/fixtures/wanikani-assignments.json");

    #[test]
    fn retry_after_is_seconds_with_a_default_and_a_cap() {
        assert_eq!(retry_after(Some("30")), Duration::from_secs(30));
        assert_eq!(retry_after(Some(" 5 ")), Duration::from_secs(5));
        assert_eq!(retry_after(None), Duration::from_secs(60));
        assert_eq!(
            retry_after(Some("Wed, 21 Oct 2026 07:28:00 GMT")),
            Duration::from_secs(60)
        );
        assert_eq!(retry_after(Some("0")), Duration::from_secs(1));
        assert_eq!(retry_after(Some("100000")), Duration::from_secs(300));
    }

    #[test]
    fn subjects_page_parses() {
        let page: Collection = serde_json::from_str(SUBJECTS).unwrap();
        assert_eq!(
            page.pages.next_url.as_deref(),
            Some("https://api.wanikani.com/v2/subjects?page_after_id=1000")
        );
        let list = subjects(&page);
        assert_eq!(list.len(), 3); // the radical is skipped
        assert_eq!(
            list[0],
            Subject {
                id: 440,
                kind: Kind::Kanji,
                text: "一".into(),
                reading: "いち".into(),
                level: 1,
                hidden: false
            }
        );
        assert_eq!(list[1].kind, Kind::Vocabulary);
        assert_eq!(list[1].text, "食べる");
        assert_eq!(list[1].reading, "たべる");
        assert_eq!(list[2].reading, ""); // kana vocabulary lists no readings
        assert_eq!(list[2].text, "こんにちは"); // kana vocabulary
        assert!(list[2].hidden);
    }

    #[test]
    fn assignments_page_parses() {
        let page: Collection = serde_json::from_str(ASSIGNMENTS).unwrap();
        assert!(page.pages.next_url.is_none());
        let list = assignments(&page);
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].subject_id, 440);
        assert_eq!(list[0].stage, 9);
        assert_eq!(list[0].burned.as_deref(), Some("2024-05-01T00:00:00.000000Z"));
        assert_eq!(list[1].stage, 5);
        assert!(list[1].burned.is_none());
    }

    #[test]
    fn learned_rows_come_from_subjects_and_assignments() {
        let db = UserDb::open_in_memory().unwrap();
        let page: Collection = serde_json::from_str(SUBJECTS).unwrap();
        db.upsert_wk_subjects(&subjects(&page)).unwrap();
        let page: Collection = serde_json::from_str(ASSIGNMENTS).unwrap();
        db.upsert_wk_assignments(&assignments(&page)).unwrap();
        assert_eq!(db.rebuild_learned_wanikani().unwrap(), 2); // the hidden subject is left out
        let one = db.learned_for(&["一".to_string()]).unwrap();
        assert_eq!(one.len(), 1);
        assert_eq!((one[0].kind, one[0].level, one[0].stage), (Kind::Kanji, 1, 9));
        let index = db.learned_index().unwrap();
        assert_eq!(
            index.get(&(Kind::Vocabulary, "食べる".to_string())),
            Some(&(6, 5))
        );
        assert_eq!(db.learned_count(PROVIDER).unwrap(), 2);
        db.clear_provider(PROVIDER).unwrap();
        assert_eq!(db.learned_count(PROVIDER).unwrap(), 0);
        assert!(db.sync_state(PROVIDER, "subjects").unwrap().is_none());
    }
}
