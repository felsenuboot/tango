//! Streaming reader for the KanjiVG single-file release (`kanjivg-<date>.xml.gz`): one `<kanji
//! id="kvg:kanji_<hex>">` per character, with nested `<g>` groups for the components and one
//! `<path d="…">` per stroke in stroke order, in a 109×109 box.

use std::io::BufRead;

use anyhow::bail;
use quick_xml::Reader;
use quick_xml::events::Event;

use crate::model::Strokes;

pub use crate::dict::jmdict::open;

/// The date in a KanjiVG file name (`kanjivg-20250816.xml.gz`), the version of the release.
pub fn version_from_name(name: &str) -> Option<String> {
    let start = name.find("kanjivg-")? + "kanjivg-".len();
    let digits: String = name[start..].chars().take_while(char::is_ascii_digit).collect();
    (digits.len() == 8).then(|| format!("{}-{}-{}", &digits[..4], &digits[4..6], &digits[6..]))
}

/// The newest release's single XML on GitHub's releases API page (JSON text).
pub fn latest_url(page: &str) -> Option<String> {
    let mut rest = page;
    while let Some(i) = rest.find("https://github.com/KanjiVG/kanjivg/releases/download/") {
        let candidate = &rest[i..];
        let end = candidate.find('"').unwrap_or(candidate.len());
        let url = &candidate[..end];
        if url.ends_with(".xml.gz") {
            return Some(url.to_string());
        }
        rest = &rest[i + 1..];
    }
    None
}

/// Calls `f` for every `<kanji>`; returns how many there were.
pub fn for_each_kanji<R: BufRead>(
    input: R,
    mut f: impl FnMut(Strokes) -> anyhow::Result<()>,
) -> anyhow::Result<usize> {
    let mut reader = Reader::from_reader(input);
    let mut buf = Vec::new();
    let mut count = 0;
    let mut current: Option<Strokes> = None;
    loop {
        match reader.read_event_into(&mut buf)? {
            Event::Eof => break,
            Event::Start(start) if start.name().as_ref() == "kanji" => {
                let mut literal = None;
                for a in start.attributes().flatten() {
                    if a.key.as_ref() == "id" {
                        let hex = a.value.rsplit('_').next().unwrap_or("");
                        literal = u32::from_str_radix(hex, 16).ok().and_then(char::from_u32);
                    }
                }
                let Some(literal) = literal else {
                    bail!("kanji element without a code point id")
                };
                current = Some(Strokes {
                    literal,
                    paths: Vec::new(),
                });
            }
            Event::Empty(start) | Event::Start(start) if start.name().as_ref() == "path" => {
                if let Some(s) = current.as_mut() {
                    for a in start.attributes().flatten() {
                        if a.key.as_ref() == "d" {
                            s.paths.push(a.value.to_string());
                        }
                    }
                }
            }
            Event::End(end) if end.name().as_ref() == "kanji" => {
                if let Some(s) = current.take() {
                    f(s)?;
                    count += 1;
                }
            }
            _ => {}
        }
        buf.clear();
    }
    Ok(count)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = include_str!("../../tests/fixtures/kanjivg-sample.xml");

    #[test]
    fn strokes_in_order() {
        let mut out = Vec::new();
        let n = for_each_kanji(SAMPLE.as_bytes(), |s| {
            out.push(s);
            Ok(())
        })
        .unwrap();
        assert_eq!(n, 2);
        assert_eq!(out[0].literal, '猫');
        assert_eq!(out[0].paths.len(), 11);
        assert!(out[0].paths[0].starts_with("M36.05,19.5"));
        assert_eq!(out[1].literal, '一');
        assert_eq!(out[1].paths.len(), 1);
    }

    #[test]
    fn version_and_latest() {
        assert_eq!(
            version_from_name("kanjivg-20250816.xml.gz").as_deref(),
            Some("2025-08-16")
        );
        assert_eq!(version_from_name("kanjivg.xml.gz"), None);
        let page = r#"{"assets":[{"browser_download_url":"https://github.com/KanjiVG/kanjivg/releases/download/r20250816/kanjivg-20250816-all.zip"},{"browser_download_url":"https://github.com/KanjiVG/kanjivg/releases/download/r20250816/kanjivg-20250816.xml.gz"}]}"#;
        assert_eq!(
            latest_url(page).as_deref(),
            Some("https://github.com/KanjiVG/kanjivg/releases/download/r20250816/kanjivg-20250816.xml.gz")
        );
    }
}
