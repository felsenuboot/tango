//! Tatoeba example sentences (<https://tatoeba.org>, CC BY 2.0 FR) and the Tanaka corpus index
//! that ties the Japanese ones to JMdict words.
//!
//! The exports are tab-separated, bzip2'd, one file per kind:
//! - `jpn_sentences.tsv`: `id, lang, text` for every Japanese sentence.
//! - `eng_sentences.tsv`, `deu_sentences.tsv`: the same for the translations.
//! - `jpn-eng_links.tsv`, `jpn-deu_links.tsv`: `jpn id, other id` pairs that translate each other.
//! - `jpn_indices.csv` (in `jpn_indices.tar.bz2`): `jpn id, eng id, words`, where the words are
//!   the sentence's JMdict headwords annotated Tanaka-corpus style, see [`parse_index`].

use std::io::{BufRead, BufReader, Read};
use std::path::Path;

use anyhow::Context;

use crate::model::SentenceWord;

/// The files of one export, by the names Tatoeba gives them.
pub const JPN_SENTENCES: &str = "jpn_sentences.tsv.bz2";
pub const JPN_INDICES: &str = "jpn_indices.tar.bz2";
pub const JPN_ENG_LINKS: &str = "jpn-eng_links.tsv.bz2";
pub const ENG_SENTENCES: &str = "eng_sentences.tsv.bz2";
pub const JPN_DEU_LINKS: &str = "jpn-deu_links.tsv.bz2";
pub const DEU_SENTENCES: &str = "deu_sentences.tsv.bz2";
/// All of them, the source's main file first; the registry test checks `sources::TATOEBA` against it.
#[cfg_attr(not(test), allow(dead_code))]
pub const FILES: &[&str] = &[
    JPN_SENTENCES,
    JPN_INDICES,
    JPN_ENG_LINKS,
    ENG_SENTENCES,
    JPN_DEU_LINKS,
    DEU_SENTENCES,
];

/// Tatoeba's language codes (ISO 639-3) for JMdict's gloss language codes (ISO 639-2/B).
pub fn language(jmdict: &str) -> &str {
    match jmdict {
        "ger" => "deu",
        "fre" => "fra",
        "dut" => "nld",
        other => other,
    }
}

/// The preferred gloss languages as Tatoeba codes; English when the list is empty. A sentence
/// with none of them still shows its first translation (see `Database::attach_translations`).
pub fn languages(preferred: &[String]) -> Vec<String> {
    if preferred.is_empty() {
        return vec!["eng".into()];
    }
    preferred.iter().map(|l| language(l).to_string()).collect()
}

/// Opens one export file for line reading: bzip2'd or plain, a tar with one member or bare.
pub fn open(path: &Path) -> anyhow::Result<Box<dyn BufRead>> {
    let file = std::fs::File::open(path).with_context(|| format!("opening {}", path.display()))?;
    let name = path.file_name().map(|n| n.to_string_lossy()).unwrap_or_default();
    let raw: Box<dyn Read> = if name.ends_with(".bz2") {
        Box::new(bzip2::read::MultiBzDecoder::new(file))
    } else {
        Box::new(file)
    };
    if name.contains(".tar") {
        // The indices come as a tar with a single csv inside. `tar` streams, so the member is read
        // into memory once rather than seeking around a compressed archive.
        let mut archive = tar::Archive::new(raw);
        for entry in archive.entries()? {
            let mut entry = entry?;
            if entry.header().entry_type().is_file() {
                let mut bytes = Vec::new();
                entry.read_to_end(&mut bytes)?;
                return Ok(Box::new(std::io::Cursor::new(bytes)));
            }
        }
        anyhow::bail!("no file inside {}", path.display());
    }
    Ok(Box::new(BufReader::with_capacity(1 << 16, raw)))
}

/// `id \t lang \t text` per line; calls `f(id, text)` for each. Returns the line count.
pub fn for_each_sentence(
    reader: impl BufRead,
    mut f: impl FnMut(i64, &str) -> anyhow::Result<()>,
) -> anyhow::Result<usize> {
    let mut n = 0;
    for line in reader.lines() {
        let line = line?;
        let mut cols = line.splitn(3, '\t');
        let (Some(id), Some(_lang), Some(text)) = (cols.next(), cols.next(), cols.next()) else {
            continue;
        };
        let Ok(id) = id.parse::<i64>() else { continue };
        f(id, text.trim())?;
        n += 1;
    }
    Ok(n)
}

/// `a \t b` per line, `f(a, b)` for each.
pub fn for_each_link(
    reader: impl BufRead,
    mut f: impl FnMut(i64, i64) -> anyhow::Result<()>,
) -> anyhow::Result<usize> {
    let mut n = 0;
    for line in reader.lines() {
        let line = line?;
        let Some((a, b)) = line.split_once('\t') else {
            continue;
        };
        let (Ok(a), Ok(b)) = (a.trim().parse::<i64>(), b.trim().parse::<i64>()) else {
            continue;
        };
        f(a, b)?;
        n += 1;
    }
    Ok(n)
}

/// `jpn id \t eng id \t words` per line, `f(jpn, eng, words)` for each.
pub fn for_each_index(
    reader: impl BufRead,
    mut f: impl FnMut(i64, i64, Vec<SentenceWord>) -> anyhow::Result<()>,
) -> anyhow::Result<usize> {
    let mut n = 0;
    for line in reader.lines() {
        let line = line?;
        let mut cols = line.splitn(3, '\t');
        let (Some(jpn), Some(eng), Some(words)) = (cols.next(), cols.next(), cols.next()) else {
            continue;
        };
        let (Ok(jpn), Ok(eng)) = (jpn.parse::<i64>(), eng.parse::<i64>()) else {
            continue;
        };
        f(jpn, eng, parse_index(words))?;
        n += 1;
    }
    Ok(n)
}

/// The words of one indexed sentence. Each is `headword` with optional decorations in this
/// order: `(reading)` or `(#seq)`, `[sense]`, `|1` (a priority mark, ignored), `{surface}`
/// and a trailing `~` for a good example.
pub fn parse_index(text: &str) -> Vec<SentenceWord> {
    text.split_whitespace().filter_map(parse_word).collect()
}

fn parse_word(token: &str) -> Option<SentenceWord> {
    let mut word = SentenceWord::default();
    let mut rest = token;
    if let Some(stripped) = rest.strip_suffix('~') {
        word.good = true;
        rest = stripped;
    }
    // Decorations from the back, so a headword containing brackets is left alone.
    if let Some((head, tail)) = rest.split_once('{') {
        word.surface = Some(tail.trim_end_matches('}').to_string()).filter(|s| !s.is_empty());
        rest = head;
    }
    if let Some((head, _)) = rest.split_once('|') {
        rest = head;
    }
    if let Some((head, tail)) = rest.split_once('[') {
        word.sense = tail.trim_end_matches(']').parse().ok();
        rest = head;
    }
    if let Some((head, tail)) = rest.split_once('(') {
        let inner = tail.trim_end_matches(')');
        match inner.strip_prefix('#') {
            Some(seq) => word.seq = seq.parse().ok(),
            None if !inner.is_empty() => word.reading = Some(inner.to_string()),
            None => {}
        }
        rest = head;
    }
    if rest.is_empty() {
        return None;
    }
    word.headword = rest.to_string();
    Some(word)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn index_words_are_taken_apart() {
        let words = parse_index(
            "は 二十歳(はたち){２０歳} になる[01]{になりました} 猫(ねこ)[01] が(#2028930) 縞模様~ 為る(する){した}",
        );
        assert_eq!(words.len(), 7);
        assert_eq!(
            words[0],
            SentenceWord {
                headword: "は".into(),
                ..Default::default()
            }
        );
        assert_eq!(
            words[1],
            SentenceWord {
                headword: "二十歳".into(),
                reading: Some("はたち".into()),
                surface: Some("２０歳".into()),
                ..Default::default()
            }
        );
        assert_eq!(words[2].sense, Some(1));
        assert_eq!(words[2].surface.as_deref(), Some("になりました"));
        assert_eq!(words[3].reading.as_deref(), Some("ねこ"));
        assert_eq!(words[4].seq, Some(2028930));
        assert!(words[4].reading.is_none());
        assert!(words[5].good);
        assert_eq!(words[5].headword, "縞模様");
        assert_eq!(words[6].reading.as_deref(), Some("する"));
    }

    #[test]
    fn priority_marks_and_junk_are_ignored() {
        let words = parse_index("は|1 ~ 走る|2{走った}~");
        assert_eq!(words.len(), 2);
        assert_eq!(words[0].headword, "は");
        assert_eq!(words[1].headword, "走る");
        assert_eq!(words[1].surface.as_deref(), Some("走った"));
        assert!(words[1].good);
    }

    #[test]
    fn language_codes_are_mapped() {
        assert_eq!(languages(&["ger".to_string()]), ["deu"]);
        assert_eq!(languages(&[]), ["eng"]);
        assert_eq!(languages(&["eng".to_string(), "fre".to_string()]), ["eng", "fra"]);
    }

    #[test]
    fn tsv_lines_are_read() {
        let sentences = "1\tjpn\t猫が好きです。\nx\tjpn\tbroken\n2\tjpn\t犬 \n";
        let mut got = Vec::new();
        let n = for_each_sentence(sentences.as_bytes(), |id, text| {
            got.push((id, text.to_string()));
            Ok(())
        })
        .unwrap();
        assert_eq!(n, 2);
        assert_eq!(got, [(1, "猫が好きです。".to_string()), (2, "犬".to_string())]);
        let mut links = Vec::new();
        for_each_link("1\t10\n\n2\t20\n".as_bytes(), |a, b| {
            links.push((a, b));
            Ok(())
        })
        .unwrap();
        assert_eq!(links, [(1, 10), (2, 20)]);
        let mut indexed = Vec::new();
        for_each_index("1\t10\t猫(ねこ) が\n".as_bytes(), |j, e, w| {
            indexed.push((j, e, w.len()));
            Ok(())
        })
        .unwrap();
        assert_eq!(indexed, [(1, 10, 2)]);
    }
}
