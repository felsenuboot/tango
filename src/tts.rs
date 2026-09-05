//! Reading things aloud through speech-dispatcher's `spd-say`, when it is installed and has a
//! Japanese voice (Felix runs Open JTalk and VOICEVOX behind it). Nothing is bundled: without a
//! voice the speaker buttons stay away.
//!
//! The voice list is probed once, on a thread started at startup (#101): `spd-say -L` can take
//! a few hundred milliseconds when the daemon has to start, too long for the UI thread.

use std::process::{Command, Stdio};
use std::sync::OnceLock;

static AVAILABLE: OnceLock<bool> = OnceLock::new();

/// Starts the probe on a thread; `available` answers `false` until it is done.
pub fn probe() {
    if AVAILABLE.get().is_some() {
        return;
    }
    let spawned = std::thread::Builder::new()
        .name("tango-tts-probe".into())
        .spawn(|| {
            let _ = AVAILABLE.set(detect());
        });
    if let Err(e) = spawned {
        log::warn!("tts: cannot probe for a voice: {e}");
    }
}

/// Whether `spd-say` exists and lists a Japanese voice, as far as the probe has found out.
pub fn available() -> bool {
    AVAILABLE.get().copied().unwrap_or(false)
}

fn detect() -> bool {
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
    match result {
        // A child nobody waits for stays a zombie until the parent exits; a thread reaps it.
        Ok(mut child) => {
            let _ = std::thread::Builder::new()
                .name("tango-tts-reap".into())
                .spawn(move || {
                    let _ = child.wait();
                });
        }
        Err(e) => log::warn!("tts: cannot run spd-say: {e}"),
    }
}
