//! Just enough CSV for word lists: RFC 4180 quoting, any of comma, semicolon or tab as separator
//! when reading, a chosen separator when writing. No dependency for a format this small.

/// One row, fields quoted where needed.
pub fn row(fields: &[&str], separator: char) -> String {
    let quoted: Vec<String> = fields
        .iter()
        .map(|f| {
            if f.contains(separator) || f.contains('"') || f.contains('\n') || f.contains('\r') {
                format!("\"{}\"", f.replace('"', "\"\""))
            } else {
                (*f).to_string()
            }
        })
        .collect();
    let mut line = quoted.join(&separator.to_string());
    line.push('\n');
    line
}

/// Rows of fields. The separator is whichever of tab, semicolon or comma the first line uses
/// (tab wins if present). A leading byte-order mark is dropped, as are empty lines.
pub fn parse(text: &str) -> Vec<Vec<String>> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let first = text.lines().next().unwrap_or("");
    let separator = if first.contains('\t') {
        '\t'
    } else if first.contains(';') && !first.contains(',') {
        ';'
    } else {
        ','
    };
    let mut rows = Vec::new();
    let mut fields = Vec::new();
    let mut field = String::new();
    let mut quoted = false;
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if quoted {
            match c {
                '"' if chars.peek() == Some(&'"') => {
                    chars.next();
                    field.push('"');
                }
                '"' => quoted = false,
                c => field.push(c),
            }
        } else {
            match c {
                '"' if field.is_empty() => quoted = true,
                c if c == separator => fields.push(std::mem::take(&mut field)),
                '\r' => {}
                '\n' => {
                    fields.push(std::mem::take(&mut field));
                    if fields.iter().any(|f| !f.is_empty()) {
                        rows.push(std::mem::take(&mut fields));
                    } else {
                        fields.clear();
                    }
                }
                c => field.push(c),
            }
        }
    }
    if !field.is_empty() || !fields.is_empty() {
        fields.push(field);
        if fields.iter().any(|f| !f.is_empty()) {
            rows.push(fields);
        }
    }
    rows
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_with_quoting() {
        assert_eq!(row(&["猫", "ねこ", "cat; feline"], ','), "猫,ねこ,cat; feline\n");
        assert_eq!(
            row(&["a,b", "say \"hi\"", "x"], ','),
            "\"a,b\",\"say \"\"hi\"\"\",x\n"
        );
        assert_eq!(row(&["a", "b\nc"], '\t'), "a\t\"b\nc\"\n");
    }

    #[test]
    fn reads_all_three_separators_and_bom() {
        assert_eq!(parse("\u{feff}a,b\n\n1,2\r\n"), [["a", "b"], ["1", "2"]]);
        assert_eq!(parse("a\tb\n1\t2,3"), [vec!["a", "b"], vec!["1", "2,3"]]);
        assert_eq!(parse("a;b\n1;2"), [["a", "b"], ["1", "2"]]);
        assert_eq!(parse("\"x,y\",\"say \"\"hi\"\"\"\n"), [["x,y", "say \"hi\""]]);
        assert!(parse("").is_empty());
        assert!(parse(",,\n").is_empty());
    }

    #[test]
    fn roundtrip() {
        let fields = ["猫, , ねこ", "line\nbreak", "\"q\""];
        let text = row(&fields, ',');
        assert_eq!(parse(&text), [fields]);
    }
}
