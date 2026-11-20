//! Helpers that read command params from JSON. Params come from users, scripts and the network,
//! so every number is range-checked and every string length-capped before a command sees it.

use serde_json::Value;

use crate::CommandError;

/// Longest string param we accept unless a command asks for a different cap.
pub const MAX_STRING: usize = 4096;

fn field<'a>(p: &'a Value, key: &str) -> Option<&'a Value> {
    match p {
        Value::Object(m) => m.get(key).filter(|v| !v.is_null()),
        _ => None,
    }
}

fn invalid(msg: String) -> CommandError {
    CommandError::InvalidParams(msg)
}

/// Whether the field is present and not null.
pub fn has(p: &Value, key: &str) -> bool {
    field(p, key).is_some()
}

/// A required whole number in `min..=max`.
pub fn i64_in(p: &Value, key: &str, min: i64, max: i64) -> Result<i64, CommandError> {
    opt_i64_in(p, key, min, max)?.ok_or_else(|| invalid(format!("`{key}` is required")))
}

/// An optional whole number in `min..=max`.
pub fn opt_i64_in(p: &Value, key: &str, min: i64, max: i64) -> Result<Option<i64>, CommandError> {
    let Some(v) = field(p, key) else { return Ok(None) };
    let n = v
        .as_i64()
        .or_else(|| v.as_f64().filter(|f| f.fract() == 0.0 && f.abs() < 9.0e15).map(|f| f as i64))
        .ok_or_else(|| invalid(format!("`{key}` must be a whole number")))?;
    if n < min || n > max {
        return Err(invalid(format!("`{key}` must be between {min} and {max}")));
    }
    Ok(Some(n))
}

/// A required index (`usize`) below `len`.
pub fn index(p: &Value, key: &str, len: usize) -> Result<usize, CommandError> {
    if len == 0 {
        return Err(invalid(format!("`{key}`: nothing to choose from")));
    }
    let max = i64::try_from(len - 1).unwrap_or(i64::MAX);
    let n = i64_in(p, key, 0, max)?;
    usize::try_from(n).map_err(|_| invalid(format!("`{key}` out of range")))
}

/// A required finite number in `min..=max`.
pub fn f64_in(p: &Value, key: &str, min: f64, max: f64) -> Result<f64, CommandError> {
    opt_f64_in(p, key, min, max)?.ok_or_else(|| invalid(format!("`{key}` is required")))
}

/// An optional finite number in `min..=max`.
pub fn opt_f64_in(p: &Value, key: &str, min: f64, max: f64) -> Result<Option<f64>, CommandError> {
    let Some(v) = field(p, key) else { return Ok(None) };
    let n = v.as_f64().ok_or_else(|| invalid(format!("`{key}` must be a number")))?;
    if !n.is_finite() || n < min || n > max {
        return Err(invalid(format!("`{key}` must be between {min} and {max}")));
    }
    Ok(Some(n))
}

/// A required bool.
pub fn bool(p: &Value, key: &str) -> Result<bool, CommandError> {
    opt_bool(p, key)?.ok_or_else(|| invalid(format!("`{key}` is required")))
}

/// An optional bool.
pub fn opt_bool(p: &Value, key: &str) -> Result<Option<bool>, CommandError> {
    let Some(v) = field(p, key) else { return Ok(None) };
    v.as_bool().map(Some).ok_or_else(|| invalid(format!("`{key}` must be true or false")))
}

/// A required string of at most `max` bytes.
pub fn str_max<'a>(p: &'a Value, key: &str, max: usize) -> Result<&'a str, CommandError> {
    opt_str_max(p, key, max)?.ok_or_else(|| invalid(format!("`{key}` is required")))
}

/// A required string of at most [`MAX_STRING`] bytes.
pub fn str<'a>(p: &'a Value, key: &str) -> Result<&'a str, CommandError> {
    str_max(p, key, MAX_STRING)
}

/// An optional string of at most `max` bytes.
pub fn opt_str_max<'a>(p: &'a Value, key: &str, max: usize) -> Result<Option<&'a str>, CommandError> {
    let Some(v) = field(p, key) else { return Ok(None) };
    let s = v.as_str().ok_or_else(|| invalid(format!("`{key}` must be text")))?;
    if s.len() > max {
        return Err(invalid(format!("`{key}` is longer than {max} bytes")));
    }
    Ok(Some(s))
}

/// An optional string of at most [`MAX_STRING`] bytes.
pub fn opt_str<'a>(p: &'a Value, key: &str) -> Result<Option<&'a str>, CommandError> {
    opt_str_max(p, key, MAX_STRING)
}

/// A required list of strings, at most `max_items` items of at most `max_len` bytes each.
pub fn str_list(p: &Value, key: &str, max_items: usize, max_len: usize) -> Result<Vec<String>, CommandError> {
    let Some(v) = field(p, key) else { return Err(invalid(format!("`{key}` is required"))) };
    let arr = v.as_array().ok_or_else(|| invalid(format!("`{key}` must be a list")))?;
    if arr.len() > max_items {
        return Err(invalid(format!("`{key}` has more than {max_items} items")));
    }
    arr.iter()
        .map(|item| {
            let s = item.as_str().ok_or_else(|| invalid(format!("`{key}` items must be text")))?;
            if s.len() > max_len {
                return Err(invalid(format!("`{key}` item longer than {max_len} bytes")));
            }
            Ok(s.to_owned())
        })
        .collect()
}

/// A required list of indexes, each below `len`, at most `max_items` of them.
pub fn index_list(p: &Value, key: &str, len: usize, max_items: usize) -> Result<Vec<usize>, CommandError> {
    let Some(v) = field(p, key) else { return Err(invalid(format!("`{key}` is required"))) };
    let arr = v.as_array().ok_or_else(|| invalid(format!("`{key}` must be a list")))?;
    if arr.len() > max_items {
        return Err(invalid(format!("`{key}` has more than {max_items} items")));
    }
    arr.iter()
        .map(|item| {
            item.as_u64()
                .and_then(|n| usize::try_from(n).ok())
                .filter(|&n| n < len)
                .ok_or_else(|| invalid(format!("`{key}` items must be indexes below {len}")))
        })
        .collect()
}
