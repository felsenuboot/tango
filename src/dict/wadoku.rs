//! Streaming reader for the Wadoku XML dump (Japanese–German, with pitch accent).
//!
//! The dump is `wadoku-xml-<date>.tar.xz` with `wadoku.xml` inside: an `<entries date=…>` root and
//! one `<entry id=…>` per word. Each has `<form>` with `<orth>` spellings (one marked `midashigo`
//! is the display form with optional okurigana in parentheses and is skipped) and a `<reading>`
//! with `<hira>` and zero or more `<accent>` numbers (the mora after which the pitch drops, 0 for
//! flat); `<gramGrp>` with the part of speech as an element (`<meishi/>`, `<doushi level="5"
//! godanrow="ra"/>` …); and `<sense>` elements with `<usg type="dom">` domains and `<trans><tr>`
//! translations, whose text may carry inline markup (`<token>`, `<expl>` …). Parts of speech are
//! mapped to JMdict's wording so the deinflection and the `#tags` work on both dictionaries.

use std::fs::File;
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use anyhow::{Context, bail};
use quick_xml::events::{BytesStart, Event};
use quick_xml::{Reader, XmlVersion};

use crate::model::{Entry, Gloss, Sense};

/// Unpacks `wadoku.xml` out of the `.tar.xz` next to it and returns its path; a plain XML file is
/// returned as is.
pub fn unpack(path: &Path) -> anyhow::Result<PathBuf> {
    let mut file = File::open(path).with_context(|| format!("opening {}", path.display()))?;
    let mut magic = [0u8; 6];
    let n = file.read(&mut magic)?;
    file.seek(SeekFrom::Start(0))?;
    if n < 6 || magic != [0xfd, b'7', b'z', b'X', b'Z', 0x00] {
        return Ok(path.to_path_buf());
    }
    let dest = path.with_file_name("wadoku.xml");
    let decoder = liblzma::read::XzDecoder::new(BufReader::new(file));
    let mut archive = tar::Archive::new(decoder);
    for entry in archive.entries()? {
        let mut entry = entry?;
        let name = entry.path()?.to_path_buf();
        if name.file_name().is_some_and(|f| f == "wadoku.xml") {
            let part = dest.with_extension("part");
            let mut out = File::create(&part)?;
            std::io::copy(&mut entry, &mut out)?;
            std::fs::rename(&part, &dest)?;
            return Ok(dest);
        }
    }
    bail!("no wadoku.xml in {}", path.display())
}

pub fn open(path: &Path) -> anyhow::Result<Box<dyn BufRead>> {
    let file = File::open(path).with_context(|| format!("opening {}", path.display()))?;
    Ok(Box::new(BufReader::new(file)))
}

/// The `date` of the `<entries>` root, the version of the dump.
pub fn created<R: BufRead>(input: R) -> anyhow::Result<Option<String>> {
    let mut reader = Reader::from_reader(input);
    let mut buf = Vec::new();
    loop {
        match reader.read_event_into(&mut buf)? {
            Event::Eof => return Ok(None),
            Event::Start(start) if start.name().as_ref() == "entries" => {
                return Ok(attr(&start, "date")?.map(|d| d.get(..10).unwrap_or(&d).to_string()));
            }
            Event::Start(_) => return Ok(None),
            _ => {}
        }
        buf.clear();
    }
}

fn attr(start: &BytesStart, name: &str) -> anyhow::Result<Option<String>> {
    for a in start.attributes().flatten() {
        if a.key.as_ref() == name {
            return Ok(Some(a.normalized_value(XmlVersion::Implicit1_0)?.into_owned()));
        }
    }
    Ok(None)
}

/// JMdict's wording for a Wadoku grammar element, so `deinflect::pos_matches` and the search
/// tags see the same strings for both dictionaries.
fn parts_of_speech(start: &BytesStart) -> anyhow::Result<Vec<String>> {
    let name = start.name();
    let mut out = Vec::new();
    match name.as_ref() {
        "meishi" => {
            out.push("noun (common) (futsuumeishi)".to_string());
            if attr(start, "suru")?.is_some() {
                out.push("noun or participle which takes the aux. verb suru".to_string());
            }
        }
        "doushi" => {
            let level = attr(start, "level")?.unwrap_or_default();
            let row = attr(start, "godanrow")?.unwrap_or_default();
            out.push(match level.as_str() {
                "5" | "4" => {
                    let ending = match row.as_str() {
                        "ka" | "ka_i_yu" => "ku",
                        "ga" => "gu",
                        "sa" => "su",
                        "ta" => "tsu",
                        "na" => "nu",
                        "ba" => "bu",
                        "ma" => "mu",
                        "ra" | "ra_i" => "ru",
                        "wa" | "wa_o" => "u",
                        _ => "",
                    };
                    if ending.is_empty() {
                        "Godan verb".to_string()
                    } else {
                        format!("Godan verb with '{ending}' ending")
                    }
                }
                "1e" | "1i" | "2e" | "2i" => "Ichidan verb".to_string(),
                "suru" => "suru verb".to_string(),
                "kuru" => "Kuru verb - special class".to_string(),
                _ => "verb".to_string(),
            });
            match attr(start, "transitivity")?.as_deref() {
                Some("trans") => out.push("transitive verb".to_string()),
                Some("intrans") => out.push("intransitive verb".to_string()),
                _ => {}
            }
        }
        "keiyoushi" => out.push("adjective (keiyoushi)".to_string()),
        "keiyoudoushi" => out.push("adjectival noun or quasi-adjective (keiyodoshi)".to_string()),
        "fukushi" => out.push("adverb (fukushi)".to_string()),
        "rengo" => out.push("expressions (phrases, clauses, etc.)".to_string()),
        "kandoushi" => out.push("interjection (kandoushi)".to_string()),
        "daimeishi" => out.push("pronoun".to_string()),
        "rentaishi" => out.push("pre-noun adjectival (rentaishi)".to_string()),
        "setsuzokushi" => out.push("conjunction".to_string()),
        "jodoushi" => out.push("auxiliary verb".to_string()),
        "suffix" => out.push("suffix".to_string()),
        "prefix" => out.push("prefix".to_string()),
        "kanji" => out.push("kanji".to_string()),
        "wordcomponent" => out.push("word component".to_string()),
        "specialcharacter" => out.push("special character".to_string()),
        n if n.ends_with("joshi") => out.push("particle".to_string()),
        _ => {}
    }
    Ok(out)
}

/// Calls `f` for every `<entry>` as soon as it is complete; returns how many there were.
pub fn for_each_entry<R: BufRead>(
    input: R,
    mut f: impl FnMut(Entry) -> anyhow::Result<()>,
) -> anyhow::Result<usize> {
    let mut reader = Reader::from_reader(input);
    let mut buf = Vec::new();
    let mut count = 0;
    let mut entry: Option<Entry> = None;
    let mut sense: Option<Sense> = None;
    /// Where text goes while inside the element that collects it.
    #[derive(PartialEq)]
    enum Collect {
        Nothing,
        Orth,
        Hira,
        Accent,
        Usage(bool), // true: a domain, false: a hint
        Translation,
    }
    let mut collect = Collect::Nothing;
    let mut text = String::new();
    let mut skip_orth = false;
    let mut in_gram = false;
    let mut sense_depth = 0usize;

    loop {
        match reader.read_event_into(&mut buf)? {
            Event::Eof => break,
            Event::Start(start) => {
                let name = start.name();
                match name.as_ref() {
                    "entry" => {
                        let id = attr(&start, "id")?.unwrap_or_default();
                        entry = Some(Entry {
                            id: id.parse().with_context(|| format!("entry id {id:?}"))?,
                            ..Entry::default()
                        });
                    }
                    "orth" => {
                        skip_orth = attr(&start, "midashigo")?.is_some();
                        collect = Collect::Orth;
                        text.clear();
                    }
                    "hira" => {
                        collect = Collect::Hira;
                        text.clear();
                    }
                    "accent" => {
                        collect = Collect::Accent;
                        text.clear();
                    }
                    "gramGrp" => in_gram = true,
                    "sense" => {
                        sense_depth += 1;
                        if sense_depth == 1 {
                            sense = Some(Sense::default());
                        }
                    }
                    "usg" if sense.is_some() => {
                        collect = Collect::Usage(attr(&start, "type")?.as_deref() == Some("dom"));
                        text.clear();
                    }
                    "tr" if sense.is_some() => {
                        collect = Collect::Translation;
                        text.clear();
                    }
                    "expl" if collect == Collect::Translation => text.push_str(" ("),
                    _ => {}
                }
            }
            Event::Empty(start) => {
                if in_gram && let Some(e) = entry.as_mut() {
                    let pos = parts_of_speech(&start)?;
                    if !pos.is_empty() {
                        e.senses_pos_pending().extend(pos);
                    }
                }
            }
            Event::Text(t) => {
                if collect != Collect::Nothing {
                    let raw: &str = &t;
                    text.push_str(
                        &quick_xml::escape::unescape(raw).with_context(|| format!("unescaping {raw:?}"))?,
                    );
                }
            }
            Event::GeneralRef(r) => {
                if collect != Collect::Nothing {
                    match r.resolve_char_ref()? {
                        Some(c) => text.push(c),
                        None => text.push_str(quick_xml::escape::resolve_predefined_entity(&r).unwrap_or("")),
                    }
                }
            }
            Event::End(end) => {
                let name = end.name();
                match name.as_ref() {
                    "orth" => {
                        let value = text.trim().to_string();
                        if let Some(e) = entry.as_mut()
                            && !skip_orth
                            && !value.is_empty()
                            && !e.kanji.contains(&value)
                        {
                            e.kanji.push(value);
                        }
                        collect = Collect::Nothing;
                    }
                    "hira" => {
                        if let Some(e) = entry.as_mut() {
                            let value = text.trim().to_string();
                            if !value.is_empty() && !e.readings.contains(&value) {
                                e.readings.push(value);
                            }
                        }
                        collect = Collect::Nothing;
                    }
                    "accent" => {
                        if let Some(e) = entry.as_mut()
                            && let Ok(n) = text.trim().parse::<u8>()
                        {
                            e.pitch.push(n);
                        }
                        collect = Collect::Nothing;
                    }
                    "gramGrp" => in_gram = false,
                    "usg" => {
                        if let (Some(s), Collect::Usage(domain)) = (sense.as_mut(), &collect) {
                            let value = text.trim().to_string();
                            if !value.is_empty() {
                                if *domain {
                                    s.fields.push(value);
                                } else {
                                    s.misc.push(value);
                                }
                            }
                        }
                        collect = Collect::Nothing;
                    }
                    "expl" if collect == Collect::Translation => text.push(')'),
                    "tr" => {
                        if let Some(s) = sense.as_mut() {
                            let value = text.split_whitespace().collect::<Vec<_>>().join(" ");
                            if !value.is_empty() {
                                s.glosses.push(Gloss {
                                    lang: "ger".to_string(),
                                    text: value,
                                });
                            }
                        }
                        collect = Collect::Nothing;
                    }
                    "sense" => {
                        sense_depth = sense_depth.saturating_sub(1);
                        if sense_depth == 0
                            && let (Some(e), Some(s)) = (entry.as_mut(), sense.take())
                            && !s.glosses.is_empty()
                        {
                            e.senses.push(s);
                        }
                    }
                    "entry" => {
                        let Some(mut e) = entry.take() else {
                            bail!("</entry> without <entry>")
                        };
                        e.finish_wadoku();
                        f(e)?;
                        count += 1;
                    }
                    _ => {}
                }
            }
            _ => {}
        }
        buf.clear();
    }
    Ok(count)
}

/// The Wadoku downloads page lists dated dumps; the newest one is the URL to fetch.
pub fn latest_url(page: &str) -> Option<String> {
    let mut best: Option<&str> = None;
    let mut rest = page;
    while let Some(i) = rest.find("wadoku-xml-") {
        let candidate = &rest[i..];
        let end = candidate.find(".tar.xz").map(|e| e + ".tar.xz".len());
        if let Some(end) = end
            && candidate[11..end - 7].chars().all(|c| c.is_ascii_digit())
            && best.is_none_or(|b| candidate[..end] > *b)
        {
            best = Some(&candidate[..end]);
        }
        rest = &rest[i + 11..];
    }
    best.map(|name| format!("https://www.wadoku.de/downloads/xml-export/{name}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = include_str!("../../tests/fixtures/wadoku-sample.xml");

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
    fn entries_forms_accents_and_senses() {
        let entries = parse_sample();
        let ids: Vec<i64> = entries.iter().map(|e| e.id).collect();
        assert_eq!(ids, [15, 52, 1707, 8546, 273]);
        assert_eq!(created(SAMPLE.as_bytes()).unwrap().as_deref(), Some("2026-07-05"));

        let insulin = &entries[0];
        assert_eq!(insulin.headword(), "インスリン");
        assert_eq!(insulin.readings, ["いんすりん"]);
        assert_eq!(insulin.pitch, [0]);
        assert_eq!(insulin.senses[0].fields, ["Physiol.", "Med."]);
        assert_eq!(insulin.senses[0].gloss_text("ger"), "Insulin");
        assert_eq!(insulin.senses[0].pos, ["noun (common) (futsuumeishi)"]);

        let hitokai = &entries[1];
        assert_eq!(hitokai.kanji, ["人買い", "人買"]); // the midashigo form 人買(い) is skipped
        assert_eq!(hitokai.pitch, [0, 3]);
        assert_eq!(hitokai.senses.len(), 2);

        let hikikomoru = &entries[2];
        assert_eq!(hikikomoru.kanji.len(), 9);
        assert_eq!(hikikomoru.readings, ["ひきこもる"]);
        assert_eq!(hikikomoru.pitch, [4]);
        assert_eq!(
            hikikomoru.senses[0].pos,
            ["Godan verb with 'ru' ending", "intransitive verb"]
        );
        assert_eq!(
            hikikomoru.senses[0].gloss_text("ger"),
            "sich zurückziehen (ins Haus, Zimmer); (das Haus, Zimmer) nicht verlassen; sich einigeln"
        );
        assert_eq!(entries[3].senses[0].pos, ["Ichidan verb", "transitive verb"]);
        assert!(entries[3].pitch.is_empty());
        let boushoku = &entries[4];
        assert_eq!(
            boushoku.senses[0].pos,
            [
                "noun (common) (futsuumeishi)",
                "noun or participle which takes the aux. verb suru"
            ]
        );
        assert_eq!(boushoku.senses[0].gloss_text("ger"), "unmäßiges Essen; Völlerei");
    }

    #[test]
    fn newest_dump_on_the_downloads_page() {
        let page = "…/downloads/xml-export/wadoku-xml-20250105.tar.xz … wadoku-xml-20260705.tar.xz … wadoku-xml-20251201.tar.xz";
        assert_eq!(
            latest_url(page).as_deref(),
            Some("https://www.wadoku.de/downloads/xml-export/wadoku-xml-20260705.tar.xz")
        );
        assert_eq!(latest_url("nothing here"), None);
    }

    #[test]
    fn plain_xml_is_not_unpacked() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/wadoku-sample.xml");
        assert_eq!(unpack(&path).unwrap(), path);
    }
}
