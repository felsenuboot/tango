//! Word list files in the layouts other tools read: plain CSV for spreadsheets, and Takoboto's
//! own export layout, which can also be read back in.
//!
//! Takoboto (Android) writes `Download/Takoboto/Takoboto.<date>.csv`: comma-separated, UTF-8
//! with a byte-order mark, no header. Column 1 is the list name, column 4 the word and reading
//! joined with `, , `, column 5 the meanings joined with `, , `; the columns in between are not
//! documented, so they are left empty on export and ignored on import.

use crate::model::Entry;
use crate::store::csv;
use crate::store::user::ListEntry;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Layout {
    Csv,
    Takoboto,
}

impl Layout {
    pub fn label(self) -> &'static str {
        match self {
            Layout::Csv => "CSV for spreadsheets",
            Layout::Takoboto => "Takoboto CSV",
        }
    }

    pub fn file_name(self, list: &str) -> String {
        match self {
            Layout::Csv => format!("{list}.csv"),
            Layout::Takoboto => format!("Takoboto.{list}.csv"),
        }
    }
}

/// What an exported row is built from: the list entry, and the dictionary entry behind it when
/// its source is still installed (for meanings beyond the one gloss the list keeps).
pub struct Row<'a> {
    pub list: &'a str,
    pub item: &'a ListEntry,
    pub entry: Option<&'a Entry>,
}

pub const CSV_HEADER: [&str; 5] = ["headword", "reading", "meaning", "note", "added"];

/// Takoboto joins the pieces of a field with this.
const TAKOBOTO_SEP: &str = ", , ";

pub fn render(layout: Layout, rows: &[Row]) -> String {
    let mut out = String::new();
    match layout {
        Layout::Csv => {
            out.push_str(&csv::row(&CSV_HEADER, ','));
            for r in rows {
                let i = r.item;
                out.push_str(&csv::row(
                    &[&i.headword, &i.reading, &i.gloss, &i.note, &i.added],
                    ',',
                ));
            }
        }
        Layout::Takoboto => {
            out.push('\u{feff}');
            for r in rows {
                let word = format!("{}{TAKOBOTO_SEP}{}", r.item.headword, r.item.reading);
                let meanings = match r.entry {
                    Some(e) => e
                        .senses
                        .iter()
                        .map(|s| s.gloss_text("eng"))
                        .filter(|t| !t.is_empty())
                        .collect::<Vec<_>>()
                        .join(TAKOBOTO_SEP),
                    None => r.item.gloss.clone(),
                };
                out.push_str(&csv::row(&[r.list, "", "", &word, &meanings], ','));
            }
        }
    }
    out
}

/// A row of a Takoboto export: the list it belongs to, the word and its reading.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TakobotoRow {
    pub list: String,
    pub headword: String,
    pub reading: String,
}

/// Recognises a Takoboto export (five or more columns, the fourth holding `word, , reading`) and
/// returns its rows; `None` for any other file, which is then read as plain CSV.
pub fn parse_takoboto(rows: &[Vec<String>]) -> Option<Vec<TakobotoRow>> {
    if rows.is_empty() || !rows.iter().all(|r| r.len() >= 5 && !r[3].trim().is_empty()) {
        return None;
    }
    if !rows.iter().any(|r| r[3].contains(TAKOBOTO_SEP)) {
        return None;
    }
    Some(
        rows.iter()
            .map(|r| {
                let mut parts = r[3].split(TAKOBOTO_SEP);
                TakobotoRow {
                    list: r[0].trim().to_string(),
                    headword: parts.next().unwrap_or("").trim().to_string(),
                    reading: parts.next().unwrap_or("").trim().to_string(),
                }
            })
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Gloss, Sense};

    fn item() -> ListEntry {
        ListEntry {
            source: "jmdict".into(),
            seq: 1467640,
            headword: "猫".into(),
            reading: "ねこ".into(),
            gloss: "Katze".into(),
            note: "has a tail".into(),
            added: "2026-09-04T20:00:00Z".into(),
        }
    }

    fn cat() -> Entry {
        let sense = |eng: &str| Sense {
            glosses: vec![Gloss {
                lang: "eng".into(),
                text: eng.into(),
            }],
            ..Sense::default()
        };
        Entry {
            senses: vec![sense("cat"), sense("shamisen")],
            ..Entry::default()
        }
    }

    #[test]
    fn csv_layout() {
        let i = item();
        let text = render(
            Layout::Csv,
            &[Row {
                list: "Favourites",
                item: &i,
                entry: None,
            }],
        );
        assert_eq!(
            text,
            "headword,reading,meaning,note,added\n猫,ねこ,Katze,has a tail,2026-09-04T20:00:00Z\n"
        );
    }

    #[test]
    fn takoboto_round_trip() {
        let i = item();
        let e = cat();
        let text = render(
            Layout::Takoboto,
            &[Row {
                list: "Favourites",
                item: &i,
                entry: Some(&e),
            }],
        );
        assert_eq!(text, "\u{feff}Favourites,,,\"猫, , ねこ\",\"cat, , shamisen\"\n");
        let rows = csv::parse(&text);
        assert_eq!(
            parse_takoboto(&rows).unwrap(),
            [TakobotoRow {
                list: "Favourites".into(),
                headword: "猫".into(),
                reading: "ねこ".into(),
            }]
        );
        let plain = csv::parse("headword,reading\n猫,ねこ\n");
        assert_eq!(parse_takoboto(&plain), None);
        let without_entry = render(
            Layout::Takoboto,
            &[Row {
                list: "L",
                item: &i,
                entry: None,
            }],
        );
        assert!(without_entry.ends_with("\"猫, , ねこ\",Katze\n"));
    }
}
