//! A small JSON (RFC 8259) value model, parser and writer, plus conversion
//! between [`DataTable`]s and JSON arrays of row objects.
//!
//! # Values
//! [`JsonValue`] keeps object members in document order (a `Vec` of pairs), so
//! a file that is read and written again keeps its layout. Numbers are `f64`.
//!
//! # Tables as JSON
//! A table is written as an array with one object per row, keyed by column
//! name in column order: `[{"x": 1.5, "group": "a"}, …]`. Missing numeric and
//! date-time values are written as `null`; date-time values as ISO 8601
//! strings. On reading, the columns are the union of the keys in first-seen
//! order; a key absent from a row is a missing value. A column is numeric if
//! every non-null value is a number; date-time if every non-null value is a
//! string that reads as a date, time of day or duration
//! ([`crate::temporal::detect_column`]); otherwise text (numbers, booleans and
//! nested values are rendered as text).
//!
//! # Numbers
//! Numbers are written in the shortest form that reads back to the same
//! `f64` ([`format_number`]), so a write–read round trip is exact.
//! Non-finite numbers have no JSON form and are written as `null`.

use crate::table::{Column, ColumnData, DataTable, format_number, is_missing_marker};
use crate::temporal::detect_column;
use stat_core::{Result, StatError};
use std::fmt::Write as _;

/// Nesting deeper than this is rejected, which bounds parser recursion.
const MAX_DEPTH: usize = 256;

/// A JSON value.
#[derive(Debug, Clone, PartialEq)]
pub enum JsonValue {
    /// `null`.
    Null,
    /// `true` or `false`.
    Bool(bool),
    /// A number.
    Number(f64),
    /// A string.
    String(String),
    /// An array.
    Array(Vec<JsonValue>),
    /// An object, members in document order.
    Object(Vec<(String, JsonValue)>),
}

impl JsonValue {
    /// The member called `key` of an object (the first, if repeated).
    #[must_use]
    pub fn get(&self, key: &str) -> Option<&JsonValue> {
        match self {
            Self::Object(members) => members.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }

    /// The number, if this is one.
    #[must_use]
    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Self::Number(x) => Some(*x),
            _ => None,
        }
    }

    /// The number as a non-negative integer, if it is one exactly.
    #[must_use]
    pub fn as_usize(&self) -> Option<usize> {
        self.as_f64().filter(|x| *x >= 0.0 && x.fract() == 0.0 && *x <= 9.007_199_254_740_992e15).map(|x| x as usize)
    }

    /// The string, if this is one.
    #[must_use]
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Self::String(s) => Some(s),
            _ => None,
        }
    }

    /// The boolean, if this is one.
    #[must_use]
    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Self::Bool(b) => Some(*b),
            _ => None,
        }
    }

    /// The elements, if this is an array.
    #[must_use]
    pub fn as_array(&self) -> Option<&[JsonValue]> {
        match self {
            Self::Array(v) => Some(v),
            _ => None,
        }
    }

    /// The members, if this is an object.
    #[must_use]
    pub fn as_object(&self) -> Option<&[(String, JsonValue)]> {
        match self {
            Self::Object(m) => Some(m),
            _ => None,
        }
    }

    /// A number value; non-finite numbers become `null`.
    #[must_use]
    pub fn number(x: f64) -> Self {
        if x.is_finite() { Self::Number(x) } else { Self::Null }
    }

    /// Compact JSON text.
    #[must_use]
    pub fn to_compact(&self) -> String {
        let mut out = String::new();
        self.write(&mut out, None, 0);
        out
    }

    /// Indented JSON text (two spaces per level) with a trailing newline.
    /// Arrays whose elements are all scalars stay on one line.
    #[must_use]
    pub fn to_pretty(&self) -> String {
        let mut out = String::new();
        self.write(&mut out, Some(2), 0);
        out.push('\n');
        out
    }

    fn write(&self, out: &mut String, indent: Option<usize>, depth: usize) {
        let newline = |out: &mut String, depth: usize| {
            if let Some(n) = indent {
                out.push('\n');
                out.extend(std::iter::repeat_n(' ', n * depth));
            }
        };
        match self {
            Self::Null => out.push_str("null"),
            Self::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
            Self::Number(x) => {
                if x.is_finite() {
                    out.push_str(&format_number(*x));
                } else {
                    out.push_str("null");
                }
            }
            Self::String(s) => write_string(out, s),
            Self::Array(items) => {
                let flat = items.iter().all(|v| !matches!(v, Self::Array(_) | Self::Object(_)));
                out.push('[');
                for (i, v) in items.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                        if flat && indent.is_some() {
                            out.push(' ');
                        }
                    }
                    if !flat {
                        newline(out, depth + 1);
                    }
                    v.write(out, indent, depth + 1);
                }
                if !flat && !items.is_empty() {
                    newline(out, depth);
                }
                out.push(']');
            }
            Self::Object(members) => {
                out.push('{');
                for (i, (k, v)) in members.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    newline(out, depth + 1);
                    write_string(out, k);
                    out.push(':');
                    if indent.is_some() {
                        out.push(' ');
                    }
                    v.write(out, indent, depth + 1);
                }
                if !members.is_empty() {
                    newline(out, depth);
                }
                out.push('}');
            }
        }
    }
}

fn write_string(out: &mut String, s: &str) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => {
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

/// Parse JSON text into a [`JsonValue`].
///
/// # Errors
/// [`StatError::InvalidArgument`] with the byte offset of the first syntax
/// error, for trailing content after the value, or for nesting deeper than 256.
pub fn parse_json(text: &str) -> Result<JsonValue> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let mut p = Parser { bytes: text.as_bytes(), text, pos: 0 };
    p.skip_ws();
    let value = p.value(0)?;
    p.skip_ws();
    if p.pos != p.bytes.len() {
        return Err(p.error("unexpected content after the JSON value"));
    }
    Ok(value)
}

struct Parser<'a> {
    bytes: &'a [u8],
    text: &'a str,
    pos: usize,
}

impl Parser<'_> {
    fn error(&self, what: &str) -> StatError {
        StatError::InvalidArgument { message: format!("invalid JSON at byte {}: {what}", self.pos) }
    }

    fn skip_ws(&mut self) {
        while matches!(self.bytes.get(self.pos), Some(b' ' | b'\t' | b'\n' | b'\r')) {
            self.pos += 1;
        }
    }

    fn expect(&mut self, literal: &str, value: JsonValue) -> Result<JsonValue> {
        if self.text[self.pos..].starts_with(literal) {
            self.pos += literal.len();
            Ok(value)
        } else {
            Err(self.error("unknown literal"))
        }
    }

    fn value(&mut self, depth: usize) -> Result<JsonValue> {
        if depth > MAX_DEPTH {
            return Err(self.error("nesting is too deep"));
        }
        match self.bytes.get(self.pos) {
            None => Err(self.error("unexpected end of input")),
            Some(b'n') => self.expect("null", JsonValue::Null),
            Some(b't') => self.expect("true", JsonValue::Bool(true)),
            Some(b'f') => self.expect("false", JsonValue::Bool(false)),
            Some(b'"') => Ok(JsonValue::String(self.string()?)),
            Some(b'[') => {
                self.pos += 1;
                let mut items = Vec::new();
                self.skip_ws();
                if self.bytes.get(self.pos) == Some(&b']') {
                    self.pos += 1;
                    return Ok(JsonValue::Array(items));
                }
                loop {
                    self.skip_ws();
                    items.push(self.value(depth + 1)?);
                    self.skip_ws();
                    match self.bytes.get(self.pos) {
                        Some(b',') => self.pos += 1,
                        Some(b']') => {
                            self.pos += 1;
                            return Ok(JsonValue::Array(items));
                        }
                        _ => return Err(self.error("expected ',' or ']'")),
                    }
                }
            }
            Some(b'{') => {
                self.pos += 1;
                let mut members = Vec::new();
                self.skip_ws();
                if self.bytes.get(self.pos) == Some(&b'}') {
                    self.pos += 1;
                    return Ok(JsonValue::Object(members));
                }
                loop {
                    self.skip_ws();
                    if self.bytes.get(self.pos) != Some(&b'"') {
                        return Err(self.error("expected a member name"));
                    }
                    let key = self.string()?;
                    self.skip_ws();
                    if self.bytes.get(self.pos) != Some(&b':') {
                        return Err(self.error("expected ':'"));
                    }
                    self.pos += 1;
                    self.skip_ws();
                    members.push((key, self.value(depth + 1)?));
                    self.skip_ws();
                    match self.bytes.get(self.pos) {
                        Some(b',') => self.pos += 1,
                        Some(b'}') => {
                            self.pos += 1;
                            return Ok(JsonValue::Object(members));
                        }
                        _ => return Err(self.error("expected ',' or '}'")),
                    }
                }
            }
            Some(b'-' | b'0'..=b'9') => self.number(),
            Some(_) => Err(self.error("unexpected character")),
        }
    }

    fn number(&mut self) -> Result<JsonValue> {
        let start = self.pos;
        let digits = |p: &mut Self| {
            let s = p.pos;
            while matches!(p.bytes.get(p.pos), Some(b'0'..=b'9')) {
                p.pos += 1;
            }
            p.pos > s
        };
        if self.bytes.get(self.pos) == Some(&b'-') {
            self.pos += 1;
        }
        if self.bytes.get(self.pos) == Some(&b'0') {
            self.pos += 1;
        } else if !digits(self) {
            return Err(self.error("expected digits"));
        }
        if self.bytes.get(self.pos) == Some(&b'.') {
            self.pos += 1;
            if !digits(self) {
                return Err(self.error("expected digits after '.'"));
            }
        }
        if matches!(self.bytes.get(self.pos), Some(b'e' | b'E')) {
            self.pos += 1;
            if matches!(self.bytes.get(self.pos), Some(b'+' | b'-')) {
                self.pos += 1;
            }
            if !digits(self) {
                return Err(self.error("expected exponent digits"));
            }
        }
        let x: f64 = self.text[start..self.pos].parse().map_err(|_| self.error("invalid number"))?;
        if x.is_finite() { Ok(JsonValue::Number(x)) } else { Err(self.error("number out of range")) }
    }

    fn hex4(&mut self) -> Result<u32> {
        let h = self.text.get(self.pos..self.pos + 4).ok_or_else(|| self.error("short \\u escape"))?;
        let v = u32::from_str_radix(h, 16).map_err(|_| self.error("invalid \\u escape"))?;
        self.pos += 4;
        Ok(v)
    }

    fn string(&mut self) -> Result<String> {
        self.pos += 1; // opening quote
        let mut out = String::new();
        loop {
            // Copy the run of ordinary characters in one step.
            let run_start = self.pos;
            while let Some(&b) = self.bytes.get(self.pos) {
                if b == b'"' || b == b'\\' || b < 0x20 {
                    break;
                }
                self.pos += 1;
            }
            out.push_str(&self.text[run_start..self.pos]);
            match self.bytes.get(self.pos) {
                None => return Err(self.error("unterminated string")),
                Some(b'"') => {
                    self.pos += 1;
                    return Ok(out);
                }
                Some(b'\\') => {
                    self.pos += 1;
                    let Some(&e) = self.bytes.get(self.pos) else { return Err(self.error("unterminated escape")) };
                    self.pos += 1;
                    match e {
                        b'"' => out.push('"'),
                        b'\\' => out.push('\\'),
                        b'/' => out.push('/'),
                        b'b' => out.push('\u{8}'),
                        b'f' => out.push('\u{c}'),
                        b'n' => out.push('\n'),
                        b'r' => out.push('\r'),
                        b't' => out.push('\t'),
                        b'u' => {
                            let hi = self.hex4()?;
                            let code = if (0xD800..0xDC00).contains(&hi) {
                                if !self.text[self.pos..].starts_with("\\u") {
                                    return Err(self.error("unpaired surrogate"));
                                }
                                self.pos += 2;
                                let lo = self.hex4()?;
                                if !(0xDC00..0xE000).contains(&lo) {
                                    return Err(self.error("unpaired surrogate"));
                                }
                                0x10000 + ((hi - 0xD800) << 10) + (lo - 0xDC00)
                            } else {
                                hi
                            };
                            out.push(char::from_u32(code).ok_or_else(|| self.error("invalid code point"))?);
                        }
                        _ => return Err(self.error("unknown escape")),
                    }
                }
                Some(_) => return Err(self.error("control character in string")),
            }
        }
    }
}

/// Write `table` as a JSON array of row objects (see the module documentation),
/// one row per line.
#[must_use]
pub fn write_json_table(table: &DataTable) -> String {
    let mut out = String::from("[");
    for r in 0..table.n_rows() {
        let row = JsonValue::Object(
            table
                .columns()
                .iter()
                .map(|c| {
                    let v = match &c.data {
                        ColumnData::Numeric(v) => JsonValue::number(v[r]),
                        ColumnData::Text(v) => JsonValue::String(v[r].clone()),
                        ColumnData::DateTime { values, .. } if values[r].is_nan() => JsonValue::Null,
                        ColumnData::DateTime { values, display } => JsonValue::String(display.iso_text(values[r])),
                    };
                    (c.name.clone(), v)
                })
                .collect(),
        );
        out.push_str(if r == 0 { "\n  " } else { ",\n  " });
        out.push_str(&row.to_compact());
    }
    out.push_str(if table.n_rows() == 0 { "]\n" } else { "\n]\n" });
    out
}

/// Read a table written as a JSON array of row objects.
///
/// # Errors
/// [`StatError::InvalidArgument`] for invalid JSON, a top level that is not an
/// array, or an element that is not an object.
pub fn read_json_table(name: &str, text: &str) -> Result<DataTable> {
    let value = parse_json(text)?;
    let rows = value.as_array().ok_or_else(|| invalid("expected a JSON array of row objects"))?;
    table_from_json_rows(name, rows)
}

/// A table from JSON row objects, typed as [`read_json_table`] types them.
///
/// # Errors
/// [`StatError::InvalidArgument`] for an element that is not an object.
pub fn table_from_json_rows(name: &str, rows: &[JsonValue]) -> Result<DataTable> {
    let mut names: Vec<String> = Vec::new();
    for (i, row) in rows.iter().enumerate() {
        let members = row.as_object().ok_or_else(|| invalid(format!("element {i} is not an object")))?;
        for (k, _) in members {
            if !names.contains(k) {
                names.push(k.clone());
            }
        }
    }
    let columns = names
        .iter()
        .map(|key| {
            // `null` and missing-marker strings such as "NA" are missing in a
            // number or date column; a text column keeps the strings.
            let cells: Vec<Option<&JsonValue>> =
                rows.iter().map(|r| r.get(key).filter(|v| **v != JsonValue::Null)).collect();
            let marker = |v: &JsonValue| v.as_str().is_some_and(is_missing_marker);
            let values: Vec<Option<&JsonValue>> = cells.iter().map(|c| c.filter(|v| !marker(v))).collect();
            let numeric = values.iter().all(|c| c.is_none_or(|v| v.as_f64().is_some()));
            if numeric && values.iter().any(Option::is_some) {
                Column::numeric(
                    key.clone(),
                    values.iter().map(|c| c.and_then(JsonValue::as_f64).unwrap_or(f64::NAN)).collect(),
                )
            } else {
                let texts: Vec<String> = cells
                    .iter()
                    .map(|c| match c {
                        None => String::new(),
                        Some(JsonValue::String(s)) => s.clone(),
                        Some(other) => other.to_compact(),
                    })
                    .collect();
                let all_strings = cells.iter().all(|c| c.is_none_or(|v| matches!(v, JsonValue::String(_))));
                let dates: Vec<&str> =
                    texts.iter().map(|t| if is_missing_marker(t) { "" } else { t.as_str() }).collect();
                match detect_column(&dates).filter(|_| all_strings && values.iter().any(Option::is_some)) {
                    Some(d) => Column::date_time(key.clone(), d.values, d.display),
                    None => Column::text(key.clone(), texts),
                }
            }
        })
        .collect();
    DataTable::new(name, columns)
}

/// An object with nested objects flattened into dotted member names:
/// `{"a": {"b": 1, "c": {"d": 2}}, "e": 3}` becomes
/// `{"a.b": 1, "a.c.d": 2, "e": 3}`. Arrays are kept as values (a table
/// shows them as their JSON text); anything other than an object is
/// returned as it is.
#[must_use]
pub fn flatten_object(value: &JsonValue) -> JsonValue {
    fn walk(prefix: &str, v: &JsonValue, out: &mut Vec<(String, JsonValue)>) {
        match v {
            JsonValue::Object(members) if !members.is_empty() => {
                for (k, x) in members {
                    let key = if prefix.is_empty() { k.clone() } else { format!("{prefix}.{k}") };
                    walk(&key, x, out);
                }
            }
            other => out.push((prefix.to_owned(), other.clone())),
        }
    }
    match value {
        JsonValue::Object(members) => {
            let mut out = Vec::new();
            for (k, x) in members {
                walk(k, x, &mut out);
            }
            JsonValue::Object(out)
        }
        other => other.clone(),
    }
}

fn invalid(message: impl Into<String>) -> StatError {
    StatError::InvalidArgument { message: message.into() }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nested_objects_flatten_with_dotted_names() {
        let v = parse_json(r#"{"a": {"b": 1, "c": {"d": 2}}, "e": [1, 2], "f": {}}"#).unwrap();
        let flat = flatten_object(&v);
        let names: Vec<&str> = flat.as_object().unwrap().iter().map(|(k, _)| k.as_str()).collect();
        assert_eq!(names, ["a.b", "a.c.d", "e", "f"]);
        assert_eq!(flat.get("a.c.d").and_then(JsonValue::as_f64), Some(2.0));
        let t = table_from_json_rows("t", &[flat]).unwrap();
        assert_eq!(t.column(2).unwrap().cell_text(0), "[1,2]");
    }

    #[test]
    fn parses_every_kind_of_value() {
        let v = parse_json(
            r#" {"a": [1, -2.5e3, 0.0], "b": {"c": null, "d": true, "e": false}, "s": "x\"\\\/\n\u00e9\ud83d\ude00"} "#,
        )
        .unwrap();
        assert_eq!(v.get("a").unwrap().as_array().unwrap()[1].as_f64(), Some(-2500.0));
        assert_eq!(v.get("b").unwrap().get("c"), Some(&JsonValue::Null));
        assert_eq!(v.get("b").unwrap().get("d").unwrap().as_bool(), Some(true));
        assert_eq!(v.get("s").unwrap().as_str(), Some("x\"\\/\né😀"));
        // Member order is preserved.
        let keys: Vec<&str> = v.as_object().unwrap().iter().map(|(k, _)| k.as_str()).collect();
        assert_eq!(keys, ["a", "b", "s"]);
    }

    #[test]
    fn rejects_invalid_json() {
        for bad in [
            "",
            "{",
            "[1,]",
            "[1 2]",
            "{\"a\" 1}",
            "{a: 1}",
            "01",
            "1.",
            "-",
            "1e",
            "\"open",
            "\"\\x\"",
            "nul",
            "[1] x",
            "\"\u{1}\"",
            "\"\\ud800\"",
            "1e999",
        ] {
            assert!(parse_json(bad).is_err(), "{bad:?} should fail");
        }
        let deep = "[".repeat(1000) + &"]".repeat(1000);
        assert!(parse_json(&deep).is_err(), "depth limit");
    }

    #[test]
    fn write_then_parse_round_trips() {
        let v = JsonValue::Object(vec![
            (
                "n".into(),
                JsonValue::Array(vec![JsonValue::Number(0.1), JsonValue::Number(-2.5e-300), JsonValue::Number(1e21)]),
            ),
            ("t".into(), JsonValue::String("tab\tquote\" ctrl\u{1} é".into())),
            ("o".into(), JsonValue::Object(vec![])),
            ("a".into(), JsonValue::Array(vec![JsonValue::Array(vec![]), JsonValue::Null])),
        ]);
        assert_eq!(parse_json(&v.to_compact()).unwrap(), v);
        assert_eq!(parse_json(&v.to_pretty()).unwrap(), v);
        assert_eq!(JsonValue::number(f64::NAN), JsonValue::Null);
        assert_eq!(JsonValue::Number(3.0).to_compact(), "3");
        assert_eq!(JsonValue::Number(2.5e-300).to_compact(), "2.5e-300");
    }

    #[test]
    fn table_round_trip() {
        let t = DataTable::new(
            "t",
            vec![
                Column::numeric("x", vec![1.5, f64::NAN, 1e-7]),
                Column::text("group \"g\"", vec!["a".into(), String::new(), "c\nd".into()]),
            ],
        )
        .unwrap();
        let text = write_json_table(&t);
        let back = read_json_table("t", &text).unwrap();
        assert_eq!(back.n_rows(), 3);
        let (a, b) = (t.column(0).unwrap().as_numeric().unwrap(), back.column(0).unwrap().as_numeric().unwrap());
        assert_eq!(a[0], b[0]);
        assert!(b[1].is_nan());
        assert_eq!(a[2].to_bits(), b[2].to_bits());
        assert_eq!(back.column(1).unwrap().data, t.column(1).unwrap().data);
        let empty = DataTable::new("e", vec![]).unwrap();
        assert_eq!(write_json_table(&empty), "[]\n");
    }

    #[test]
    fn reads_ragged_and_mixed_rows() {
        let t = read_json_table("t", r#"[{"a": 1, "b": "x"}, {"c": true, "a": null}, {"b": 2}]"#).unwrap();
        assert_eq!(t.columns().iter().map(|c| c.name.as_str()).collect::<Vec<_>>(), ["a", "b", "c"]);
        let a = t.column(0).unwrap().as_numeric().unwrap();
        assert_eq!(a[0], 1.0);
        assert!(a[1].is_nan() && a[2].is_nan());
        assert_eq!(t.column(1).unwrap().data, ColumnData::Text(vec!["x".into(), String::new(), "2".into()]));
        assert_eq!(t.column(2).unwrap().data, ColumnData::Text(vec![String::new(), "true".into(), String::new()]));
        assert!(read_json_table("t", "{}").is_err());
        assert!(read_json_table("t", "[1]").is_err());
    }

    #[test]
    fn missing_marker_strings_are_missing_in_number_columns() {
        let t =
            read_json_table("j", r#"[{"x": 1, "c": "NA"}, {"x": "NA", "c": "US"}, {"x": null, "c": "NaN"}]"#).unwrap();
        let x = t.column(0).unwrap().as_numeric().unwrap();
        assert_eq!(x[0], 1.0);
        assert!(x[1].is_nan() && x[2].is_nan());
        assert_eq!(t.column(1).unwrap().cell_text(0), "NA");
    }

    #[test]
    fn date_columns_round_trip() {
        let t =
            read_json_table("t", r#"[{"d": "2026-10-13", "n": "2026"}, {"d": null, "n": "x"}, {"d": "13 Oct 2026"}]"#)
                .unwrap();
        let d = t.column(0).unwrap();
        assert_eq!(d.temporal().unwrap().format, crate::temporal::TemporalFormat::Date);
        assert_eq!(
            (d.cell_text(0), d.cell_text(1), d.cell_text(2)),
            ("2026-10-13".into(), String::new(), "2026-10-13".into())
        );
        assert!(t.column(1).unwrap().temporal().is_none());
        let written = write_json_table(&t);
        assert!(written.contains(r#"{"d":"2026-10-13","n":"2026"}"#) && written.contains(r#""d":null"#), "{written}");
        assert_eq!(read_json_table("t", &written).unwrap(), t);
        // Numbers are never dates, even as JSON numbers that look like years.
        let t = read_json_table("t", r#"[{"y": 2026}]"#).unwrap();
        assert!(t.column(0).unwrap().temporal().is_none());
    }
}
