//! Reading things aloud through speech-dispatcher's `spd-say`, when it is installed and has a
//! Japanese voice (Felix runs Open JTalk and VOICEVOX behind it). Nothing is bundled: without a
//! voice the speaker buttons stay away.

use std::process::{Command, Stdio};
use std::sync::OnceLock;

static AVAILABLE: OnceLock<bool> = OnceLock::new();

/// Whether `spd-say` exists and lists a Japanese voice; checked once.
pub fn available() -> bool {
    *AVAILABLE.get_or_init(|| {
        if std::env::var_os("TANGO_NO_TTS").is_some() {
            return false;
        }
        let output = Command::new("spd-say").arg("-L").stderr(Stdio::null()).output();
        let Ok(output) = output else { return false };
        let list = String::from_utf8_lossy(&output.stdout);
        let japanese = list.lines().any(|line| {
            let mut fields = line.split_whitespace();
            fields.next().is_some() && fields.next().is_some_and(|lang| lang.starts_with("ja"))
        });
        log::info!(
            "tts: spd-say {}",
            if japanese {
                "has a Japanese voice"
            } else {
                "has no Japanese voice"
            }
        );
        japanese
    })
}

/// Speaks `text` in Japanese; returns at once, the daemon does the rest.
pub fn speak(text: &str) {
    if text.trim().is_empty() {
        return;
    }
    let result = Command::new("spd-say")
        .args(["-l", "ja", "--"])
        .arg(text)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn();
    if let Err(e) = result {
        log::warn!("tts: cannot run spd-say: {e}");
    }
}
