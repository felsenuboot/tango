//! Dictionary entries as plain data, independent of the source format and the storage.

/// JMdict/EDICT priority markers that mark a headword as common (see the JMdict DTD).
pub const COMMON_PRIORITIES: &[&str] = &["ichi1", "news1", "spec1", "spec2", "gai1"];

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Gloss {
    /// ISO 639-2 code as JMdict uses it: "eng", "ger", "dut", "fre", ...
    pub lang: String,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Sense {
    /// Parts of speech, e.g. "noun (common) (futsuumeishi)".
    pub pos: Vec<String>,
    /// "word usually written using kana alone", ...
    pub misc: Vec<String>,
    /// "computing", "medicine", ...
    pub fields: Vec<String>,
    /// In document order, so the languages come out in the order the source lists them.
    pub glosses: Vec<Gloss>,
}

impl Sense {
    pub fn gloss_text(&self, lang: &str) -> String {
        self.glosses
            .iter()
            .filter(|g| g.lang == lang)
            .map(|g| g.text.as_str())
            .collect::<Vec<_>>()
            .join("; ")
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Entry {
    /// `dict::sources::Source::id` this entry came from.
    pub source: String,
    /// The source's own number: JMdict `ent_seq`.
    pub id: i64,
    pub kanji: Vec<String>,
    pub readings: Vec<String>,
    pub senses: Vec<Sense>,
    pub common: bool,
}

impl Entry {
    pub fn headword(&self) -> &str {
        self.kanji
            .first()
            .or(self.readings.first())
            .map_or("", String::as_str)
    }

    pub fn reading(&self) -> &str {
        self.readings.first().map_or("", String::as_str)
    }

    /// Languages that have at least one gloss, in first-seen order.
    pub fn languages(&self) -> Vec<&str> {
        let mut seen: Vec<&str> = Vec::new();
        for g in self.senses.iter().flat_map(|s| &s.glosses) {
            if !seen.contains(&g.lang.as_str()) {
                seen.push(&g.lang);
            }
        }
        seen
    }

    /// First gloss in the first of `langs` that has one; what a result row shows.
    pub fn summary(&self, langs: &[String]) -> &str {
        for lang in langs {
            for s in &self.senses {
                if let Some(g) = s.glosses.iter().find(|g| &g.lang == lang) {
                    return &g.text;
                }
            }
        }
        ""
    }
}
