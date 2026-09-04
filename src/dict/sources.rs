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
    pub licence: &'static str,
    pub licence_url: &'static str,
    /// Rough download size, for the status texts.
    pub size_mb: u32,
}

/// The full JMdict, all gloss languages (English, German, Dutch, French, Russian, ...), gzip'd.
/// The `ftp.edrdg.org` host has a broken TLS certificate; this one works.
pub const JMDICT: Source = Source {
    id: "jmdict",
    name: "JMdict",
    description: "Japanese–English, with German, Dutch, French and other glosses, by the EDRDG.",
    url: "https://www.edrdg.org/pub/Nihongo/JMdict.gz",
    filename: "JMdict.gz",
    licence: "Creative Commons Attribution-ShareAlike 4.0 (EDRDG licence)",
    licence_url: "https://www.edrdg.org/edrdg/licence.html",
    size_mb: 22,
};

/// Every source, in the default search order.
pub const SOURCES: &[&Source] = &[&JMDICT];

pub fn by_id(id: &str) -> Option<&'static Source> {
    SOURCES.iter().find(|s| s.id == id).copied()
}

/// `(bytes so far, total bytes if the server said)`
pub type Progress<'a> = &'a mut dyn FnMut(u64, Option<u64>);

/// Fetches `url` into `dest`, via a `.part` file so a partial download is never mistaken for a whole one.
pub fn download(url: &str, dest: &Path, progress: Progress) -> anyhow::Result<PathBuf> {
    let part = dest.with_extension("part");
    let mut response = ureq::get(url)
        .header(
            "User-Agent",
            concat!(
                "tango/",
                env!("CARGO_PKG_VERSION"),
                " (+https://github.com/felsenuboot/tango)"
            ),
        )
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
}
