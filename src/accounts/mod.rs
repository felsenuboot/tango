//! Accounts on learning sites and what the user has learned there. Each provider syncs into
//! the same `learned` table of the user database, so the entry view, the kanji page and the
//! `#known` filters work the same whichever site the items come from.
//!
//! Providers: WaniKani (#11). MaruMori (#12) waits for its API details.

pub mod wanikani;

/// What an item is on the provider's side.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Kind {
    Kanji,
    Vocabulary,
}

impl Kind {
    pub fn as_str(self) -> &'static str {
        match self {
            Kind::Kanji => "kanji",
            Kind::Vocabulary => "vocabulary",
        }
    }

    pub fn parse(text: &str) -> Option<Self> {
        match text {
            "kanji" => Some(Kind::Kanji),
            "vocabulary" => Some(Kind::Vocabulary),
            _ => None,
        }
    }
}

/// One learned item: the text as the user sees it on the site (a kanji, or a word in its usual
/// written form), the site's level, and the SRS stage on WaniKani's 0–9 scale.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Learned {
    pub provider: String,
    pub kind: Kind,
    pub text: String,
    /// The provider's primary reading, "ふじさん" for ふじ山; empty for kanji and before the
    /// sync that fetched it (#98).
    pub reading: String,
    pub level: u32,
    pub stage: u8,
}

/// A CJK ideograph (or the iteration mark 々).
pub fn is_kanji(c: char) -> bool {
    matches!(c, '\u{4e00}'..='\u{9fff}' | '\u{3400}'..='\u{4dbf}' | '\u{f900}'..='\u{faff}' | '々')
}

/// Whether a provider's spelling of a word fits a dictionary form: every kanji in it appears in
/// the form. ふじ山 fits 富士山 (and 不二山), not 藤さん; used when the reading matched but the
/// characters did not (#98).
pub fn spelled_like(text: &str, form: &str) -> bool {
    text.chars().filter(|&c| is_kanji(c)).all(|c| form.contains(c))
}

/// WaniKani's stage groups; other providers map onto them.
pub fn stage_name(stage: u8) -> &'static str {
    match stage {
        0 => "Locked",
        1..=4 => "Apprentice",
        5 | 6 => "Guru",
        7 => "Master",
        8 => "Enlightened",
        _ => "Burned",
    }
}

/// The CSS class for a stage, coloured after WaniKani's own palette (`style.css`).
pub fn stage_class(stage: u8) -> &'static str {
    match stage {
        0 => "tango-wk-locked",
        1..=4 => "tango-wk-apprentice",
        5 | 6 => "tango-wk-guru",
        7 => "tango-wk-master",
        8 => "tango-wk-enlightened",
        _ => "tango-wk-burned",
    }
}

/// The CSS class for an item kind: WaniKani's pink for kanji, purple for vocabulary.
pub fn kind_class(kind: Kind) -> &'static str {
    match kind {
        Kind::Kanji => "tango-wk-kanji",
        Kind::Vocabulary => "tango-wk-vocabulary",
    }
}

/// "Known" means passed: Guru or beyond, which is when WaniKani unlocks what builds on it.
pub fn is_known(stage: u8) -> bool {
    stage >= 5
}

/// A connected account as the Accounts page shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Status {
    pub username: String,
    pub level: u32,
    /// ISO 8601, or none before the first sync.
    pub last_sync: Option<String>,
    pub items: usize,
}

pub fn provider_name(id: &str) -> &str {
    match id {
        "wanikani" => "WaniKani",
        "marumori" => "MaruMori",
        other => other,
    }
}
