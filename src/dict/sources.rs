//! The dictionary sources Tango knows: where each comes from, and a download helper with progress.
//!
//! One `Source` per data set. The Dictionaries page lists them all, installed or not; the import
//! jobs, the config file and the database refer to them by `id`. Adding a data set means a
//! `Source` here, a reader for its format under `dict/`, and a match arm in `store::import`.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use anyhow::Context;

#[derive(Debug)]
pub struct Source {
    /// Stable key, used in the database and the config file.
    pub id: &'static str,
    pub name: &'static str,
    /// One line for the Dictionaries page.
    pub description: &'static str,
    pub url: &'static str,
    /// File name in the cache directory.
    pub filename: &'static str,
    /// Further `(url, file name)` pairs a source made of several files needs next to `filename`.
    pub extra_files: &'static [(&'static str, &'static str)],
    pub licence: &'static str,
    pub licence_url: &'static str,
    /// Rough download size, for the status texts.
    pub size_mb: u32,
    /// For sources whose download URL changes (dated files): finds the current one from the
    /// page at `url` when given the page's text.
    pub latest: Option<fn(&str) -> Option<String>>,
}

/// The full JMdict, all gloss languages (English, German, Dutch, French, Russian, ...), gzip'd.
/// The `ftp.edrdg.org` host has a broken TLS certificate; this one works.
pub const JMDICT: Source = Source {
    id: "jmdict",
    name: "JMdict",
    description: "Japanese–English, with German, Dutch, French and other glosses, by the EDRDG.",
    url: "https://www.edrdg.org/pub/Nihongo/JMdict.gz",
    filename: "JMdict.gz",
    extra_files: &[],
    licence: "Creative Commons Attribution-ShareAlike 4.0 (EDRDG licence)",
    licence_url: "https://www.edrdg.org/edrdg/licence.html",
    size_mb: 22,
    latest: None,
};

/// Wadoku, Japanese–German with pitch accent. The dump is dated and published twice a year, so
/// the URL is the downloads page and `latest` picks the newest file from it.
pub const WADOKU: Source = Source {
    id: "wadoku",
    name: "Wadoku",
    description: "Japanese–German with pitch accent, by wadoku.de.",
    url: "https://www.wadoku.de/wiki/display/WAD/Downloads+und+Links",
    filename: "wadoku-xml.tar.xz",
    extra_files: &[],
    licence: "Wadoku dictionary licence (free software with attribution)",
    licence_url: "https://www.wadoku.de/wiki/display/WAD/W%C3%B6rterbuch+Lizenz",
    size_mb: 25,
    latest: Some(crate::dict::wadoku::latest_url),
};

/// JMnedict: Japanese proper names (people, places, companies, products), the EDRDG's names
/// file in the JMdict XML shape. Searched only for exact matches or with `#names`.
pub const JMNEDICT: Source = Source {
    id: "jmnedict",
    name: "JMnedict",
    description: "Names: people, places, companies and products, by the EDRDG.",
    url: "https://www.edrdg.org/pub/Nihongo/JMnedict.xml.gz",
    filename: "JMnedict.xml.gz",
    extra_files: &[],
    licence: "Creative Commons Attribution-ShareAlike 4.0 (EDRDG licence)",
    licence_url: "https://www.edrdg.org/edrdg/licence.html",
    size_mb: 13,
    latest: None,
};

/// JLPT vocabulary levels, one CSV per level. Unofficial lists (Jonathan Waller's, with JMdict
/// numbers added by Stephen Kraus); an opt-in download, never fetched by itself.
pub const JLPT: Source = Source {
    id: "jlpt",
    name: "JLPT",
    description: "N5–N1 per word: Jonathan Waller's unofficial lists with JMdict numbers, by Stephen Kraus. \
                  There are no official lists.",
    url: "https://raw.githubusercontent.com/stephenmk/yomitan-jlpt-vocab/main/original_data/n5.csv",
    filename: "jlpt-n5.csv",
    extra_files: &[
        (
            "https://raw.githubusercontent.com/stephenmk/yomitan-jlpt-vocab/main/original_data/n4.csv",
            "jlpt-n4.csv",
        ),
        (
            "https://raw.githubusercontent.com/stephenmk/yomitan-jlpt-vocab/main/original_data/n3.csv",
            "jlpt-n3.csv",
        ),
        (
            "https://raw.githubusercontent.com/stephenmk/yomitan-jlpt-vocab/main/original_data/n2.csv",
            "jlpt-n2.csv",
        ),
        (
            "https://raw.githubusercontent.com/stephenmk/yomitan-jlpt-vocab/main/original_data/n1.csv",
            "jlpt-n1.csv",
        ),
    ],
    licence: "Creative Commons Attribution-ShareAlike 4.0 (lists CC BY, Jonathan Waller)",
    licence_url: "https://github.com/stephenmk/yomitan-jlpt-vocab",
    size_mb: 1,
    latest: None,
};

/// KANJIDIC2: readings, meanings, stroke count, grade, JLPT and frequency per kanji.
pub const KANJIDIC: Source = Source {
    id: "kanjidic",
    name: "KANJIDIC2",
    description: "Kanji: readings, meanings, stroke count, grade, JLPT level and frequency, by the EDRDG.",
    url: "https://www.edrdg.org/kanjidic/kanjidic2.xml.gz",
    filename: "kanjidic2.xml.gz",
    extra_files: &[],
    licence: "Creative Commons Attribution-ShareAlike 4.0 (EDRDG licence)",
    licence_url: "https://www.edrdg.org/edrdg/licence.html",
    size_mb: 2,
    latest: None,
};

/// KanjiVG: stroke order as SVG paths. Released as dated files on GitHub; `latest` reads the
/// releases API for the newest single-file XML.
pub const KANJIVG: Source = Source {
    id: "kanjivg",
    name: "KanjiVG",
    description: "Stroke order diagrams for 6,700 kanji, by Ulrich Apel.",
    url: "https://api.github.com/repos/KanjiVG/kanjivg/releases/latest",
    filename: "kanjivg.xml.gz",
    extra_files: &[],
    licence: "Creative Commons Attribution-ShareAlike 3.0",
    licence_url: "https://kanjivg.tagaini.net/",
    size_mb: 4,
    latest: Some(crate::dict::kanjivg::latest_url),
};

/// RADKFILE: which radicals each kanji contains, for search by radicals.
pub const RADKFILE: Source = Source {
    id: "radkfile",
    name: "Radicals (RADKFILE)",
    description: "The radical index for looking kanji up by their parts, by the EDRDG.",
    url: "https://www.edrdg.org/pub/Nihongo/kradzip.zip",
    filename: "kradzip.zip",
    extra_files: &[],
    licence: "EDRDG licence (Creative Commons Attribution-ShareAlike 4.0)",
    licence_url: "https://www.edrdg.org/edrdg/licence.html",
    size_mb: 1,
    latest: None,
};

/// Tatoeba example sentences: the Japanese ones, their English and German translations, the
/// links between them, and the Tanaka corpus index that ties sentences to JMdict words. The
/// exports are rebuilt weekly under fixed names, so the download date is the version.
pub const TATOEBA: Source = Source {
    id: "tatoeba",
    name: "Tatoeba",
    description: "Example sentences with English and German translations, by the Tatoeba project.",
    url: "https://downloads.tatoeba.org/exports/per_language/jpn/jpn_sentences.tsv.bz2",
    filename: "jpn_sentences.tsv.bz2",
    extra_files: &[
        (
            "https://downloads.tatoeba.org/exports/jpn_indices.tar.bz2",
            "jpn_indices.tar.bz2",
        ),
        (
            "https://downloads.tatoeba.org/exports/per_language/jpn/jpn-eng_links.tsv.bz2",
            "jpn-eng_links.tsv.bz2",
        ),
        (
            "https://downloads.tatoeba.org/exports/per_language/eng/eng_sentences.tsv.bz2",
            "eng_sentences.tsv.bz2",
        ),
        (
            "https://downloads.tatoeba.org/exports/per_language/jpn/jpn-deu_links.tsv.bz2",
            "jpn-deu_links.tsv.bz2",
        ),
        (
            "https://downloads.tatoeba.org/exports/per_language/deu/deu_sentences.tsv.bz2",
            "deu_sentences.tsv.bz2",
        ),
    ],
    licence: "Creative Commons Attribution 2.0 France",
    licence_url: "https://tatoeba.org/en/terms_of_use",
    size_mb: 45,
    latest: None,
};

/// Every source, in the default search order. The kanji sources hold no entries; they are
/// listed so the Dictionaries page manages them like the others.
pub const SOURCES: &[&Source] = &[
    &JMDICT, &WADOKU, &JMNEDICT, &JLPT, &KANJIDIC, &KANJIVG, &RADKFILE, &TATOEBA,
];

pub fn by_id(id: &str) -> Option<&'static Source> {
    SOURCES.iter().find(|s| s.id == id).copied()
}

/// `(bytes so far, total bytes if the server said)`
pub type Progress<'a> = &'a mut dyn FnMut(u64, Option<u64>);

pub const USER_AGENT: &str = concat!(
    "tango/",
    env!("CARGO_PKG_VERSION"),
    " (+https://github.com/felsenuboot/tango)"
);

/// The URL to download `source` from now: its `url`, or the newest file its page lists.
pub fn download_url(source: &Source) -> anyhow::Result<String> {
    let Some(latest) = source.latest else {
        return Ok(source.url.to_string());
    };
    let page = ureq::get(source.url)
        .header("User-Agent", USER_AGENT)
        .call()
        .with_context(|| format!("reading {}", source.url))?
        .body_mut()
        .read_to_string()
        .with_context(|| format!("reading {}", source.url))?;
    latest(&page).with_context(|| format!("no download found on {}", source.url))
}

/// Fetches `url` into `dest`, via a `.part` file so a partial download is never mistaken for a whole one.
pub fn download(url: &str, dest: &Path, progress: Progress) -> anyhow::Result<PathBuf> {
    let part = dest.with_extension("part");
    let mut response = ureq::get(url)
        .header("User-Agent", USER_AGENT)
        .call()
        .with_context(|| format!("downloading {url}"))?;
    let total = response
        .headers()
        .get("content-length")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse::<u64>().ok());
    let mut reader = response.body_mut().with_config().limit(u64::MAX).reader();
    let mut out = std::fs::File::create(&part).with_context(|| format!("creating {}", part.display()))?;
    let mut buf = vec![0u8; 256 * 1024];
    let mut done = 0u64;
    loop {
        let n = reader.read(&mut buf)?;
        if n == 0 {
            break;
        }
        out.write_all(&buf[..n])?;
        done += n as u64;
        progress(done, total);
    }
    out.flush()?;
    drop(out);
    std::fs::rename(&part, dest)?;
    Ok(dest.to_path_buf())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_unique_and_findable() {
        for (i, s) in SOURCES.iter().enumerate() {
            assert!(by_id(s.id).is_some());
            assert!(
                !SOURCES[..i].iter().any(|o| o.id == s.id),
                "duplicate id {}",
                s.id
            );
        }
        assert!(by_id("nope").is_none());
    }

    #[test]
    fn jlpt_files_match_the_reader() {
        let names: Vec<&str> = std::iter::once(JLPT.filename)
            .chain(JLPT.extra_files.iter().map(|(_, name)| *name))
            .collect();
        let expected: Vec<&str> = crate::dict::jlpt::FILES.iter().map(|(n, _)| *n).collect();
        assert_eq!(names, expected);
    }

    #[test]
    fn tatoeba_files_match_the_reader() {
        let names: Vec<&str> = std::iter::once(TATOEBA.filename)
            .chain(TATOEBA.extra_files.iter().map(|(_, name)| *name))
            .collect();
        assert_eq!(names, crate::dict::tatoeba::FILES);
    }
}
