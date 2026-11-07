//! Lenient deserializers. PHP endpoints are not always consistent about `"1"` vs `1`,
//! so ids and flags accept either form instead of failing the whole response.

use serde::{Deserialize, Deserializer, de::Error};

#[derive(Deserialize)]
#[serde(untagged)]
enum StrOrNum {
    Str(String),
    Int(i64),
    Float(f64),
    Bool(bool),
}

pub fn string<'de, D: Deserializer<'de>>(d: D) -> Result<String, D::Error> {
    Ok(match StrOrNum::deserialize(d)? {
        StrOrNum::Str(s) => s,
        StrOrNum::Int(n) => n.to_string(),
        StrOrNum::Float(n) => n.to_string(),
        StrOrNum::Bool(b) => b.to_string(),
    })
}

pub fn int<'de, D: Deserializer<'de>>(d: D) -> Result<i64, D::Error> {
    match StrOrNum::deserialize(d)? {
        StrOrNum::Int(n) => Ok(n),
        StrOrNum::Float(n) if n.fract() == 0.0 => Ok(n as i64),
        StrOrNum::Bool(b) => Ok(b as i64),
        StrOrNum::Str(s) => s.trim().parse().map_err(D::Error::custom),
        StrOrNum::Float(n) => Err(D::Error::custom(format!("expected an integer, got {n}"))),
    }
}

/// `true`/`false`, `1`/`0` or `"1"`/`"0"`.
pub fn boolean<'de, D: Deserializer<'de>>(d: D) -> Result<bool, D::Error> {
    match StrOrNum::deserialize(d)? {
        StrOrNum::Bool(b) => Ok(b),
        StrOrNum::Int(n) => Ok(n != 0),
        StrOrNum::Float(n) => Ok(n != 0.0),
        StrOrNum::Str(s) => match s.trim() {
            "1" | "true" => Ok(true),
            "0" | "false" | "" => Ok(false),
            other => Err(D::Error::custom(format!(
                "expected a boolean, got {other:?}"
            ))),
        },
    }
}
