//! Streaming reader for KANJIDIC2 (`kanjidic2.xml.gz`): one `<character>` per kanji with its
//! `<literal>`, `<misc>` (grade, stroke_count, freq, jlpt), classical `<radical>`, and
//! `<reading_meaning>` with on/kun readings, meanings per language, and nanori.

use std::io::BufRead;
use std::path::Path;

use anyhow::{Context, bail};
use quick_xml::events::Event;
use quick_xml::{Reader, XmlVersion};

use crate::model::{Gloss, Kanji};

pub use crate::dict::jmdict::open;

/// The `<date_of_creation>` in the header, the version of the file.
pub fn created<R: BufRead>(input: R) -> anyhow::Result<Option<String>> {
    let mut reader = Reader::from_reader(input);
    let mut buf = Vec::new();
    let mut in_date = false;
    loop {
        match reader.read_event_into(&mut buf)? {
            Event::Eof => return Ok(None),
            Event::Start(s) if s.name().as_ref() == "date_of_creation" => in_date = true,
            Event::Start(s) if s.name().as_ref() == "character" => return Ok(None),
            Event::Text(t) if in_date => return Ok(Some(t.trim().to_string())),
            _ => {}
        }
        buf.clear();
    }
}

/// ISO 639-2 for KANJIDIC2's two-letter meaning languages, so kanji and entries agree.
fn lang_code(m_lang: Option<&str>) -> &'static str {
    match m_lang {
        None | Some("en") => "eng",
        Some("fr") => "fre",
        Some("es") => "spa",
        Some("pt") => "por",
        Some("de") => "ger",
        _ => "und",
    }
}

/// Calls `f` for every `<character>`; returns how many there were.
pub fn for_each_kanji<R: BufRead>(
    input: R,
    mut f: impl FnMut(Kanji) -> anyhow::Result<()>,
) -> anyhow::Result<usize> {
    let mut reader = Reader::from_reader(input);
    let mut buf = Vec::new();
    let mut count = 0;
    let mut kanji: Option<Kanji> = None;
    // The leaf element whose text is being collected, with the attribute that qualifies it.
    let mut leaf: Option<(String, Option<String>)> = None;
    let mut text = String::new();
    let mut classical_radical = false;
    loop {
        match reader.read_event_into(&mut buf)? {
            Event::Eof => break,
            Event::Start(start) => {
                let name = start.name().as_ref().to_string();
                if name == "character" {
                    kanji = Some(Kanji::default());
                    continue;
                }
                if kanji.is_none() {
                    continue;
                }
                let mut qualifier = None;
                for a in start.attributes().flatten() {
                    let key = a.key.as_ref();
                    if matches!(key, "r_type" | "m_lang" | "rad_type") {
                        qualifier = Some(a.normalized_value(XmlVersion::Implicit1_0)?.into_owned());
                    }
                }
                classical_radical = name == "rad_value" && qualifier.as_deref() == Some("classical");
                leaf = Some((name, qualifier));
                text.clear();
            }
            Event::Text(t) => {
                if leaf.is_some() {
                    text.push_str(&t);
                }
            }
            Event::End(end) => {
                let name = end.name();
                let name: &str = name.as_ref();
                if name == "character" {
                    let Some(k) = kanji.take() else {
                        bail!("</character> without <character>")
                    };
                    f(k)?;
                    count += 1;
                    continue;
                }
                let (Some(k), Some((leaf_name, qualifier))) = (kanji.as_mut(), leaf.take()) else {
                    continue;
                };
                if leaf_name != name {
                    continue;
                }
                let value = text.trim().to_string();
                let number = || {
                    value
                        .parse::<u16>()
                        .with_context(|| format!("{leaf_name} {value:?}"))
                };
                match leaf_name.as_str() {
                    "literal" => k.literal = value.chars().next().unwrap_or('\u{fffd}'),
                    "grade" => k.grade = Some(number()? as u8),
                    "stroke_count" if k.strokes == 0 => k.strokes = number()? as u8,
                    "freq" => k.freq = Some(number()?),
                    "jlpt" => k.jlpt = Some(number()? as u8),
                    "rad_value" if classical_radical => k.radical = number()? as u8,
                    "reading" => match qualifier.as_deref() {
                        Some("ja_on") => k.on.push(value),
                        Some("ja_kun") => k.kun.push(value),
                        _ => {}
                    },
                    "meaning" => k.meanings.push(Gloss {
                        lang: lang_code(qualifier.as_deref()).to_string(),
                        text: value,
                    }),
                    "nanori" => k.nanori.push(value),
                    _ => {}
                }
            }
            _ => {}
        }
        buf.clear();
    }
    Ok(count)
}

#[allow(dead_code)]
pub fn count_in(path: &Path) -> anyhow::Result<usize> {
    for_each_kanji(open(path)?, |_| Ok(()))
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = include_str!("../../tests/fixtures/kanjidic2-sample.xml");

    #[test]
    fn characters_readings_and_meanings() {
        let mut out = Vec::new();
        let n = for_each_kanji(SAMPLE.as_bytes(), |k| {
            out.push(k);
            Ok(())
        })
        .unwrap();
        assert_eq!(n, 3);
        assert_eq!(created(SAMPLE.as_bytes()).unwrap().as_deref(), Some("2026-09-04"));
        let cat = out.iter().find(|k| k.literal == '猫').unwrap();
        assert_eq!(
            (cat.grade, cat.strokes, cat.freq, cat.jlpt, cat.radical),
            (Some(8), 11, Some(1702), Some(2), 94)
        );
        assert_eq!(cat.on, ["ビョウ"]);
        assert!(cat.kun.contains(&"ねこ".to_string()));
        assert_eq!(cat.meanings_in("eng"), ["cat"]);
        assert_eq!(cat.meanings_in("fre"), ["chat"]);
        let one = out.iter().find(|k| k.literal == '一').unwrap();
        assert_eq!((one.grade, one.strokes), (Some(1), 1));
        assert!(!one.nanori.is_empty());
    }
}
