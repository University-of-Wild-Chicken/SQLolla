//! Validated transport values with semantic keys separate from display spelling.
use super::{arrays::ArrayValue, json_value::JsonValue, temporal::Interval, Decimal, PgFloat};
use crate::{Result, SqlType};
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;

/// PostgreSQL text transport plus its declared type; constructed only by validation.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(try_from = "Wire", into = "Wire")]
pub struct ScalarValue {
    pub(crate) ty: SqlType,
    pub(crate) value: String,
    pub(crate) key: Key,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Key {
    Integer(i64),
    Numeric(Decimal),
    Float(PgFloat),
    Text(String),
    Bytes(Vec<u8>),
    Date(i64),
    Timestamp(i64),
    Timestamptz(i64),
    Interval(Interval),
    Enum(String, usize),
    Array(ArrayValue),
    Json(JsonValue),
    JsonText(String),
    Void,
    Bits(Vec<bool>),
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Wire {
    ty: SqlType,
    value: String,
}

impl ScalarValue {
    /// Parse and validate one rich transport scalar without evaluating SQL.
    pub fn new(ty: SqlType, value: &str) -> Result<Self> {
        super::input::rich(&ty, value)
    }
    /// Return exact declared type identity and modifiers.
    pub fn sql_type(&self) -> &SqlType {
        &self.ty
    }
    /// Return PostgreSQL-compatible canonical text transport.
    pub fn text(&self) -> &str {
        &self.value
    }
}
impl TryFrom<Wire> for ScalarValue {
    type Error = crate::Error;
    /// Reject malformed/forged canonical values at the deserialization boundary.
    fn try_from(wire: Wire) -> Result<Self> {
        Self::new(wire.ty, &wire.value)
    }
}
impl From<ScalarValue> for Wire {
    /// Serialize only portable type and spelling, never unchecked internal keys.
    fn from(value: ScalarValue) -> Self {
        Self {
            ty: value.ty,
            value: value.value,
        }
    }
}
impl PartialEq for ScalarValue {
    /// Transport identity retains type modifiers and observable display spelling.
    fn eq(&self, right: &Self) -> bool {
        self.cmp(right) == Ordering::Equal
    }
}
impl Eq for ScalarValue {}
impl PartialOrd for ScalarValue {
    /// Provide stable collection ordering; SQL comparison adds type compatibility checks.
    fn partial_cmp(&self, right: &Self) -> Option<Ordering> {
        Some(self.cmp(right))
    }
}
impl Ord for ScalarValue {
    /// Collection ordering is typed transport identity, separate from SQL equality.
    fn cmp(&self, right: &Self) -> Ordering {
        self.ty
            .cmp(&right.ty)
            .then(self.key.cmp(&right.key))
            .then(self.value.cmp(&right.value))
    }
}
