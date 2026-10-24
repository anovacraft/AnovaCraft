//! Conversions between script values and the application's data: JSON
//! request documents, table cells and report tables.
//!
//! | Script value | JSON | Table cell |
//! |---|---|---|
//! | `()` | `null` | missing value |
//! | `true` / `false` | `true` / `false` | 1 / 0 |
//! | integer, float | number | number |
//! | string | string | text |
//! | array | array | — |
//! | object map `#{…}` | object | — |

use rhai::{Array, Dynamic, EvalAltResult, FLOAT, INT, Map};
use stat_data::{CellValue, Column, ColumnData, JsonValue};

/// Result type of registered script functions.
pub(crate) type ScriptResult<T> = Result<T, Box<EvalAltResult>>;

/// A runtime error carrying `message`, reported at the calling line.
pub(crate) fn fail<T>(message: impl Into<String>) -> ScriptResult<T> {
    Err(message.into().into())
}

/// Convert an application error into a script error.
pub(crate) fn script_err(e: impl std::fmt::Display) -> Box<EvalAltResult> {
    e.to_string().into()
}

/// A script value as JSON (object members in key order).
///
/// # Errors
/// For values with no JSON form (functions, tables, reports).
pub(crate) fn to_json(value: &Dynamic) -> ScriptResult<JsonValue> {
    if value.is_unit() {
        return Ok(JsonValue::Null);
    }
    if let Ok(b) = value.as_bool() {
        return Ok(JsonValue::Bool(b));
    }
    if let Ok(i) = value.as_int() {
        return Ok(JsonValue::Number(i as f64));
    }
    if let Ok(x) = value.as_float() {
        return Ok(JsonValue::Number(x));
    }
    if let Ok(c) = value.as_char() {
        return Ok(JsonValue::String(c.to_string()));
    }
    if value.is_string() {
        return Ok(JsonValue::String(value.clone().into_string().unwrap_or_default()));
    }
    if value.is_array() {
        let a = value.read_lock::<Array>().expect("an array");
        return a.iter().map(to_json).collect::<ScriptResult<_>>().map(JsonValue::Array);
    }
    if value.is_map() {
        let m = value.read_lock::<Map>().expect("a map");
        return m
            .iter()
            .map(|(k, v)| Ok((k.to_string(), to_json(v)?)))
            .collect::<ScriptResult<_>>()
            .map(JsonValue::Object);
    }
    fail(format!("a {} cannot be used as a setting", value.type_name()))
}

/// JSON as a script value.
pub(crate) fn from_json(value: &JsonValue) -> Dynamic {
    match value {
        JsonValue::Null => Dynamic::UNIT,
        JsonValue::Bool(b) => (*b).into(),
        JsonValue::Number(x) => number(*x),
        JsonValue::String(s) => s.clone().into(),
        JsonValue::Array(a) => Dynamic::from_array(a.iter().map(from_json).collect()),
        JsonValue::Object(m) => {
            Dynamic::from_map(m.iter().map(|(k, v)| (k.as_str().into(), from_json(v))).collect::<Map>())
        }
    }
}

/// A number as a script value: whole numbers of moderate size become
/// integers (so they work as counts and indices), others floats.
pub(crate) fn number(x: f64) -> Dynamic {
    if x.fract() == 0.0 && x.abs() < 9.0e15 { (x as INT).into() } else { (x as FLOAT).into() }
}

/// A column's value at `row` (missing values are `()`).
pub(crate) fn cell(column: &Column, row: usize) -> Dynamic {
    match column.value(row) {
        CellValue::Number(x) if x.is_nan() => Dynamic::UNIT,
        CellValue::Number(x) => (x as FLOAT).into(),
        CellValue::Text(s) if s.is_empty() => Dynamic::UNIT,
        CellValue::Text(s) => s.into(),
    }
}

/// Every value of a column, as an array.
pub(crate) fn column_values(column: &Column) -> Array {
    (0..column.data.len()).map(|r| cell(column, r)).collect()
}

/// The numeric value of a script value for a numeric cell; `None` if it is
/// not a number, a boolean or `()`.
pub(crate) fn as_number(value: &Dynamic) -> Option<f64> {
    if value.is_unit() {
        Some(f64::NAN)
    } else if let Ok(x) = value.as_float() {
        Some(x)
    } else if let Ok(i) = value.as_int() {
        Some(i as f64)
    } else {
        value.as_bool().ok().map(|b| if b { 1.0 } else { 0.0 })
    }
}

/// Text of a script value for a text cell (`()` is the empty, missing value).
pub(crate) fn as_text(value: &Dynamic) -> String {
    if value.is_unit() { String::new() } else { value.to_string() }
}

/// A column named `name` from script values: numeric if every value is a
/// number, a boolean or `()`, otherwise text.
pub(crate) fn column_from_values(name: &str, values: &Array) -> Column {
    let numbers: Option<Vec<f64>> = values.iter().map(as_number).collect();
    match numbers {
        Some(v) => Column::numeric(name, v),
        None => Column::text(name, values.iter().map(as_text).collect()),
    }
}

/// A cell value for `column` from a script value.
///
/// # Errors
/// A non-numeric value for a numeric or date-time column.
pub(crate) fn cell_value(column: &Column, value: &Dynamic) -> ScriptResult<CellValue> {
    match &column.data {
        ColumnData::Text(_) => Ok(CellValue::Text(as_text(value))),
        ColumnData::Numeric(_) | ColumnData::DateTime { .. } => match as_number(value) {
            Some(x) => Ok(CellValue::Number(x)),
            None if value.is_string() => {
                let text = value.clone().into_string().unwrap_or_default();
                column.parse_value(&text).map_err(script_err)
            }
            None => fail(format!("column {:?} holds numbers; got a {}", column.name, value.type_name())),
        },
    }
}

/// A formatted report cell as a script value: an integer or float when the
/// whole cell reads as one, `()` when empty, otherwise the text.
pub(crate) fn report_cell(text: &str) -> Dynamic {
    let t = text.trim();
    if t.is_empty() {
        return Dynamic::UNIT;
    }
    if let Ok(i) = t.parse::<INT>() {
        return i.into();
    }
    match t.parse::<f64>() {
        Ok(x) if x.is_finite() => (x as FLOAT).into(),
        _ => text.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_round_trips_through_script_values() {
        let text = r#"{"a": [1, 2.5, null, true, "x"], "b": {"c": -3}}"#;
        let json = stat_data::parse_json(text).unwrap();
        let back = to_json(&from_json(&json)).unwrap();
        assert_eq!(back.to_compact(), json.to_compact());
    }

    #[test]
    fn columns_are_numeric_unless_a_value_is_text() {
        let nums: Array = vec![1_i64.into(), 2.5_f64.into(), Dynamic::UNIT, true.into()];
        let c = column_from_values("x", &nums);
        let v = c.as_numeric().unwrap();
        assert_eq!(&v[..2], &[1.0, 2.5]);
        assert!(v[2].is_nan());
        assert_eq!(v[3], 1.0);
        let mixed: Array = vec![1_i64.into(), "b".into(), Dynamic::UNIT];
        let c = column_from_values("y", &mixed);
        assert!(c.as_numeric().is_none());
        assert_eq!(column_values(&c)[0].to_string(), "1");
        assert!(column_values(&c)[2].is_unit());
    }

    #[test]
    fn report_cells_become_numbers_where_they_read_as_numbers() {
        assert_eq!(report_cell(" 1.5e-3 ").as_float().unwrap(), 1.5e-3);
        assert_eq!(report_cell("17").as_int().unwrap(), 17);
        assert_eq!(report_cell("<.0001").into_string().unwrap(), "<.0001");
        assert!(report_cell("").is_unit());
    }
}
