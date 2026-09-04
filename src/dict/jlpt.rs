//! JLPT vocabulary levels: Jonathan Waller's unofficial N5–N1 lists (there are no official ones)
//! with a JMdict number per word, as published by Stephen Kraus in
//! <https://github.com/stephenmk/yomitan-jlpt-vocab> (`original_data/n5.csv` …). One CSV per
//! level: `jmdict_seq,kana,kanji,waller_definition`; only the number is needed here.

use crate::store::csv;

/// The cache file per level, N5 first; the source's main file is the first.
pub const FILES: [(&str, u8); 5] = [
    ("jlpt-n5.csv", 5),
    ("jlpt-n4.csv", 4),
    ("jlpt-n3.csv", 3),
    ("jlpt-n2.csv", 2),
    ("jlpt-n1.csv", 1),
];

/// The JMdict numbers in one level's CSV; the header line and anything unparsable are skipped.
pub fn parse(text: &str) -> Vec<i64> {
    csv::parse(text)
        .iter()
        .filter_map(|row| row.first()?.trim().parse::<i64>().ok())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_come_out_of_the_csv() {
        let text = "jmdict_seq,kana,kanji,waller_definition\n1467640,ねこ,猫,cat\n1000225,めいはく,明白,\"obvious, clear\"\n\n";
        assert_eq!(parse(text), [1467640, 1000225]);
        assert!(parse("").is_empty());
    }
}
