//! Resolve complete type syntax against builtin families and declared nominal enums.
use super::*;

/// Cast modifiers remain part of the target type rather than being ignored.
pub(super) fn data_type(node: &Value, scope: &Expressions<'_>) -> Result<SqlType> {
    fields(node, &["name", "modifiers"])?;
    let modifiers = crate::json::array(node, "modifiers")?
        .iter()
        .map(|value| {
            value
                .as_u64()
                .and_then(|value| u32::try_from(value).ok())
                .ok_or_else(|| error("invalid.type_modifier", "Invalid type modifier"))
        })
        .collect::<Result<Vec<_>>>()?;
    type_name(&node["name"], &modifiers, scope)
}

/// Array nesting and nominal identities are preserved recursively.
fn type_name(node: &Value, modifiers: &[u32], scope: &Expressions<'_>) -> Result<SqlType> {
    match text(node, "tag")? {
        "scalar_type" => {
            fields(node, &["tag", "name"])?;
            builtin(text(node, "name")?, modifiers)
        }
        "array_type" => {
            fields(node, &["tag", "element"])?;
            let element = type_name(&node["element"], modifiers, scope)?;
            Ok(match element {
                SqlType::Array(_) => element,
                _ => SqlType::Array(Box::new(element)),
            })
        }
        "named_type" => named(node, scope),
        tag => Err(error("unsupported.type", tag)),
    }
}

/// Builtin scalar labels are mapped explicitly; unknown names fail closed.
fn builtin(name: &str, modifiers: &[u32]) -> Result<SqlType> {
    match name {
        "varchar" => width(modifiers).map(|length| SqlType::Varchar { length }),
        "char" => width(modifiers).map(|length| SqlType::Char {
            length: length.unwrap_or(1),
        }),
        "bit" => width(modifiers).map(|length| SqlType::Bit {
            length: length.unwrap_or(1),
        }),
        "numeric" | "decimal" => numeric(modifiers),
        _ if !modifiers.is_empty() => Err(error("unsupported.type_modifier", name)),
        _ => plain(name),
    }
}

/// Supported plain builtins retain their PostgreSQL family identity.
fn plain(name: &str) -> Result<SqlType> {
    Ok(match name {
        "int" | "integer" | "int4" => SqlType::Int32,
        "bigint" | "int8" => SqlType::Int64,
        "smallint" | "int2" => SqlType::Int16,
        "text" => SqlType::Text,
        "name" => SqlType::Name,
        "oid" => SqlType::Oid,
        "xid" => SqlType::Xid,
        "bool" | "boolean" => SqlType::Bool,
        "double_precision" | "float8" => SqlType::Float64,
        "bytea" => SqlType::Bytea,
        "date" => SqlType::Date,
        "timestamp" => SqlType::Timestamp,
        "timestamptz" => SqlType::Timestamptz,
        "interval" => SqlType::Interval,
        "jsonb" => SqlType::Jsonb,
        "json" => SqlType::Json,
        _ => return Err(error("unsupported.type", name)),
    })
}

/// Character/bit typmods accept at most one strictly positive length.
fn width(modifiers: &[u32]) -> Result<Option<u32>> {
    match modifiers {
        [] => Ok(None),
        [value] if *value > 0 => Ok(Some(*value)),
        _ => Err(error("sql.22023", "Invalid type length")),
    }
}

/// Numeric precision/scale modifiers are checked before type construction.
fn numeric(modifiers: &[u32]) -> Result<SqlType> {
    let (precision, scale) = match modifiers {
        [] => (None, None),
        [precision] => (Some(*precision), Some(0)),
        [precision, scale] => (Some(*precision), Some(*scale)),
        _ => return Err(error("sql.22023", "Invalid numeric modifiers")),
    };
    let precision = precision
        .map(|value| u16::try_from(value).map_err(|_| error("sql.22023", "Numeric precision")))
        .transpose()?;
    let scale = scale
        .map(|value| i16::try_from(value).map_err(|_| error("sql.22023", "Numeric scale")))
        .transpose()?;
    Ok(SqlType::Numeric { precision, scale })
}

/// Named casts require a unique nominal declaration in the explicit column/parameter context.
fn named(node: &Value, scope: &Expressions<'_>) -> Result<SqlType> {
    fields(node, &["tag", "name"])?;
    let name = qualified_name(node)?;
    if let Some(ty) = catalog_builtin(&name) {
        return Ok(ty);
    }
    if let Some(context) = scope.scope {
        if let Some(ty) = context.nominal_type(&name)? {
            return Ok(ty);
        }
    }
    let mut matches = Vec::new();
    for ty in scope
        .columns
        .iter()
        .map(|column| column.ty.clone())
        .chain(scope.params.values().map(Cell::sql_type))
    {
        if let Some(ty) = nominal(&ty, &name) {
            if !matches.contains(&ty) {
                matches.push(ty);
            }
        }
    }
    match matches.as_slice() {
        [ty] => Ok(ty.clone()),
        _ => Err(error("unsupported.nominal_type", name)),
    }
}

/// Keep decoded identifier components explicit without parsing SQL spelling in Rust.
fn qualified_name(node: &Value) -> Result<String> {
    crate::json::array(node, "name")?
        .iter()
        .map(|value| {
            value
                .as_str()
                .ok_or_else(|| error("invalid.type_name", "Type identifiers must be text"))
        })
        .collect::<Result<Vec<_>>>()
        .map(|names| names.join("."))
}

/// Named catalog types retain quoted-char identity instead of SQL CHAR storage semantics.
fn catalog_builtin(name: &str) -> Option<SqlType> {
    match name {
        "oid" | "pg_catalog.oid" => Some(SqlType::Oid),
        "xid" | "pg_catalog.xid" => Some(SqlType::Xid),
        "regclass" | "pg_catalog.regclass" => Some(SqlType::Regclass),
        "char" | "pg_catalog.char" => Some(SqlType::InternalChar),
        _ => None,
    }
}

/// Resolve array element enums without replacing nominal equality by plain text equality.
fn nominal(ty: &SqlType, name: &str) -> Option<SqlType> {
    match ty {
        SqlType::Array(element) => nominal(element, name),
        SqlType::Enum { name: declared, .. }
            if declared == name
                || (!name.contains('.') && declared.rsplit('.').next() == Some(name)) =>
        {
            Some(ty.clone())
        }
        _ => None,
    }
}
