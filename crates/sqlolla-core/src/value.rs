//! Canonical scalar families and typed cell values.
use serde::{Deserialize, Serialize};

/// Maximum admitted UTF-8 byte length of a primitive text value.
pub const MAX_TEXT_BYTES: usize = 1_048_576;

/// Supported PostgreSQL scalar families; text uses exact deterministic equality.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SqlType {
    /// PostgreSQL signed 32-bit integer.
    Int32,
    /// PostgreSQL signed 64-bit integer, including COUNT result values.
    Int64,
    /// Boolean scalar, with UNKNOWN represented by a typed NULL.
    Bool,
    /// Text under the admitted exact-equality comparison profile.
    Text,
    /// Signed PostgreSQL smallint.
    Int16,
    /// IEEE binary64 with PostgreSQL NaN and signed-zero comparison rules.
    Float64,
    /// Arbitrary precision decimal, optionally constrained by a declared typmod.
    Numeric {
        /// Maximum significant decimal digits.
        precision: Option<u16>,
        /// Declared fractional scale; negative scales round to powers of ten.
        scale: Option<i16>,
    },
    /// Variable-length character data with an optional character count bound.
    Varchar {
        /// Declared maximum Unicode character count.
        length: Option<u32>,
    },
    /// Blank-padded fixed-width character data.
    Char {
        /// Declared Unicode character count.
        length: u32,
    },
    /// Blank-padded character comparison/input family without a storage typmod.
    Bpchar,
    /// PostgreSQL catalog name, clipped at 63 UTF-8 bytes on input.
    Name,
    /// PostgreSQL unsigned 32-bit catalog object identifier.
    Oid,
    /// Nominal unsigned 32-bit transaction identity; SQL equality does not imply ordering.
    Xid,
    /// Nominal relation reference with canonical numeric OID transport; display needs catalog context.
    Regclass,
    /// PostgreSQL internal single-byte catalog char, distinct from SQL character(n).
    InternalChar,
    /// Binary octets, independent of text encoding.
    Bytea,
    /// Gregorian date with explicit infinite endpoints.
    Date,
    /// Local timestamp under the deterministic ISO input profile.
    Timestamp,
    /// UTC timestamp; input offsets are resolved before comparison.
    Timestamptz,
    /// Calendar months, days and microseconds remain distinct components.
    Interval,
    /// Canonical PostgreSQL-style binary JSON value semantics.
    Jsonb,
    /// Validated JSON source text retaining whitespace, key order and duplicate keys.
    Json,
    /// PostgreSQL void function result; non-NULL empty transport is distinct from SQL NULL.
    Void,
    /// Nominal enum identity and its declaration order.
    Enum {
        /// Schema-qualified enum identity.
        name: String,
        /// Declaration-order labels.
        labels: Vec<String>,
    },
    /// Typed rectangular array; values retain dimensions, lower bounds and NULL types.
    Array(Box<SqlType>),
    /// Fixed-width bit string.
    Bit {
        /// Number of stored bits.
        length: u32,
    },
}

/// A typed value. NULL retains its resolved SQL type.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(
    tag = "type",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum Cell {
    /// Missing value with its contextually resolved type.
    Null(SqlType),
    /// A signed 32-bit integer.
    Int32(i32),
    /// A signed 64-bit integer.
    Int64(i64),
    /// A non-NULL Boolean.
    Bool(bool),
    /// Exact text content without collation-based ordering.
    Text(#[serde(deserialize_with = "deserialize_text")] String),
    /// Validated richer value with canonical equality and ordering.
    Scalar(crate::scalar::ScalarValue),
}

impl Cell {
    /// Reject impossible supplied text before it can become a modeled database value.
    pub fn validate(&self) -> crate::Result<()> {
        if let Self::Text(value) = self {
            validate_text(value)?;
        }
        Ok(())
    }
    /// Return the resolved SQL scalar family, including for NULL.
    pub fn sql_type(&self) -> SqlType {
        match self {
            Self::Null(ty) => ty.clone(),
            Self::Int32(_) => SqlType::Int32,
            Self::Int64(_) => SqlType::Int64,
            Self::Bool(_) => SqlType::Bool,
            Self::Text(_) => SqlType::Text,
            Self::Scalar(value) => value.sql_type().clone(),
        }
    }
    /// Identify NULL without confusing it with false or an empty string.
    pub fn is_null(&self) -> bool {
        matches!(self, Self::Null(_))
    }
}

/// Primitive text shares the same representability gate across all transport and execution paths.
pub(crate) fn validate_text(value: &str) -> crate::Result<()> {
    if value.contains('\0') {
        return Err(crate::json::error(
            "invalid.text_nul",
            "PostgreSQL text cannot contain NUL",
        ));
    }
    if value.len() > MAX_TEXT_BYTES {
        return Err(crate::json::error(
            "resource.scalar_bytes",
            "Text exceeds byte budget",
        ));
    }
    Ok(())
}

/// JSON text cells cannot bypass the same gate used by direct Rust callers.
fn deserialize_text<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> std::result::Result<String, D::Error> {
    let value = String::deserialize(deserializer)?;
    validate_text(&value).map_err(serde::de::Error::custom)?;
    Ok(value)
}
