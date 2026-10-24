//! Writing values as Rhai source text, for recorded scripts and "Copy as
//! script": JSON objects become object maps (`#{ key: value }`), arrays stay
//! arrays, `null` becomes `()` and numbers keep every digit needed to read
//! back the same `f64`.

use stat_data::JsonValue;

/// Line length above which a map is written one member per line.
const WIDTH: usize = 96;

/// A Rhai string literal for `text`.
#[must_use]
pub fn string(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    out.push('"');
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\0' => out.push_str("\\0"),
            c if c.is_control() => out.push_str(&format!("\\u{:04x}", u32::from(c))),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// A Rhai number literal: whole numbers (within ±2⁵³) as integers, others
/// as the shortest decimal that reads back exactly; `()` for NaN and the
/// infinities, which have no literal.
#[must_use]
pub fn number(x: f64) -> String {
    if !x.is_finite() {
        return "()".to_owned();
    }
    if x.fract() == 0.0 && x.abs() < 9.007_199_254_740_992e15 {
        // `+ 0.0` turns −0 into 0.
        return format!("{}", (x + 0.0) as i64);
    }
    let text = format!("{x:?}");
    // Rhai needs a digit after the point before an exponent ("1.0e-10").
    match text.split_once('e') {
        Some((mantissa, exp)) if !mantissa.contains('.') => format!("{mantissa}.0e{exp}"),
        _ => text,
    }
}

/// Words Rhai reserves, which cannot be written bare as map keys or
/// variable names.
pub(crate) const RESERVED: &[&str] = &[
    "true",
    "false",
    "let",
    "const",
    "if",
    "else",
    "switch",
    "do",
    "while",
    "until",
    "loop",
    "for",
    "in",
    "continue",
    "break",
    "return",
    "throw",
    "try",
    "catch",
    "fn",
    "private",
    "import",
    "export",
    "as",
    "global",
    "this",
    "is",
    "var",
    "static",
    "shared",
    "goto",
    "exit",
    "match",
    "case",
    "public",
    "protected",
    "new",
    "use",
    "with",
    "module",
    "package",
    "super",
    "thread",
    "spawn",
    "go",
    "await",
    "async",
    "sync",
    "yield",
    "default",
    "void",
    "null",
    "nil",
    "Fn",
    "call",
    "curry",
    "print",
    "debug",
    "eval",
    "type_of",
    "is_shared",
    "is_def_var",
    "is_def_fn",
];

/// A map key: bare when it is a plain identifier, else quoted.
fn key(name: &str) -> String {
    let mut chars = name.chars();
    let plain = chars.next().is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
        && !RESERVED.contains(&name);
    if plain { name.to_owned() } else { string(name) }
}

/// `value` as Rhai source, on one line.
#[must_use]
pub fn compact(value: &JsonValue) -> String {
    match value {
        JsonValue::Null => "()".to_owned(),
        JsonValue::Bool(b) => b.to_string(),
        JsonValue::Number(x) => number(*x),
        JsonValue::String(s) => string(s),
        JsonValue::Array(items) => format!("[{}]", items.iter().map(compact).collect::<Vec<_>>().join(", ")),
        JsonValue::Object(members) if members.is_empty() => "#{}".to_owned(),
        JsonValue::Object(members) => format!(
            "#{{ {} }}",
            members.iter().map(|(k, v)| format!("{}: {}", key(k), compact(v))).collect::<Vec<_>>().join(", ")
        ),
    }
}

/// `value` as Rhai source, breaking maps that would run past the line
/// width one member per line; `indent` is the current indentation.
#[must_use]
pub fn pretty(value: &JsonValue, indent: usize) -> String {
    let one_line = compact(value);
    let JsonValue::Object(members) = value else { return one_line };
    if indent + one_line.len() <= WIDTH || members.is_empty() {
        return one_line;
    }
    let pad = " ".repeat(indent + 4);
    let mut out = "#{\n".to_owned();
    for (k, v) in members {
        let k = key(k);
        out.push_str(&format!("{pad}{k}: {},\n", pretty(v, indent + 4)));
    }
    out.push_str(&" ".repeat(indent));
    out.push('}');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn literals() {
        assert_eq!(string("a \"b\" \\ c\nd"), r#""a \"b\" \\ c\nd""#);
        assert_eq!(number(3.0), "3");
        assert_eq!(number(-0.0), "0");
        assert_eq!(number(0.1), "0.1");
        assert_eq!(number(1e-10), "1.0e-10");
        assert_eq!(number(2.5e30), "2.5e30");
        assert_eq!(number(1e20), "1.0e20");
        assert_eq!(number(f64::NAN), "()");
        let v = JsonValue::Object(vec![
            ("response".into(), JsonValue::String("Y".into())),
            ("type".into(), JsonValue::Null),
            ("two words".into(), JsonValue::Array(vec![JsonValue::Number(1.0), JsonValue::Bool(true)])),
            ("default".into(), JsonValue::Object(Vec::new())),
        ]);
        assert_eq!(compact(&v), r#"#{ response: "Y", type: (), "two words": [1, true], "default": #{} }"#);
        // Long maps break one member per line.
        let long = JsonValue::Object(
            (0..8).map(|i| (format!("member_{i}"), JsonValue::String("some value".into()))).collect(),
        );
        let text = pretty(&long, 0);
        assert!(text.starts_with("#{\n    member_0: \"some value\",\n"), "{text}");
        assert!(text.ends_with(",\n}"));
    }
}
