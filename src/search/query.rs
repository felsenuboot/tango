//! The search box syntax: `#tags`, `"exact"` quotes and `*`/`?` wildcards around the text.
//!
//! `#common verb`, `#verb 食べ`, `"cat"`, `猫*`, `ne?o`, `#sentences 猫`. Tags filter the hits;
//! quotes ask for a whole gloss or an exact form; wildcards switch to a pattern search;
//! `#sentences` searches the example sentences instead of the dictionary.

/// A filter on the hits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tag {
    Common,
    /// A substring of a JMdict part-of-speech description ("noun", "verb", "adverb" …).
    Pos(&'static str),
    /// A substring of a JMdict misc marker ("abbreviation", "kana alone").
    Misc(&'static str),
    /// Not a filter: search the Tatoeba sentences instead of the entries.
    Sentences,
    /// Not a filter: search the JMnedict names instead of the words.
    Names,
}

/// Tag names as typed, without the `#`.
pub const TAGS: &[(&str, Tag)] = &[
    ("common", Tag::Common),
    ("noun", Tag::Pos("noun")),
    ("verb", Tag::Pos("verb")),
    ("adjective", Tag::Pos("adjectiv")),
    ("adverb", Tag::Pos("adverb")),
    ("expression", Tag::Pos("expression")),
    ("counter", Tag::Pos("counter")),
    ("particle", Tag::Pos("particle")),
    ("prefix", Tag::Pos("prefix")),
    ("suffix", Tag::Pos("suffix")),
    ("pronoun", Tag::Pos("pronoun")),
    ("conjunction", Tag::Pos("conjunction")),
    ("interjection", Tag::Pos("interjection")),
    ("abbreviation", Tag::Misc("abbreviation")),
    ("kana", Tag::Misc("kana alone")),
    ("sentences", Tag::Sentences),
    ("sentence", Tag::Sentences),
    ("names", Tag::Names),
    ("name", Tag::Names),
];

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Query {
    /// The text without tags and quotes.
    pub text: String,
    pub tags: Vec<Tag>,
    /// `#tags` nobody knows, for the "no results" hint.
    pub unknown_tags: Vec<String>,
    /// The text was in double quotes: whole gloss or exact form only.
    pub exact: bool,
    /// The text contains `*` or `?`.
    pub wildcard: bool,
    /// `#sentences`: look in the example sentences, not the dictionary.
    pub sentences: bool,
    /// `#names`: look in JMnedict only.
    pub names: bool,
}

pub fn parse(input: &str) -> Query {
    let mut q = Query::default();
    let mut words: Vec<&str> = Vec::new();
    for token in input.split_whitespace() {
        if let Some(name) = token.strip_prefix('#').filter(|n| !n.is_empty()) {
            let name = name.to_lowercase();
            match TAGS.iter().find(|(n, _)| *n == name) {
                Some((_, Tag::Sentences)) => q.sentences = true,
                Some((_, Tag::Names)) => q.names = true,
                Some((_, tag)) => q.tags.push(*tag),
                None => q.unknown_tags.push(name),
            }
        } else {
            words.push(token);
        }
    }
    let mut text = words.join(" ");
    if text.len() >= 2 && text.starts_with('"') && text.ends_with('"') {
        text = text[1..text.len() - 1].trim().to_string();
        q.exact = true;
    }
    q.wildcard = text.contains('*') || text.contains('?');
    q.text = text;
    q
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tags_quotes_and_wildcards() {
        let q = parse("#common #Verb  食べ");
        assert_eq!(q.text, "食べ");
        assert_eq!(q.tags, [Tag::Common, Tag::Pos("verb")]);
        assert!(q.unknown_tags.is_empty());
        let q = parse("\"domestic cat\" #jlpt-n5");
        assert_eq!(q.text, "domestic cat");
        assert!(q.exact);
        assert_eq!(q.unknown_tags, ["jlpt-n5"]);
        let q = parse("ne?o*");
        assert!(q.wildcard && !q.exact);
        assert_eq!(parse("#").text, "#");
        let q = parse("#names 佐藤");
        assert!(q.names);
        assert_eq!(q.text, "佐藤");
        let q = parse("#sentences 猫が");
        assert!(q.sentences);
        assert!(q.tags.is_empty());
        assert_eq!(q.text, "猫が");
        assert_eq!(parse("\"\"").text, "");
    }
}
