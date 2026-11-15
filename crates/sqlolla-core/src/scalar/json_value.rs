//! Canonical JSONB duplicate-key, numeric and container ordering semantics.
use super::Decimal;
use crate::json::error;
use crate::Result;
use serde_json::Value;
use std::cmp::Ordering;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum JsonValue {
    Null,
    String(String),
    Number(Decimal),
    Bool(bool),
    Array(Vec<JsonValue>),
    Object(Vec<(String, JsonValue)>),
}
impl JsonValue {
    /// Parse JSON without binary-float rounding and retain only each key's last value.
    pub fn parse(value: &str) -> Result<Self> {
        let value: Value =
            serde_json::from_str(value).map_err(|_| error("sql.22P02", "Invalid JSON input"))?;
        Self::from_json(value)
    }
    /// Build comparison structure; JSONB object keys sort by byte length then bytes.
    fn from_json(value: Value) -> Result<Self> {
        Ok(match value {
            Value::Null => Self::Null,
            Value::Bool(value) => Self::Bool(value),
            Value::Number(value) => Self::Number(Decimal::parse(&value.to_string())?),
            Value::String(value) if value.contains('\0') => {
                return Err(error("sql.22P05", "JSONB cannot contain Unicode NUL"));
            }
            Value::String(value) => Self::String(value),
            Value::Array(values) => Self::Array(
                values
                    .into_iter()
                    .map(Self::from_json)
                    .collect::<Result<_>>()?,
            ),
            Value::Object(values) => {
                let mut values = values
                    .into_iter()
                    .map(|(key, value)| Ok((key, Self::from_json(value)?)))
                    .collect::<Result<Vec<_>>>()?;
                if values.iter().any(|(key, _)| key.contains('\0')) {
                    return Err(error("sql.22P05", "JSONB key contains Unicode NUL"));
                }
                values.sort_by(|left, right| {
                    left.0.len().cmp(&right.0.len()).then(left.0.cmp(&right.0))
                });
                Self::Object(values)
            }
        })
    }
    /// Render JSONB's deterministic key ordering with lossless decimal values.
    pub fn text(&self) -> String {
        match self {
            Self::Null => "null".into(),
            Self::Bool(value) => value.to_string(),
            Self::Number(value) => value.to_string(),
            Self::String(value) => serde_json::to_string(value).unwrap_or_default(),
            Self::Array(values) => {
                "[".to_owned() + &values.iter().map(Self::text).collect::<Vec<_>>().join(", ") + "]"
            }
            Self::Object(values) => {
                "{".to_owned()
                    + &values
                        .iter()
                        .map(|(key, value)| {
                            serde_json::to_string(key).unwrap_or_default() + ": " + &value.text()
                        })
                        .collect::<Vec<_>>()
                        .join(", ")
                    + "}"
            }
        }
    }
    /// PostgreSQL JSONB type precedence in B-tree comparisons.
    fn rank(&self) -> u8 {
        match self {
            Self::Array(values) if values.is_empty() => 0,
            Self::Null => 1,
            Self::String(_) => 2,
            Self::Number(_) => 3,
            Self::Bool(_) => 4,
            Self::Array(_) => 5,
            Self::Object(_) => 6,
        }
    }
}
impl PartialOrd for JsonValue {
    /// Use the canonical JSONB total order.
    fn partial_cmp(&self, right: &Self) -> Option<Ordering> {
        Some(self.cmp(right))
    }
}
impl Ord for JsonValue {
    /// Containers compare cardinality before their storage-order contents.
    fn cmp(&self, right: &Self) -> Ordering {
        match (self, right) {
            (Self::Null, Self::Null) => Ordering::Equal,
            (Self::String(left), Self::String(right)) => left.cmp(right),
            (Self::Number(left), Self::Number(right)) => left.cmp(right),
            (Self::Bool(left), Self::Bool(right)) => left.cmp(right),
            (Self::Array(left), Self::Array(right)) => {
                left.len().cmp(&right.len()).then(left.cmp(right))
            }
            (Self::Object(left), Self::Object(right)) => {
                left.len().cmp(&right.len()).then(left.cmp(right))
            }
            _ => self.rank().cmp(&right.rank()),
        }
    }
}
