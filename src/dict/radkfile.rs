//! RADKFILE, the radical → kanji index behind multi-radical lookup (EDRDG, in `kradzip.zip`,
//! EUC-JP). Sections start with `$ <radical> <strokes>` and list the kanji that contain the
//! radical, run together, on the following lines.

use std::io::Read;
use std::path::Path;

use anyhow::Context;

use crate::model::Radical;

/// Reads `radkfile` out of `kradzip.zip` (or a plain file) and decodes it to UTF-8.
pub fn read(path: &Path) -> anyhow::Result<String> {
    let bytes = if path.extension().is_some_and(|e| e == "zip") {
        let file = std::fs::File::open(path).with_context(|| format!("opening {}", path.display()))?;
        let mut archive = zip::ZipArchive::new(file)?;
        let mut entry = archive
            .by_name("radkfile")
            .with_context(|| format!("no radkfile in {}", path.display()))?;
        let mut bytes = Vec::new();
        entry.read_to_end(&mut bytes)?;
        bytes
    } else {
        std::fs::read(path).with_context(|| format!("reading {}", path.display()))?
    };
    Ok(decode(&bytes))
}

/// EUC-JP unless the bytes are already valid UTF-8.
pub fn decode(bytes: &[u8]) -> String {
    match std::str::from_utf8(bytes) {
        Ok(text) => text.to_string(),
        Err(_) => encoding_rs::EUC_JP.decode(bytes).0.into_owned(),
    }
}

pub fn parse(text: &str) -> Vec<Radical> {
    let mut out: Vec<Radical> = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some(rest) = line.strip_prefix('$') {
            let mut parts = rest.split_whitespace();
            let radical = parts.next().unwrap_or("").to_string();
            let strokes = parts.next().and_then(|s| s.parse().ok()).unwrap_or(0);
            out.push(Radical {
                radical,
                strokes,
                kanji: Vec::new(),
            });
        } else if let Some(current) = out.last_mut() {
            current.kanji.extend(line.chars().filter(|c| !c.is_whitespace()));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = include_str!("../../tests/fixtures/radkfile-sample.txt");

    #[test]
    fn sections_and_kanji() {
        let radicals = parse(SAMPLE);
        assert_eq!(radicals[0].radical, "一");
        assert_eq!(radicals[0].strokes, 1);
        assert!(radicals[0].kanji.contains(&'一'));
        assert!(radicals[0].kanji.len() > 100);
        let two = parse("$ 一 1\n亜唖\n$ ｜ 1\n中\n");
        assert_eq!(two.len(), 2);
        assert_eq!(two[1].kanji, ['中']);
    }

    #[test]
    fn euc_jp_is_decoded() {
        let (bytes, _, _) = encoding_rs::EUC_JP.encode("$ 一 1\n亜\n");
        assert_eq!(parse(&decode(&bytes))[0].kanji, ['亜']);
        assert_eq!(decode("plain".as_bytes()), "plain");
    }
}
