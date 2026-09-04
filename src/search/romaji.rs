//! Romaji → kana for the search box.
//!
//! Hepburn plus the variants people type: `si`/`shi`, `tu`/`tsu`, `hu`/`fu`, `zi`/`ji`, `nn` or `n'`
//! for ん, doubled consonants for っ, `x`/`l` prefixes for small kana, macrons or `-` for long
//! vowels. Conversion succeeds only when the whole text is romaji, so an English word like "cat"
//! (no `ca` syllable) stays a gloss search, while "sake" becomes さけ as well.

/// Longest-first syllable table. `n` alone is handled separately.
const SYLLABLES: &[(&str, &str)] = &[
    ("kya", "きゃ"),
    ("kyu", "きゅ"),
    ("kyo", "きょ"),
    ("gya", "ぎゃ"),
    ("gyu", "ぎゅ"),
    ("gyo", "ぎょ"),
    ("sha", "しゃ"),
    ("shu", "しゅ"),
    ("sho", "しょ"),
    ("sya", "しゃ"),
    ("syu", "しゅ"),
    ("syo", "しょ"),
    ("shi", "し"),
    ("chi", "ち"),
    ("tsu", "つ"),
    ("cha", "ちゃ"),
    ("chu", "ちゅ"),
    ("cho", "ちょ"),
    ("tya", "ちゃ"),
    ("tyu", "ちゅ"),
    ("tyo", "ちょ"),
    ("nya", "にゃ"),
    ("nyu", "にゅ"),
    ("nyo", "にょ"),
    ("hya", "ひゃ"),
    ("hyu", "ひゅ"),
    ("hyo", "ひょ"),
    ("mya", "みゃ"),
    ("myu", "みゅ"),
    ("myo", "みょ"),
    ("rya", "りゃ"),
    ("ryu", "りゅ"),
    ("ryo", "りょ"),
    ("jya", "じゃ"),
    ("jyu", "じゅ"),
    ("jyo", "じょ"),
    ("zya", "じゃ"),
    ("zyu", "じゅ"),
    ("zyo", "じょ"),
    ("bya", "びゃ"),
    ("byu", "びゅ"),
    ("byo", "びょ"),
    ("pya", "ぴゃ"),
    ("pyu", "ぴゅ"),
    ("pyo", "ぴょ"),
    ("dya", "ぢゃ"),
    ("dyu", "ぢゅ"),
    ("dyo", "ぢょ"),
    ("xtsu", "っ"),
    ("xtu", "っ"),
    ("ltsu", "っ"),
    ("ltu", "っ"),
    ("xya", "ゃ"),
    ("xyu", "ゅ"),
    ("xyo", "ょ"),
    ("lya", "ゃ"),
    ("lyu", "ゅ"),
    ("lyo", "ょ"),
    ("ja", "じゃ"),
    ("ju", "じゅ"),
    ("jo", "じょ"),
    ("ji", "じ"),
    ("zi", "じ"),
    ("fu", "ふ"),
    ("hu", "ふ"),
    ("fa", "ふぁ"),
    ("fi", "ふぃ"),
    ("fe", "ふぇ"),
    ("fo", "ふぉ"),
    ("si", "し"),
    ("ti", "ち"),
    ("tu", "つ"),
    ("di", "ぢ"),
    ("du", "づ"),
    ("dzu", "づ"),
    ("ka", "か"),
    ("ki", "き"),
    ("ku", "く"),
    ("ke", "け"),
    ("ko", "こ"),
    ("ga", "が"),
    ("gi", "ぎ"),
    ("gu", "ぐ"),
    ("ge", "げ"),
    ("go", "ご"),
    ("sa", "さ"),
    ("su", "す"),
    ("se", "せ"),
    ("so", "そ"),
    ("za", "ざ"),
    ("zu", "ず"),
    ("ze", "ぜ"),
    ("zo", "ぞ"),
    ("ta", "た"),
    ("te", "て"),
    ("to", "と"),
    ("da", "だ"),
    ("de", "で"),
    ("do", "ど"),
    ("na", "な"),
    ("ni", "に"),
    ("nu", "ぬ"),
    ("ne", "ね"),
    ("no", "の"),
    ("ha", "は"),
    ("hi", "ひ"),
    ("he", "へ"),
    ("ho", "ほ"),
    ("ba", "ば"),
    ("bi", "び"),
    ("bu", "ぶ"),
    ("be", "べ"),
    ("bo", "ぼ"),
    ("pa", "ぱ"),
    ("pi", "ぴ"),
    ("pu", "ぷ"),
    ("pe", "ぺ"),
    ("po", "ぽ"),
    ("ma", "ま"),
    ("mi", "み"),
    ("mu", "む"),
    ("me", "め"),
    ("mo", "も"),
    ("ya", "や"),
    ("yu", "ゆ"),
    ("yo", "よ"),
    ("ra", "ら"),
    ("ri", "り"),
    ("ru", "る"),
    ("re", "れ"),
    ("ro", "ろ"),
    ("wa", "わ"),
    ("wo", "を"),
    ("we", "ゑ"),
    ("wi", "ゐ"),
    ("va", "ゔぁ"),
    ("vi", "ゔぃ"),
    ("vu", "ゔ"),
    ("ve", "ゔぇ"),
    ("vo", "ゔぉ"),
    ("xa", "ぁ"),
    ("xi", "ぃ"),
    ("xu", "ぅ"),
    ("xe", "ぇ"),
    ("xo", "ぉ"),
    ("la", "ぁ"),
    ("li", "ぃ"),
    ("lu", "ぅ"),
    ("le", "ぇ"),
    ("lo", "ぉ"),
    ("a", "あ"),
    ("i", "い"),
    ("u", "う"),
    ("e", "え"),
    ("o", "お"),
    ("-", "ー"),
];

fn is_vowel(c: u8) -> bool {
    matches!(c, b'a' | b'i' | b'u' | b'e' | b'o')
}

/// Lowercases and spells macrons out (ō → ou, as dictionaries write it).
fn normalise(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 4);
    for c in text.chars().flat_map(char::to_lowercase) {
        match c {
            'ā' => out.push_str("aa"),
            'ī' => out.push_str("ii"),
            'ū' => out.push_str("uu"),
            'ē' => out.push_str("ee"),
            'ō' => out.push_str("ou"),
            'ô' => out.push_str("ou"),
            'û' => out.push_str("uu"),
            c => out.push(c),
        }
    }
    out
}

/// Hiragana for romaji text, or `None` if any part of it is not romaji.
pub fn to_hiragana(text: &str) -> Option<String> {
    let s = normalise(text);
    let b = s.as_bytes();
    let mut out = String::new();
    let mut i = 0;
    while i < b.len() {
        let c = b[i];
        if !c.is_ascii_lowercase() && c != b'-' && c != b'\'' {
            return None;
        }
        // ん: "n" before a consonant or the end, "n'", and "nn" when no syllable can follow.
        // "kanna" is Hepburn for かんな (the second n starts な); "honn" and "konnnichiwa" are
        // how IMEs spell ほん and こんにちわ (two n's make one ん).
        if c == b'n' {
            let next = b.get(i + 1).copied();
            let third = b.get(i + 2).copied();
            let starts_syllable = |x: Option<u8>| x.is_some_and(|x| is_vowel(x) || x == b'y');
            match next {
                None => {
                    out.push('ん');
                    i += 1;
                    continue;
                }
                Some(b'\'') => {
                    out.push('ん');
                    i += 2;
                    continue;
                }
                Some(b'n') => {
                    out.push('ん');
                    i += if starts_syllable(third) { 1 } else { 2 };
                    continue;
                }
                Some(n) if !is_vowel(n) && n != b'y' => {
                    out.push('ん');
                    i += 1;
                    continue;
                }
                _ => {}
            }
        }
        // っ: a doubled consonant (kk, tt, pp, ss, cch ...), not "nn".
        if c.is_ascii_alphabetic() && !is_vowel(c) && c != b'n' && b.get(i + 1) == Some(&c) {
            out.push('っ');
            i += 1;
            continue;
        }
        if c == b't' && b.get(i + 1) == Some(&b'c') && b.get(i + 2) == Some(&b'h') {
            out.push('っ');
            i += 1;
            continue;
        }
        let rest = &s[i..];
        let (rom, kana) = SYLLABLES.iter().find(|(r, _)| rest.starts_with(r))?;
        out.push_str(kana);
        i += rom.len();
    }
    if out.is_empty() { None } else { Some(out) }
}

/// Katakana for romaji text, or `None` if any part of it is not romaji.
pub fn to_katakana(text: &str) -> Option<String> {
    to_hiragana(text).map(|h| hiragana_to_katakana(&h))
}

pub fn hiragana_to_katakana(text: &str) -> String {
    text.chars()
        .map(|c| match c {
            'ぁ'..='ゖ' => char::from_u32(c as u32 + 0x60).unwrap_or(c),
            c => c,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hepburn_and_variants() {
        for (rom, kana) in [
            ("neko", "ねこ"),
            ("taberu", "たべる"),
            ("shinbun", "しんぶん"),
            ("sinbun", "しんぶん"),
            ("kon'nichiwa", "こんにちわ"),
            ("konnnichiwa", "こんにちわ"),
            ("honn", "ほん"),
            ("honnto", "ほんと"),
            ("kanna", "かんな"),
            ("gakkou", "がっこう"),
            ("matcha", "まっちゃ"),
            ("tsukue", "つくえ"),
            ("tukue", "つくえ"),
            ("fujisan", "ふじさん"),
            ("Tōkyō", "とうきょう"),
            ("ra-men", "らーめん"),
            ("kyou", "きょう"),
            ("xtsu", "っ"),
            ("jisho", "じしょ"),
            ("hon", "ほん"),
            ("wo", "を"),
        ] {
            assert_eq!(to_hiragana(rom).as_deref(), Some(kana), "{rom}");
        }
    }

    #[test]
    fn not_romaji_stays_none() {
        for text in ["cat", "Katze", "hello world", "q", "書く", ""] {
            assert_eq!(to_hiragana(text), None, "{text:?}");
        }
    }

    #[test]
    fn katakana() {
        assert_eq!(to_katakana("ra-men").as_deref(), Some("ラーメン"));
        assert_eq!(to_katakana("neko").as_deref(), Some("ネコ"));
    }
}
