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
    /// `<s_inf>`: notes on the sense ("often of a person").
    pub info: Vec<String>,
    /// `<dial>`: "Kansai-ben", ...
    pub dialects: Vec<String>,
    /// `<lsource>`, readable: "from English: cat", "wasei, from English".
    pub origins: Vec<String>,
    /// `<xref>`: "猫・ねこ・1", kanji, reading and sense number, any of them.
    pub see_also: Vec<String>,
    /// `<ant>`, same shape as `see_also`.
    pub antonyms: Vec<String>,
    /// `<stagk>` / `<stagr>`: the sense applies to these forms only.
    pub only_for: Vec<String>,
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
    /// `<ke_inf>` per kanji form ("ateji (phonetic) reading", "rarely used kanji form");
    /// parallel to `kanji`, but read it with `get`: other sources leave it short.
    pub kanji_info: Vec<Vec<String>>,
    /// `<re_inf>` per reading, parallel to `readings`.
    pub reading_info: Vec<Vec<String>>,
    /// `<re_restr>` per reading: the kanji forms it goes with, empty for all of them.
    pub reading_for: Vec<Vec<String>>,
    /// Pitch accent of the first reading: the mora after which the pitch drops, 0 for a flat
    /// (heiban) word; several when the sources give alternatives. Empty when unknown.
    pub pitch: Vec<u8>,
    pub senses: Vec<Sense>,
    pub common: bool,
}

impl Entry {
    /// Wadoku puts the part of speech on the entry, JMdict on each sense. The reader collects
    /// it here and `finish_wadoku` copies it onto every sense once they are all read.
    pub(crate) fn senses_pos_pending(&mut self) -> &mut Vec<String> {
        if self.senses.is_empty() {
            self.senses.push(Sense::default());
        }
        &mut self.senses[0].pos
    }

    pub(crate) fn finish_wadoku(&mut self) {
        // The first sense may be the grammar holder alone (no glosses): fold it into the rest.
        if self.senses.first().is_some_and(|s| s.glosses.is_empty()) {
            let holder = self.senses.remove(0);
            for s in &mut self.senses {
                if s.pos.is_empty() {
                    s.pos = holder.pos.clone();
                }
            }
        }
    }

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

/// A sense's language. JMdict never mixes languages within one sense, so the first gloss decides;
/// a sense without glosses (cross-reference only) counts as English.
fn sense_lang(sense: &Sense) -> &str {
    sense.glosses.first().map_or("eng", |g| g.lang.as_str())
}

/// One meaning as the entry view shows it: a backbone sense and the translations lined up with it.
#[derive(Debug, PartialEq, Eq)]
pub struct Meaning<'a> {
    pub sense: &'a Sense,
    /// `(language, glosses joined with "; ")`, in display order.
    pub glosses: Vec<(&'a str, String)>,
}

/// The senses of one language that could not be lined up with the numbered meanings.
#[derive(Debug, PartialEq, Eq)]
pub struct LanguageBlock<'a> {
    pub lang: &'a str,
    pub senses: Vec<&'a Sense>,
}

#[derive(Debug, PartialEq, Eq)]
pub struct Grouped<'a> {
    pub meanings: Vec<Meaning<'a>>,
    pub blocks: Vec<LanguageBlock<'a>>,
}

impl Entry {
    /// Groups the senses for display. JMdict keeps every language in its own senses, all the
    /// English ones first and then a block per language, and records no correspondence between
    /// them ("no attempt to align senses between the languages", says the project). In practice a
    /// language with as many senses as English lines up with them by position, so those become
    /// translations of the same meaning; a language with a different count keeps its own block.
    /// `preferred` orders the languages; the rest follow as the entry lists them.
    pub fn grouped(&self, preferred: &[String]) -> Grouped<'_> {
        let available = self.languages();
        let mut order: Vec<&str> = preferred
            .iter()
            .filter_map(|p| available.iter().find(|l| **l == p.as_str()).copied())
            .collect();
        let rest: Vec<&str> = available.iter().copied().filter(|l| !order.contains(l)).collect();
        order.extend(rest);
        let backbone = if available.contains(&"eng") {
            "eng"
        } else {
            order.first().copied().unwrap_or("eng")
        };
        let of =
            |lang: &str| -> Vec<&Sense> { self.senses.iter().filter(|s| sense_lang(s) == lang).collect() };

        let spine = of(backbone);
        let mut meanings: Vec<Meaning> = spine
            .iter()
            .map(|s| Meaning {
                sense: s,
                glosses: Vec::new(),
            })
            .collect();
        let mut blocks = Vec::new();
        for lang in order.iter().copied().filter(|l| *l != backbone) {
            let senses = of(lang);
            if senses.is_empty() {
                continue; // its glosses sit inside the backbone senses (older files)
            }
            if senses.len() == spine.len() {
                for (m, s) in meanings.iter_mut().zip(&senses) {
                    m.glosses.push((lang, s.gloss_text(lang)));
                }
            } else {
                blocks.push(LanguageBlock { lang, senses });
            }
        }
        // Each meaning's own glosses come first (older files put several languages into one
        // sense), then the aligned ones, all in display order.
        for m in &mut meanings {
            let aligned = std::mem::take(&mut m.glosses);
            for lang in &order {
                let own = m.sense.gloss_text(lang);
                if !own.is_empty() {
                    m.glosses.push((lang, own));
                } else if let Some((_, text)) = aligned.iter().find(|(l, _)| l == lang) {
                    m.glosses.push((lang, text.clone()));
                }
            }
        }
        Grouped { meanings, blocks }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sense(lang: &str, texts: &[&str]) -> Sense {
        Sense {
            glosses: texts
                .iter()
                .map(|t| Gloss {
                    lang: lang.into(),
                    text: (*t).into(),
                })
                .collect(),
            ..Sense::default()
        }
    }

    fn entry(senses: Vec<Sense>) -> Entry {
        Entry {
            senses,
            ..Entry::default()
        }
    }

    fn pref(langs: &[&str]) -> Vec<String> {
        langs.iter().map(|l| l.to_string()).collect()
    }

    fn glosses<'a>(m: &'a Meaning<'a>) -> Vec<(&'a str, &'a str)> {
        m.glosses.iter().map(|(l, t)| (*l, t.as_str())).collect()
    }

    #[test]
    fn equal_counts_line_up_by_position() {
        let e = entry(vec![
            sense("eng", &["hand", "arm"]),
            sense("eng", &["handle"]),
            sense("ger", &["Hand"]),
            sense("ger", &["Griff"]),
            sense("dut", &["hand"]),
        ]);
        let g = e.grouped(&pref(&["ger", "eng"]));
        assert_eq!(g.meanings.len(), 2);
        assert_eq!(glosses(&g.meanings[0]), [("ger", "Hand"), ("eng", "hand; arm")]);
        assert_eq!(glosses(&g.meanings[1]), [("ger", "Griff"), ("eng", "handle")]);
        assert_eq!(g.blocks.len(), 1);
        assert_eq!(g.blocks[0].lang, "dut");
        assert_eq!(g.blocks[0].senses, [&e.senses[4]]);
    }

    #[test]
    fn blocks_follow_the_preferred_order_then_the_entry_order() {
        let e = entry(vec![
            sense("eng", &["cat"]),
            sense("fre", &["chat"]),
            sense("fre", &["minou"]),
            sense("dut", &["kat"]),
            sense("dut", &["poes"]),
        ]);
        let g = e.grouped(&pref(&["dut", "eng"]));
        assert_eq!(g.meanings.len(), 1);
        assert_eq!(glosses(&g.meanings[0]), [("eng", "cat")]);
        let langs: Vec<&str> = g.blocks.iter().map(|b| b.lang).collect();
        assert_eq!(langs, ["dut", "fre"]);
    }

    #[test]
    fn old_style_mixed_senses_stay_inline() {
        let mut mixed = sense("eng", &["cat"]);
        mixed.glosses.push(Gloss {
            lang: "ger".into(),
            text: "Katze".into(),
        });
        let e = entry(vec![mixed]);
        let g = e.grouped(&pref(&["ger", "eng"]));
        assert_eq!(glosses(&g.meanings[0]), [("ger", "Katze"), ("eng", "cat")]);
        assert!(g.blocks.is_empty());
    }

    #[test]
    fn without_english_the_first_preferred_language_is_the_backbone() {
        let e = entry(vec![
            sense("ger", &["Hand"]),
            sense("ger", &["Griff"]),
            sense("fre", &["main"]),
            sense("fre", &["poignée"]),
        ]);
        let g = e.grouped(&pref(&["fre", "eng"]));
        assert_eq!(g.meanings.len(), 2);
        assert_eq!(glosses(&g.meanings[0]), [("fre", "main"), ("ger", "Hand")]);
        assert!(g.blocks.is_empty());
        assert!(entry(Vec::new()).grouped(&pref(&["eng"])).meanings.is_empty());
    }
}

/// One kanji from KANJIDIC2, with the readings and meanings a learner looks up.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Kanji {
    pub literal: char,
    /// School grade 1–6, 8 for the remaining jōyō kanji, 9–10 for name kanji; None otherwise.
    pub grade: Option<u8>,
    pub strokes: u8,
    /// Newspaper frequency rank, 1 for the most common of 2,500; None beyond that.
    pub freq: Option<u16>,
    /// The old four-level JLPT rating KANJIDIC2 carries (1 hardest, 4 easiest).
    pub jlpt: Option<u8>,
    /// Classical radical number (1–214).
    pub radical: u8,
    pub on: Vec<String>,
    pub kun: Vec<String>,
    pub nanori: Vec<String>,
    /// Meanings with ISO 639-2 languages like the entries ("eng", "fre", "spa", "por").
    pub meanings: Vec<Gloss>,
}

impl Kanji {
    pub fn meanings_in(&self, lang: &str) -> Vec<&str> {
        self.meanings
            .iter()
            .filter(|m| m.lang == lang)
            .map(|m| m.text.as_str())
            .collect()
    }
}

/// The stroke order of one kanji from KanjiVG: SVG path data in stroke order, in a 109×109 box.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Strokes {
    pub literal: char,
    pub paths: Vec<String>,
}

/// One radical of the multi-radical lookup and the kanji that contain it.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Radical {
    pub radical: String,
    pub strokes: u8,
    pub kanji: Vec<char>,
}

/// The name of a language JMdict uses a code for (ISO 639-2/B); the code itself when unknown.
pub fn language_name(code: &str) -> String {
    match code {
        "eng" => "English",
        "ger" | "deu" => "German",
        "dut" | "nld" => "Dutch",
        "fre" | "fra" => "French",
        "rus" => "Russian",
        "spa" => "Spanish",
        "hun" => "Hungarian",
        "slv" => "Slovenian",
        "swe" => "Swedish",
        "por" => "Portuguese",
        "ita" => "Italian",
        "lat" => "Latin",
        "chi" | "zho" => "Chinese",
        "kor" => "Korean",
        "grc" => "Ancient Greek",
        "gre" | "ell" => "Greek",
        "ain" => "Ainu",
        "san" => "Sanskrit",
        "ara" => "Arabic",
        "heb" => "Hebrew",
        "pol" => "Polish",
        "tur" => "Turkish",
        "vie" => "Vietnamese",
        "tha" => "Thai",
        "ind" => "Indonesian",
        "may" | "msa" => "Malay",
        "fil" | "tgl" => "Tagalog",
        "hin" => "Hindi",
        "per" | "fas" => "Persian",
        "nor" => "Norwegian",
        "dan" => "Danish",
        "fin" => "Finnish",
        "afr" => "Afrikaans",
        "epo" => "Esperanto",
        "haw" => "Hawaiian",
        "mon" => "Mongolian",
        "tib" | "bod" => "Tibetan",
        "bur" | "mya" => "Burmese",
        "khm" => "Khmer",
        "ukr" => "Ukrainian",
        "cze" | "ces" => "Czech",
        "bul" => "Bulgarian",
        "rum" | "ron" => "Romanian",
        "scr" | "hrv" => "Croatian",
        "srp" => "Serbian",
        "est" => "Estonian",
        "lit" => "Lithuanian",
        "glg" => "Galician",
        "bre" => "Breton",
        "ice" | "isl" => "Icelandic",
        "yid" => "Yiddish",
        "urd" => "Urdu",
        "ben" => "Bengali",
        "tam" => "Tamil",
        "geo" | "kat" => "Georgian",
        "arn" => "Mapudungun",
        "kur" => "Kurdish",
        "mnc" => "Manchu",
        "mol" => "Moldavian",
        "sla" => "Slavic",
        "alg" => "Algonquian",
        "amh" => "Amharic",
        "swa" => "Swahili",
        "som" => "Somali",
        "tah" => "Tahitian",
        other => return other.to_string(),
    }
    .to_string()
}

/// A Tatoeba example sentence in Japanese with its translations.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Sentence {
    /// Tatoeba's sentence number.
    pub id: i64,
    pub text: String,
    /// `(language, text)`, ISO 639-3 as Tatoeba uses it ("eng", "deu"), preferred language first.
    pub translations: Vec<(String, String)>,
    /// How the looked-up word appears in the text, for highlighting; from the corpus index.
    pub surface: Option<String>,
    /// The Tanaka corpus marks sentences that are good examples of a word.
    pub good: bool,
}

/// One word of a sentence in the Tanaka corpus index: `headword(reading)[sense]{surface}~`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SentenceWord {
    /// The JMdict headword (first kanji form, or the reading of a kana word).
    pub headword: String,
    /// Given when the headword alone is ambiguous.
    pub reading: Option<String>,
    /// JMdict sense number, 1-based.
    pub sense: Option<u8>,
    /// JMdict `ent_seq`, given for some words (particles, mostly) instead of a reading.
    pub seq: Option<i64>,
    /// The form in the sentence when it differs from the headword.
    pub surface: Option<String>,
    /// Marked `~`: the sentence is a good example of this word.
    pub good: bool,
}
