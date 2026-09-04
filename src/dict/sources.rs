//! Where the dictionary files come from, and a download helper with progress.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use anyhow::Context;

/// The full JMdict, all gloss languages (English, German, Dutch, French, Russian, ...), gzip'd.
/// Licence: EDRDG Creative Commons Attribution-ShareAlike 4.0, <https://www.edrdg.org/edrdg/licence.html>
pub const JMDICT_URL: &str = "https://www.edrdg.org/pub/Nihongo/JMdict.gz";
pub const JMDICT_FILENAME: &str = "JMdict.gz";

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
