//! Deinflection: from 食べませんでした back to 食べる, with the chain of forms taken off.
//!
//! A rule table in the style of Yomitan's: each rule strips a suffix and says what kind of word it
//! can apply to and what kind comes out. Candidates are generated blindly and verified against the
//! dictionary by the caller (does the word exist, with a matching part of speech?), so an over-eager
//! rule costs a lookup, never a wrong result.

/// Word kinds, as bit flags. The intermediate forms borrow the kind they inflect like: ない and
/// たい are i-adjectives, ます-forms and potentials are ichidan-like.
pub const V1: u8 = 1; // ichidan (食べる)
pub const V5: u8 = 2; // godan (書く)
pub const ADJ_I: u8 = 4; // i-adjective (高い), also ない / たい forms
pub const VK: u8 = 8; // 来る
pub const VS: u8 = 16; // する
pub const ANY: u8 = V1 | V5 | ADJ_I | VK | VS;

struct Rule {
    from: &'static str,
    to: &'static str,
    /// Kinds the inflected word may be for the rule to apply.
    input: u8,
    /// Kind of the word that comes out.
    output: u8,
    reason: &'static str,
}

macro_rules! rules {
    ($( $from:literal => $to:literal, $input:expr, $output:expr, $reason:literal; )*) => {
        &[ $( Rule { from: $from, to: $to, input: $input, output: $output, reason: $reason }, )* ]
    };
}

/// Godan endings by row: (u-row dictionary ending, i-stem, a-stem, e-stem, o-stem, te/ta forms).
const GODAN: &[(&str, &str, &str, &str, &str, &str)] = &[
    ("く", "き", "か", "け", "こ", "い"),
    ("ぐ", "ぎ", "が", "げ", "ご", "い"),
    ("す", "し", "さ", "せ", "そ", "し"),
    ("つ", "ち", "た", "て", "と", "っ"),
    ("ぬ", "に", "な", "ね", "の", "ん"),
    ("ぶ", "び", "ば", "べ", "ぼ", "ん"),
    ("む", "み", "ま", "め", "も", "ん"),
    ("る", "り", "ら", "れ", "ろ", "っ"),
    ("う", "い", "わ", "え", "お", "っ"),
];

const RULES: &[Rule] = rules! {
    // polite
    "ませんでした" => "ません", ANY, ANY, "past";
    "ました" => "ます", ANY, ANY, "past";
    "ません" => "ます", ANY, ANY, "negative";
    "ましょう" => "ます", ANY, ANY, "volitional";
    "ます" => "る", ANY, V1, "polite";
    "します" => "する", ANY, VS, "polite";
    "きます" => "くる", ANY, VK, "polite";
    "来ます" => "来る", ANY, VK, "polite";
    // past and te-form, ichidan / irregular / adjectives
    "た" => "る", ANY, V1, "past";
    "て" => "る", ANY, V1, "te-form";
    "した" => "する", ANY, VS, "past";
    "して" => "する", ANY, VS, "te-form";
    "きた" => "くる", ANY, VK, "past";
    "きて" => "くる", ANY, VK, "te-form";
    "来た" => "来る", ANY, VK, "past";
    "来て" => "来る", ANY, VK, "te-form";
    "行った" => "行く", ANY, V5, "past";
    "行って" => "行く", ANY, V5, "te-form";
    "かった" => "い", ANY, ADJ_I, "past";
    "くて" => "い", ANY, ADJ_I, "te-form";
    "くない" => "い", ANY, ADJ_I, "negative";
    "ければ" => "い", ANY, ADJ_I, "conditional";
    "く" => "い", ANY, ADJ_I, "adverbial";
    "そう" => "い", ANY, ADJ_I, "-sou (seems)";
    // negative, ichidan / irregular
    "ない" => "る", ADJ_I, V1, "negative";
    "しない" => "する", ADJ_I, VS, "negative";
    "こない" => "くる", ADJ_I, VK, "negative";
    "来ない" => "来る", ADJ_I, VK, "negative";
    "ず" => "る", ANY, V1, "negative (-zu)";
    // tai
    "たい" => "る", ADJ_I, V1, "-tai (want to)";
    "したい" => "する", ADJ_I, VS, "-tai (want to)";
    "きたい" => "くる", ADJ_I, VK, "-tai (want to)";
    // potential, passive, causative (ichidan / irregular)
    "られる" => "る", V1, V1, "potential or passive";
    "れる" => "る", V1, V1, "potential (ra-nuki)";
    "させる" => "る", V1, V1, "causative";
    "できる" => "する", V1, VS, "potential";
    "される" => "する", V1, VS, "passive";
    "させる" => "する", V1, VS, "causative";
    "こられる" => "くる", V1, VK, "potential or passive";
    "こさせる" => "くる", V1, VK, "causative";
    // volitional, conditional, imperative (ichidan / irregular)
    "よう" => "る", ANY, V1, "volitional";
    "しよう" => "する", ANY, VS, "volitional";
    "こよう" => "くる", ANY, VK, "volitional";
    "れば" => "る", ANY, V1, "conditional";
    "すれば" => "する", ANY, VS, "conditional";
    "くれば" => "くる", ANY, VK, "conditional";
    "たら" => "た", ANY, ANY, "conditional (-tara)";
    "だら" => "だ", ANY, ANY, "conditional (-tara)";
    "ろ" => "る", ANY, V1, "imperative";
    "しろ" => "する", ANY, VS, "imperative";
    "こい" => "くる", ANY, VK, "imperative";
    // progressive
    "ている" => "て", ANY, ANY, "progressive";
    "てる" => "て", ANY, ANY, "progressive";
    "でいる" => "で", ANY, ANY, "progressive";
    "でる" => "で", ANY, ANY, "progressive";
    // stem of an ichidan verb
    "" => "る", ANY, V1, "stem";
};

/// A dictionary form the query may inflect from, with the forms taken off, innermost last.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Candidate {
    pub word: String,
    /// What kind of word this must be for the chain to be valid.
    pub kinds: u8,
    /// The rules applied, in the order they were taken off (outermost first).
    pub reasons: Vec<&'static str>,
}

const MAX_DEPTH: usize = 6;

/// All dictionary forms `text` could be an inflection of. The text itself is not included.
pub fn deinflect(text: &str) -> Vec<Candidate> {
    let mut out: Vec<Candidate> = Vec::new();
    let mut queue: Vec<Candidate> = vec![Candidate {
        word: text.to_string(),
        kinds: ANY,
        reasons: Vec::new(),
    }];
    while let Some(current) = queue.first().cloned() {
        queue.remove(0);
        if current.reasons.len() >= MAX_DEPTH {
            continue;
        }
        let mut step = |word: String, kinds: u8, reason: &'static str| {
            if word.is_empty() || word == text {
                return;
            }
            let mut reasons = current.reasons.clone();
            reasons.push(reason);
            let next = Candidate { word, kinds, reasons };
            if !out.iter().any(|c| c.word == next.word && c.kinds == next.kinds) {
                out.push(next.clone());
                queue.push(next);
            }
        };
        for rule in RULES {
            if current.kinds & rule.input == 0 {
                continue;
            }
            if let Some(stem) = current.word.strip_suffix(rule.from) {
                step(format!("{stem}{}", rule.to), rule.output, rule.reason);
            }
        }
        // Godan rules, derived from the table so they stay consistent.
        for &(u, i, a, e, o, te) in GODAN {
            let w = current.word.as_str();
            let godan = [
                (format!("{i}ます"), u, ANY, "polite"),
                (format!("{te}た"), u, ANY, "past"),
                (format!("{te}て"), u, ANY, "te-form"),
                (format!("{a}ない"), u, ADJ_I, "negative"),
                (format!("{a}ず"), u, ANY, "negative (-zu)"),
                (format!("{i}たい"), u, ADJ_I, "-tai (want to)"),
                (format!("{e}る"), u, V1, "potential"),
                (format!("{a}れる"), u, V1, "passive"),
                (format!("{a}せる"), u, V1, "causative"),
                (format!("{o}う"), u, ANY, "volitional"),
                (format!("{e}ば"), u, ANY, "conditional"),
                (e.to_string(), u, ANY, "imperative"),
                (i.to_string(), u, ANY, "stem"),
            ];
            for (from, to, input, reason) in godan {
                if current.kinds & input == 0 {
                    continue;
                }
                let voiced = (te == "い" && u == "ぐ") || (te == "ん");
                let from = if voiced && (reason == "past" || reason == "te-form") {
                    from.replace('た', "だ").replace('て', "で")
                } else {
                    from
                };
                if let Some(stem) = w.strip_suffix(from.as_str()) {
                    step(format!("{stem}{to}"), V5, reason);
                }
            }
        }
    }
    out
}

/// True if `pos` (a JMdict part-of-speech description) fits one of `kinds`.
pub fn pos_matches(pos: &str, kinds: u8) -> bool {
    (kinds & V1 != 0 && pos.contains("Ichidan"))
        || (kinds & V5 != 0 && pos.contains("Godan"))
        || (kinds & ADJ_I != 0 && pos.contains("adjective (keiyoushi)"))
        || (kinds & VK != 0 && pos.contains("Kuru"))
        || (kinds & VS != 0 && pos.contains("suru"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chain(text: &str, word: &str) -> Vec<&'static str> {
        deinflect(text)
            .into_iter()
            .filter(|c| c.word == word)
            .min_by_key(|c| c.reasons.len())
            .map(|c| c.reasons)
            .unwrap_or_else(|| panic!("{text} does not deinflect to {word}"))
    }

    #[test]
    fn verbs_and_adjectives() {
        assert_eq!(chain("食べました", "食べる"), ["past", "polite"]);
        assert_eq!(
            chain("食べませんでした", "食べる"),
            ["past", "negative", "polite"]
        );
        assert_eq!(chain("書かない", "書く"), ["negative"]);
        assert_eq!(chain("書きました", "書く"), ["past", "polite"]);
        assert_eq!(chain("飲んだ", "飲む"), ["past"]);
        assert_eq!(chain("泳いで", "泳ぐ"), ["te-form"]);
        assert_eq!(chain("行った", "行く"), ["past"]);
        assert_eq!(chain("話せる", "話す"), ["potential"]);
        assert_eq!(chain("食べられる", "食べる"), ["potential or passive"]);
        assert_eq!(chain("書かれました", "書く"), ["past", "polite", "passive"]);
        assert_eq!(chain("行けば", "行く"), ["conditional"]);
        assert_eq!(chain("高くなかった", "高い"), ["past", "negative"]);
        assert_eq!(chain("来なかった", "来る"), ["past", "negative"]);
        assert_eq!(chain("勉強しました", "勉強する"), ["past", "polite"]);
        assert_eq!(chain("食べている", "食べる"), ["progressive", "te-form"]);
        assert_eq!(chain("読みたい", "読む"), ["-tai (want to)"]);
        assert_eq!(chain("食べ", "食べる"), ["stem"]);
        assert_eq!(chain("書き", "書く"), ["stem"]);
    }

    #[test]
    fn kinds_and_pos() {
        let c = deinflect("書きました");
        let kaku = c.iter().find(|c| c.word == "書く").unwrap();
        assert_eq!(kaku.kinds, V5);
        assert!(pos_matches("Godan verb with 'ku' ending", kaku.kinds));
        assert!(!pos_matches("noun (common) (futsuumeishi)", kaku.kinds));
        assert!(pos_matches(
            "noun or participle which takes the aux. verb suru",
            VS
        ));
    }
}
