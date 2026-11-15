//! Conservative builtin collision signatures and PostgreSQL implicit call conversions.
use super::*;

/// Function argument identities ignore storage modifiers without treating unlike types as equal.
pub(crate) fn nominal(ty: &SqlType) -> SqlType {
    match ty {
        SqlType::Numeric { .. } => SqlType::Numeric {
            precision: None,
            scale: None,
        },
        SqlType::Varchar { .. } => SqlType::Varchar { length: None },
        SqlType::Char { .. } => SqlType::Bpchar,
        SqlType::Bit { .. } => SqlType::Bit { length: 1 },
        SqlType::Array(element) => SqlType::Array(Box::new(nominal(element))),
        ty => ty.clone(),
    }
}

/// Only enumerated implicit conversions can make a declared function candidate applicable.
pub(crate) fn implicit_cast(source: &SqlType, target: &SqlType) -> bool {
    let (source, target) = (nominal(source), nominal(target));
    if source == target {
        return true;
    }
    match (&source, &target) {
        (
            SqlType::Int16,
            SqlType::Int32 | SqlType::Int64 | SqlType::Numeric { .. } | SqlType::Float64,
        )
        | (SqlType::Int32, SqlType::Int64 | SqlType::Numeric { .. } | SqlType::Float64)
        | (SqlType::Int64, SqlType::Numeric { .. } | SqlType::Float64)
        | (SqlType::Numeric { .. }, SqlType::Float64) => true,
        (
            SqlType::Text | SqlType::Varchar { .. } | SqlType::Bpchar,
            SqlType::Text | SqlType::Varchar { .. } | SqlType::Bpchar | SqlType::Name,
        ) => true,
        (SqlType::Name, SqlType::Text) => true,
        (SqlType::Date, SqlType::Timestamp | SqlType::Timestamptz)
        | (SqlType::Timestamp, SqlType::Timestamptz) => true,
        (SqlType::Array(left), SqlType::Array(right)) => implicit_cast(left, right),
        _ => false,
    }
}

/// Resolve collisions only with a complete admitted overload inventory for this builtin family.
pub(crate) fn builtin_signatures(name: &str) -> Result<Vec<Vec<SqlType>>> {
    let signatures = match name {
        "abs" => numeric_signatures(),
        "lower" | "upper" => vec![vec![SqlType::Text]],
        "replace" => vec![vec![SqlType::Text; 3]],
        "left" | "right" => vec![vec![SqlType::Text, SqlType::Int32]],
        "split_part" => vec![vec![SqlType::Text, SqlType::Text, SqlType::Int32]],
        "get_bit" => vec![
            vec![SqlType::Bit { length: 1 }, SqlType::Int32],
            vec![SqlType::Bytea, SqlType::Int32],
        ],
        "jsonb_typeof" => vec![vec![SqlType::Jsonb]],
        "string_to_array" => vec![vec![SqlType::Text; 2], vec![SqlType::Text; 3]],
        "version" | "current_schema" | "current_database" | "now" | "pg_backend_pid" => {
            vec![vec![]]
        }
        "to_regclass" => vec![vec![SqlType::Text]],
        "pg_notify" => vec![vec![SqlType::Text; 2]],
        "nextval" | "currval" => vec![vec![SqlType::Regclass]],
        _ => {
            return Err(error(
                "unsupported.function_resolution",
                "Builtin collision requires a complete overload inventory",
            ))
        }
    };
    Ok(signatures)
}

/// Numeric abs has distinct fixed overloads, including both integer widths.
fn numeric_signatures() -> Vec<Vec<SqlType>> {
    [
        SqlType::Int16,
        SqlType::Int32,
        SqlType::Int64,
        SqlType::Numeric {
            precision: None,
            scale: None,
        },
        SqlType::Float64,
    ]
    .into_iter()
    .map(|ty| vec![ty])
    .collect()
}
