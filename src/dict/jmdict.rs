//! Streaming reader for the JMdict XML (plain or gzip'd).
//!
//! The pos/misc/field markers are XML entities declared in the file's own DTD (`&n;`, `&v5k;`, ...).
//! quick-xml does not expand those by itself, so the `<!DOCTYPE>` block is read first, its
//! `<!ENTITY>` declarations collected, and every text node is unescaped against that table.
//! A sense therefore carries the long form ("noun (common) (futsuumeishi)").

use std::collections::HashMap;
use std::fs::File;
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom};
use std::path::Path;

use anyhow::{Context, bail};
use quick_xml::escape::{resolve_predefined_entity, unescape_with};
use quick_xml::events::Event;
use quick_xml::{Reader, XmlVersion};

use crate::model::{COMMON_PRIORITIES, Entry, Gloss, Sense, language_name};

/// Opens a JMdict file, transparently gunzipping if it starts with the gzip magic bytes.
pub fn open(path: &Path) -> anyhow::Result<Box<dyn BufRead>> {
    let mut file = File::open(path).with_context(|| format!("opening {}", path.display()))?;
    let mut magic = [0u8; 2];
    let n = file.read(&mut magic)?;
    file.seek(SeekFrom::Start(0))?;
    Ok(if n == 2 && magic == [0x1f, 0x8b] {
        Box::new(BufReader::new(flate2::read::GzDecoder::new(BufReader::new(file))))
    } else {
        Box::new(BufReader::new(file))
    })
}

/// The date from the `<!-- JMdict created: 2026-09-04 -->` comment the file carries before its
/// first entry; it serves as the version. Reads only the head of the file.
pub fn created<R: BufRead>(input: R) -> anyhow::Result<Option<String>> {
    let mut reader = Reader::from_reader(input);
    let mut buf = Vec::new();
    loop {
        match reader.read_event_into(&mut buf)? {
            Event::Eof => return Ok(None),
            Event::Comment(comment) => {
                let text: &str = &comment;
                if let Some(date) = text.trim().strip_prefix("JMdict created:") {
                    return Ok(Some(date.trim().to_string()));
                }
            }
            Event::Start(start) if start.name().as_ref() == "entry" => return Ok(None),
            _ => {}
        }
        buf.clear();
    }
}

/// Calls `f` for every `<entry>` as soon as it is complete; returns how many there were.
/// Streaming keeps memory flat: the full JMdict is ~200k entries and we never hold them all.
pub fn for_each_entry<R: BufRead>(
    input: R,
    mut f: impl FnMut(Entry) -> anyhow::Result<()>,
) -> anyhow::Result<usize> {
    let mut reader = Reader::from_reader(input);
    let mut buf = Vec::new();
    let mut entities: HashMap<String, String> = HashMap::new();
    let mut count = 0;

    // Parser state: the entry and sense being built, the text of the innermost element so far
    // (quick-xml hands text and `&entity;` references over as separate events), the gloss language.
    let mut entry: Option<Entry> = None;
    let mut sense: Option<Sense> = None;
    let mut text = String::new();
    let mut lang = String::from("eng");
    // `<lsource ls_wasei="y">`: a Japanese coinage from foreign parts (wasei eigo).
    let mut wasei = false;

    loop {
        match reader.read_event_into(&mut buf)? {
            Event::Eof => break,
            Event::DocType(doctype) => entities = parse_entities(&doctype),
            Event::Start(start) => {
                text.clear();
                match start.name().as_ref() {
                    "entry" => entry = Some(Entry::default()),
                    "sense" => sense = Some(Sense::default()),
                    "gloss" | "lsource" => {
                        lang = "eng".to_string();
                        wasei = false;
                        for attr in start.attributes().flatten() {
                            let value = attr.normalized_value(XmlVersion::Implicit1_0)?;
                            match attr.key.as_ref() {
                                "xml:lang" => lang = value.into_owned(),
                                "ls_wasei" => wasei = value == "y",
                                _ => {}
                            }
                        }
                    }
                    _ => {}
                }
            }
            // `<lsource xml:lang="fre"/>` and `<re_nokanji/>`: an element without text.
            Event::Empty(start) => {
                let name = start.name().as_ref().to_string();
                lang = "eng".to_string();
                wasei = false;
                for attr in start.attributes().flatten() {
                    let value = attr.normalized_value(XmlVersion::Implicit1_0)?;
                    match attr.key.as_ref() {
                        "xml:lang" => lang = value.into_owned(),
                        "ls_wasei" => wasei = value == "y",
                        _ => {}
                    }
                }
                if let Some(e) = entry.as_mut() {
                    apply_text(e, sense.as_mut(), &name, &lang, wasei, String::new())?;
                }
            }
            Event::Text(t) => {
                let raw: &str = &t;
                let value = unescape_with(raw, |name| resolve(&entities, name))
                    .with_context(|| format!("unescaping {raw:?}"))?;
                text.push_str(&value);
            }
            Event::GeneralRef(r) => {
                if let Some(c) = r.resolve_char_ref()? {
                    text.push(c);
                } else if let Some(value) = resolve(&entities, &r) {
                    text.push_str(value);
                } else {
                    bail!("unknown entity &{};", &*r);
                }
            }
            Event::End(end) => {
                // Trim once here, not per fragment: "a &amp; b" arrives as three events.
                let value = std::mem::take(&mut text).trim().to_string();
                match end.name().as_ref() {
                    "sense" => {
                        if let (Some(e), Some(s)) = (entry.as_mut(), sense.take()) {
                            e.senses.push(s);
                        }
                    }
                    "entry" => {
                        let Some(e) = entry.take() else {
                            bail!("</entry> without <entry>")
                        };
                        f(e)?;
                        count += 1;
                    }
                    name => {
                        if let Some(e) = entry.as_mut() {
                            apply_text(e, sense.as_mut(), name, &lang, wasei, value)?;
                        }
                    }
                }
            }
            _ => {}
        }
        buf.clear();
    }
    Ok(count)
}

fn resolve<'a>(entities: &'a HashMap<String, String>, name: &str) -> Option<&'a str> {
    entities
        .get(name)
        .map(String::as_str)
        .or_else(|| resolve_predefined_entity(name))
}

/// Stores the text of a leaf element on the entry or sense being built.
fn apply_text(
    e: &mut Entry,
    sense: Option<&mut Sense>,
    name: &str,
    lang: &str,
    wasei: bool,
    value: String,
) -> anyhow::Result<()> {
    match name {
        "ent_seq" => e.id = value.parse().with_context(|| format!("bad ent_seq {value:?}"))?,
        "keb" => {
            e.kanji.push(value);
            e.kanji_info.push(Vec::new());
        }
        "reb" => {
            e.readings.push(value);
            e.reading_info.push(Vec::new());
            e.reading_for.push(Vec::new());
        }
        "ke_inf" => push_last(&mut e.kanji_info, value),
        "re_inf" => push_last(&mut e.reading_info, value),
        "re_restr" => push_last(&mut e.reading_for, value),
        "ke_pri" | "re_pri" => e.common |= COMMON_PRIORITIES.contains(&value.as_str()),
        "pos" | "misc" | "field" | "gloss" | "s_inf" | "dial" | "lsource" | "xref" | "ant" | "stagk"
        | "stagr" => {
            if let Some(s) = sense {
                match name {
                    "pos" => s.pos.push(value),
                    "misc" => s.misc.push(value),
                    "field" => s.fields.push(value),
                    "s_inf" => s.info.push(value),
                    "dial" => s.dialects.push(value),
                    "lsource" => s.origins.push(origin(lang, wasei, &value)),
                    "xref" => s.see_also.push(value),
                    "ant" => s.antonyms.push(value),
                    "stagk" | "stagr" => s.only_for.push(value),
                    _ => s.glosses.push(Gloss {
                        lang: lang.to_string(),
                        text: value,
                    }),
                }
            }
        }
        _ => {}
    }
    Ok(())
}

fn push_last(lists: &mut [Vec<String>], value: String) {
    if let Some(last) = lists.last_mut() {
        last.push(value);
    }
}

/// "from English: cat", "wasei, from English: salaryman", or just "from French" when the
/// source word is not given.
fn origin(lang: &str, wasei: bool, word: &str) -> String {
    let mut out = if wasei { "wasei, from " } else { "from " }.to_string();
    out.push_str(&language_name(lang));
    if !word.is_empty() {
        out.push_str(": ");
        out.push_str(word);
    }
    out
}

/// Collects `<!ENTITY name "value">` declarations from the DOCTYPE internal subset.
fn parse_entities(doctype: &str) -> HashMap<String, String> {
    let mut map = HashMap::new();
    for chunk in doctype.split("<!ENTITY ").skip(1) {
        let chunk = chunk.trim_start();
        let Some((name, rest)) = chunk.split_once(char::is_whitespace) else {
            continue;
        };
        let rest = rest.trim_start();
        let Some(quote) = rest.chars().next().filter(|c| *c == '"' || *c == '\'') else {
            continue;
        };
        let Some(value) = rest[1..].split(quote).next() else {
            continue;
        };
        map.insert(name.to_string(), value.to_string());
    }
    map
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = include_str!("../../tests/fixtures/jmdict-sample.xml");

    fn parse_sample() -> Vec<Entry> {
        let mut out = Vec::new();
        let n = for_each_entry(SAMPLE.as_bytes(), |e| {
            out.push(e);
            Ok(())
        })
        .unwrap();
        assert_eq!(n, out.len());
        out
    }

    #[test]
    fn parses_sample_entries() {
        let entries = parse_sample();
        let ids: Vec<i64> = entries.iter().map(|e| e.id).collect();
        assert_eq!(
            ids,
            [1467640, 1000225, 1236120, 2000001, 2000002, 2000003, 2000004]
        );
        let cat = &entries[0];
        assert_eq!(cat.kanji, ["猫"]);
        assert_eq!(cat.readings, ["ねこ", "ネコ"]);
        assert!(cat.common);
        assert_eq!(cat.senses[0].pos, ["noun (common) (futsuumeishi)"]); // the DTD entity, expanded
        assert_eq!(
            cat.senses[0].gloss_text("eng"),
            "cat (esp. the domestic cat, Felis catus)"
        );
        assert_eq!(cat.senses[0].gloss_text("ger"), "Katze; Hauskatze");
        assert_eq!(cat.senses[1].misc, ["word usually written using kana alone"]);
        assert_eq!(cat.languages(), ["eng", "ger"]);
    }

    #[test]
    fn entry_details_are_read() {
        let entries = parse_sample();
        let stoop = entries.iter().find(|e| e.id == 2000002).unwrap();
        assert_eq!(stoop.kanji, ["猫背", "猫脊"]);
        assert_eq!(
            stoop.kanji_info,
            [vec![], vec!["rarely used kanji form".to_string()]]
        );
        assert_eq!(
            stoop.reading_info,
            [vec![
                "gikun (meaning as reading) or jukujikun (special kanji reading)".to_string()
            ]]
        );
        assert_eq!(stoop.reading_for, [vec!["猫背".to_string()]]);
        let s = &stoop.senses[0];
        assert_eq!(s.only_for, ["猫背"]);
        assert_eq!(s.info, ["often of a person"]);
        assert_eq!(s.dialects, ["Kansai-ben"]);
        assert_eq!(s.origins, ["wasei, from English: stoop", "from French"]);
        assert_eq!(s.see_also, ["猫・ねこ・1"]);
        assert_eq!(s.antonyms, ["背筋・せすじ"]);
        assert_eq!(s.gloss_text("eng"), "bent back; hunchback; stoop");
    }

    #[test]
    fn kana_only_entry_has_no_kanji() {
        let entries = parse_sample();
        let foxtail = entries.iter().find(|e| e.id == 2000001).unwrap();
        assert!(foxtail.kanji.is_empty());
        assert_eq!(foxtail.headword(), "ねこじゃらし");
        assert!(!foxtail.common);
        assert_eq!(
            foxtail.summary(&["ger".into(), "eng".into()]),
            "Grüne Borstenhirse"
        );
        assert_eq!(
            foxtail.summary(&["eng".into()]),
            "green foxtail (Setaria viridis)"
        );
    }

    #[test]
    fn predefined_entities_still_work() {
        let entries = parse_sample();
        let amp = entries.iter().find(|e| e.id == 2000003).unwrap();
        assert_eq!(
            amp.senses[0].gloss_text("eng"),
            "cat (archaic reading, for the test) & more"
        );
    }

    #[test]
    fn created_comment_is_the_version() {
        assert_eq!(created(SAMPLE.as_bytes()).unwrap().as_deref(), Some("2024-01-01"));
        assert_eq!(
            created("<JMdict><entry></entry></JMdict>".as_bytes()).unwrap(),
            None
        );
    }

    #[test]
    fn gzip_is_detected() {
        use std::io::Write;
        let dir = std::env::temp_dir().join(format!("tango-jmdict-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let gz = dir.join("JMdict.gz");
        let mut enc = flate2::write::GzEncoder::new(File::create(&gz).unwrap(), flate2::Compression::fast());
        enc.write_all(SAMPLE.as_bytes()).unwrap();
        enc.finish().unwrap();
        let n = for_each_entry(open(&gz).unwrap(), |_| Ok(())).unwrap();
        assert_eq!(n, 7);
        std::fs::remove_dir_all(dir).unwrap();
    }
}
