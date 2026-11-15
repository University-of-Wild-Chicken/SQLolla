//! Common-type and immutable-function result inference without value-dependent guesses.
use crate::json::error;
use crate::{Result, SqlType};

/// Determine whether both scalar families have an admitted common SQL type.
pub fn compatible(left: &SqlType, right: &SqlType) -> bool {
    common_type(left, right).is_ok()
}

/// Compute deterministic common types, preserving nominal enums and array elements.
pub fn common_type(left: &SqlType, right: &SqlType) -> Result<SqlType> {
    if left == right {
        return Ok(left.clone());
    }
    if numeric(left) && numeric(right) {
        return Ok(numeric_common(left, right));
    }
    if character(left) && character(right) {
        return match (left, right) {
            (SqlType::Char { length: left }, SqlType::Char { length: right }) => {
                Ok(SqlType::Char {
                    length: (*left).max(*right),
                })
            }
            _ => Ok(SqlType::Text),
        };
    }
    match (left, right) {
        (SqlType::Oid, SqlType::Regclass) | (SqlType::Regclass, SqlType::Oid) => Ok(SqlType::Oid),
        (SqlType::Array(left), SqlType::Array(right)) => {
            Ok(SqlType::Array(Box::new(common_type(left, right)?)))
        }
        (SqlType::Date, SqlType::Timestamp) | (SqlType::Timestamp, SqlType::Date) => {
            Ok(SqlType::Timestamp)
        }
        (SqlType::Date | SqlType::Timestamp, SqlType::Timestamptz)
        | (SqlType::Timestamptz, SqlType::Date | SqlType::Timestamp) => Ok(SqlType::Timestamptz),
        _ => Err(error("sql.42804", "Incompatible SQL scalar types")),
    }
}

/// Promote numeric families without inheriting storage modifiers into arithmetic.
fn numeric_common(left: &SqlType, right: &SqlType) -> SqlType {
    if *left == SqlType::Float64 || *right == SqlType::Float64 {
        return SqlType::Float64;
    }
    if matches!(left, SqlType::Numeric { .. }) || matches!(right, SqlType::Numeric { .. }) {
        return SqlType::Numeric {
            precision: None,
            scale: None,
        };
    }
    if *left == SqlType::Int64 || *right == SqlType::Int64 {
        SqlType::Int64
    } else {
        SqlType::Int32
    }
}

/// Identify exact/approximate numeric families without conflating Boolean values.
pub(crate) fn numeric(ty: &SqlType) -> bool {
    matches!(
        ty,
        SqlType::Int16
            | SqlType::Int32
            | SqlType::Int64
            | SqlType::Float64
            | SqlType::Numeric { .. }
    )
}
/// Identify character families under the explicit C-collation profile.
pub(crate) fn character(ty: &SqlType) -> bool {
    matches!(
        ty,
        SqlType::Text
            | SqlType::Char { .. }
            | SqlType::Bpchar
            | SqlType::Name
            | SqlType::Varchar { .. }
    )
}

/// Comparison operator resolution discards storage typmods without truncating literals.
pub fn comparison_type(left: &SqlType, right: &SqlType) -> Result<SqlType> {
    if matches!(left, SqlType::Json | SqlType::Void)
        || matches!(right, SqlType::Json | SqlType::Void)
    {
        return Err(error("sql.42883", "SQL type has no comparison operator"));
    }
    if character(left) && character(right) && (*left == SqlType::Text || *right == SqlType::Text) {
        return Ok(SqlType::Text);
    }
    if character(left)
        && character(right)
        && (matches!(left, SqlType::Char { .. } | SqlType::Bpchar)
            || matches!(right, SqlType::Char { .. } | SqlType::Bpchar))
    {
        return Ok(SqlType::Bpchar);
    }
    let ty = common_type(left, right)?;
    Ok(match ty {
        SqlType::Numeric { .. } => SqlType::Numeric {
            precision: None,
            scale: None,
        },
        SqlType::Varchar { .. } => SqlType::Varchar { length: None },
        ty => ty,
    })
}

/// Infer arithmetic results, including calendar date plus interval promotion.
pub fn operation_type(op: &str, left: &SqlType, right: &SqlType) -> Result<SqlType> {
    if matches!(op, "concat" | "||") && *left == SqlType::Jsonb && *right == SqlType::Jsonb {
        return Ok(SqlType::Jsonb);
    }
    if matches!(op, "concat" | "||")
        && (character(left) || character(right))
        && !matches!(left, SqlType::Array(_))
        && !matches!(right, SqlType::Array(_))
    {
        return Ok(SqlType::Text);
    }
    if numeric(left) && numeric(right) {
        let ty = common_type(left, right)?;
        return Ok(if matches!(ty, SqlType::Numeric { .. }) {
            SqlType::Numeric {
                precision: None,
                scale: None,
            }
        } else {
            ty
        });
    }
    calendar_operation_type(op, left, right)
}

/// Resolve the admitted temporal operator signatures independently of numeric types.
fn calendar_operation_type(op: &str, left: &SqlType, right: &SqlType) -> Result<SqlType> {
    if matches!(op, "add" | "+" | "subtract" | "-") && *right == SqlType::Interval {
        return match left {
            SqlType::Date | SqlType::Timestamp => Ok(SqlType::Timestamp),
            SqlType::Timestamptz => Ok(SqlType::Timestamptz),
            SqlType::Interval => Ok(SqlType::Interval),
            _ => Err(error("sql.42883", "Operator type mismatch")),
        };
    }
    if matches!(op, "add" | "+") && *left == SqlType::Interval {
        return operation_type(op, right, left);
    }
    if matches!(op, "subtract" | "-")
        && matches!(
            (left, right),
            (SqlType::Timestamp, SqlType::Timestamp) | (SqlType::Timestamptz, SqlType::Timestamptz)
        )
    {
        return Ok(SqlType::Interval);
    }
    if matches!(op, "subtract" | "-") && *left == SqlType::Date && *right == SqlType::Date {
        return Ok(SqlType::Int32);
    }
    Err(error(
        "sql.42883",
        "No admitted arithmetic operator for these SQL types",
    ))
}

/// Infer immutable builtin return types; session/effect functions require another interface.
pub fn return_type(name: &str, args: &[SqlType]) -> Result<SqlType> {
    match name.to_ascii_lowercase().as_str() {
        "string_to_array" | "array_to_string" | "jsonb_typeof" => {
            super::string_collections::signature(name, args)
        }
        "array_append" | "array_prepend" | "array_length" | "cardinality" | "jsonb_set" => {
            array_signature(name, args)
        }
        "jsonb_exists" | "jsonb_exists_any" | "jsonb_exists_all" | "get_bit" => {
            collection_signature(name, args)
        }
        "make_interval" => super::interval::signature(args),
        "concat" | "concat_ws" | "substring" | "substr" | "lower" | "upper" | "replace"
        | "split_part" | "left" | "right" => Ok(SqlType::Text),
        "length" | "char_length" | "character_length" | "octet_length" => Ok(SqlType::Int32),
        "abs" => absolute_signature(args),
        "round" | "trunc" => first_numeric_argument(args),
        "extract" => Ok(SqlType::Numeric {
            precision: None,
            scale: None,
        }),
        "date_part" => Ok(SqlType::Float64),
        "jsonb_build_object" | "jsonb_build_array" | "jsonb_strip_nulls" => Ok(SqlType::Jsonb),
        "json_build_object" | "json_build_array" => Ok(SqlType::Json),
        "array_cat" => first_array(args),
        _ => Err(error("unsupported.function", name)),
    }
}

/// Preserve the existing round/trunc inference until their complete overload registry is admitted.
fn first_numeric_argument(args: &[SqlType]) -> Result<SqlType> {
    args.first()
        .cloned()
        .ok_or_else(|| error("sql.42883", "Missing numeric argument"))
}

/// Absolute value resolves one numeric overload before NULL execution and discards storage typmods.
fn absolute_signature(args: &[SqlType]) -> Result<SqlType> {
    let [ty] = args else {
        return Err(error(
            "sql.42883",
            "Absolute value requires one numeric argument",
        ));
    };
    if !numeric(ty) {
        return Err(error(
            "sql.42883",
            "No absolute-value overload for this SQL type",
        ));
    }
    Ok(match ty {
        SqlType::Numeric { .. } => SqlType::Numeric {
            precision: None,
            scale: None,
        },
        other => other.clone(),
    })
}

/// Resolve an array-returning signature without losing element nominal type.
fn first_array(args: &[SqlType]) -> Result<SqlType> {
    args.iter()
        .find(|ty| matches!(ty, SqlType::Array(_)))
        .cloned()
        .ok_or_else(|| error("sql.42883", "Array argument required"))
}

/// Array functions expose resolved polymorphic arguments; arity errors never reach indexing code.
fn array_signature(name: &str, args: &[SqlType]) -> Result<SqlType> {
    match (name, args) {
        ("array_length", [SqlType::Array(_), SqlType::Int32])
        | ("cardinality", [SqlType::Array(_)]) => Ok(SqlType::Int32),
        ("array_append", [ty @ SqlType::Array(element), value])
        | ("array_prepend", [value, ty @ SqlType::Array(element)])
            if **element == *value =>
        {
            Ok(ty.clone())
        }
        ("jsonb_set", [SqlType::Jsonb, SqlType::Array(element), SqlType::Jsonb])
        | ("jsonb_set", [SqlType::Jsonb, SqlType::Array(element), SqlType::Jsonb, SqlType::Bool])
            if **element == SqlType::Text =>
        {
            Ok(SqlType::Jsonb)
        }
        _ => Err(error(
            "sql.42883",
            "No array/JSON function with these argument types",
        )),
    }
}

/// Collection builtins require exact signatures before NULL short-circuiting.
fn collection_signature(name: &str, args: &[SqlType]) -> Result<SqlType> {
    let valid = match (name, args) {
        ("jsonb_exists", [SqlType::Jsonb, SqlType::Text]) => true,
        ("jsonb_exists_any" | "jsonb_exists_all", [SqlType::Jsonb, SqlType::Array(element)]) => {
            **element == SqlType::Text
        }
        ("get_bit", [SqlType::Bit { .. } | SqlType::Bytea, SqlType::Int32]) => true,
        _ => false,
    };
    if !valid {
        return Err(error(
            "sql.42883",
            "No collection function with these argument types",
        ));
    }
    Ok(if name == "get_bit" {
        SqlType::Int32
    } else {
        SqlType::Bool
    })
}
