//! Immutable builtin functions; clocks and external effects require explicit context.
use super::{
    input,
    operations::{decimal, output},
    temporal, typing,
};
use crate::json::error;
use crate::{Cell, Result, SqlType};

/// Evaluate an admitted immutable function; unsupported functions never return guessed data.
pub fn function(name: &str, args: &[Cell]) -> Result<Cell> {
    args.iter().try_for_each(Cell::validate)?;
    let result = function_inner(name, args)?;
    result.validate()?;
    Ok(result)
}

/// Dispatch after validating all supplied arguments, then validate the computed text result.
fn function_inner(name: &str, args: &[Cell]) -> Result<Cell> {
    let name = name.to_ascii_lowercase();
    if let Some(result) = constructor(&name, args) {
        return result;
    }
    let behavior =
        super::builtin_behavior(&name).ok_or_else(|| error("unsupported.function", &name))?;
    if !behavior.implemented {
        return Err(error("unsupported.function", &name));
    }
    let types: Vec<_> = args.iter().map(Cell::sql_type).collect();
    let ty = typing::return_type(&name, &types)?;
    if let Some(result) = custom_function(&name, args) {
        return result;
    }
    if behavior.null_policy == super::NullPolicy::Strict && args.iter().any(Cell::is_null) {
        return Ok(Cell::Null(ty));
    }
    dispatch(&name, args)
}

/// Argument-specific NULL handling is explicit and occurs only after signature validation.
fn custom_function(name: &str, args: &[Cell]) -> Option<Result<Cell>> {
    Some(match name {
        "string_to_array" => super::string_collections::split(args),
        "array_to_string" => super::string_collections::join(args),
        "array_append" | "array_prepend" | "array_length" | "cardinality" => {
            super::array_functions::function(name, args)
        }
        "concat" => concat(args),
        "concat_ws" => concat_ws(args),
        _ => return None,
    })
}

/// Internal operators and constructors supply their own NULL and argument validation.
fn constructor(name: &str, args: &[Cell]) -> Option<Result<Cell>> {
    Some(match name {
        "__sqlolla_regex" => super::regex::evaluate(args),
        "__sqlolla_json_access" => super::json_access::evaluate(args),
        "__sqlolla_array_slice" => super::array_functions::slice(args),
        "__sqlolla_array_element" => super::collections::array_element(args),
        "json_build_object" | "json_build_array" | "jsonb_build_object" | "jsonb_build_array" => {
            super::json_functions::construct(name, args)
        }
        "make_interval" => super::interval::make(args),
        _ => return None,
    })
}

/// SQL concat ignores NULL values without losing the output byte-admission check.
fn concat(args: &[Cell]) -> Result<Cell> {
    Ok(Cell::Text(
        args.iter()
            .filter(|value| !value.is_null())
            .map(output)
            .collect::<Result<Vec<_>>>()?
            .concat(),
    ))
}

/// Dispatch immutable function bodies after common NULL/type checks.
fn dispatch(name: &str, args: &[Cell]) -> Result<Cell> {
    match name {
        "jsonb_typeof" => super::string_collections::json_type(&args[0]),
        "jsonb_set" => super::json_update::set(args),
        "jsonb_exists" | "jsonb_exists_any" | "jsonb_exists_all" => {
            super::collections::json_exists(name, args)
        }
        "get_bit" => super::collections::get_bit(args),
        "length" | "char_length" | "character_length" => count(args.first(), false),
        "octet_length" => count(args.first(), true),
        "substring" | "substr" => substring(args),
        "left" | "right" => side(name, args),
        "lower" | "upper" => ascii_case(name, args),
        "replace" => replace(args),
        "abs" => absolute(args),
        "round" => round(args),
        "extract" | "date_part" => extract(name, args),
        _ => Err(error("unsupported.function", name)),
    }
}

/// Require exact fixed arity before indexing function arguments.
fn arity(args: &[Cell], expected: usize) -> Result<()> {
    if args.len() == expected {
        Ok(())
    } else {
        Err(error("sql.42883", "Function argument count mismatch"))
    }
}
/// Count Unicode characters or octets with a checked PostgreSQL integer result.
fn count(value: Option<&Cell>, bytes: bool) -> Result<Cell> {
    let value = output(value.ok_or_else(|| error("sql.42883", "Missing length argument"))?)?;
    let length = if bytes {
        value.len()
    } else {
        value.chars().count()
    };
    i32::try_from(length)
        .map(Cell::Int32)
        .map_err(|_| error("sql.22003", "Length out of range"))
}
/// Concatenate non-NULL values with the first argument as a nullable separator.
fn concat_ws(args: &[Cell]) -> Result<Cell> {
    let separator = args
        .first()
        .ok_or_else(|| error("sql.42883", "Missing separator"))?;
    if separator.is_null() {
        return Ok(Cell::Null(SqlType::Text));
    }
    Ok(Cell::Text(
        args[1..]
            .iter()
            .filter(|value| !value.is_null())
            .map(output)
            .collect::<Result<Vec<_>>>()?
            .join(&output(separator)?),
    ))
}
/// Apply PostgreSQL one-based substring positioning and negative-length errors.
fn substring(args: &[Cell]) -> Result<Cell> {
    if !(2..=3).contains(&args.len()) {
        return Err(error("sql.42883", "Substring arity mismatch"));
    }
    let text = output(&args[0])?;
    let start = integer(&args[1])?;
    let length = if args.len() == 3 {
        integer(&args[2])?
    } else {
        i64::MAX
    };
    if length < 0 {
        return Err(error("sql.22011", "Negative substring length"));
    }
    let skipped = start.saturating_sub(1).max(0) as usize;
    let taken = length.saturating_add(start.saturating_sub(1).min(0)).max(0) as usize;
    Ok(Cell::Text(text.chars().skip(skipped).take(taken).collect()))
}
/// Select characters from either end; negative counts remove from the opposite end.
fn side(name: &str, args: &[Cell]) -> Result<Cell> {
    arity(args, 2)?;
    let text: Vec<_> = output(&args[0])?.chars().collect();
    let count = integer(&args[1])?;
    let amount = if count >= 0 {
        (count as usize).min(text.len())
    } else {
        text.len().saturating_sub(count.unsigned_abs() as usize)
    };
    Ok(Cell::Text(
        if name == "left" {
            &text[..amount]
        } else {
            &text[text.len() - amount..]
        }
        .iter()
        .collect(),
    ))
}
/// Admit ASCII case conversion only; Unicode locale mappings require a separate profile.
fn ascii_case(name: &str, args: &[Cell]) -> Result<Cell> {
    arity(args, 1)?;
    let text = output(&args[0])?;
    if !text.is_ascii() {
        return Err(error(
            "unsupported.case_locale",
            "Non-ASCII case conversion requires locale profile",
        ));
    }
    Ok(Cell::Text(if name == "lower" {
        text.to_ascii_lowercase()
    } else {
        text.to_ascii_uppercase()
    }))
}
/// Replace literal substrings with no regex or locale side effects.
fn replace(args: &[Cell]) -> Result<Cell> {
    arity(args, 3)?;
    Ok(Cell::Text(
        output(&args[0])?.replace(&output(&args[1])?, &output(&args[2])?),
    ))
}
/// Reject overflowing signed absolute values instead of wrapping.
fn absolute(args: &[Cell]) -> Result<Cell> {
    arity(args, 1)?;
    let text = output(&args[0])?;
    let ty = typing::return_type("abs", &[args[0].sql_type()])?;
    input(&ty, text.strip_prefix('-').unwrap_or(&text))
}
/// Round exact numeric values using the declared half-away-from-zero policy.
fn round(args: &[Cell]) -> Result<Cell> {
    if !(1..=2).contains(&args.len()) {
        return Err(error("sql.42883", "Round arity mismatch"));
    }
    let scale = if args.len() == 2 {
        i32::try_from(integer(&args[1])?).map_err(|_| error("sql.22003", "Scale out of range"))?
    } else {
        0
    };
    input(
        &SqlType::Numeric {
            precision: None,
            scale: None,
        },
        &decimal(&args[0])?.rounded(scale)?.to_string(),
    )
}
/// Extract Gregorian year/month/day under the fixed UTC temporal profile.
fn extract(name: &str, args: &[Cell]) -> Result<Cell> {
    arity(args, 2)?;
    let field = output(&args[0])?.to_ascii_lowercase();
    let text = output(&args[1])?;
    let days = if args[1].sql_type() == SqlType::Date {
        temporal::date(&text)?
    } else {
        temporal::timestamp(&text, args[1].sql_type() == SqlType::Timestamptz)?
            .div_euclid(temporal::DAY)
    };
    let (year, month, day) = temporal::civil(days);
    let value = match field.as_str() {
        "year" => year,
        "month" => month,
        "day" => day,
        _ => return Err(error("unsupported.extract_field", field)),
    };
    let ty = if name == "date_part" {
        SqlType::Float64
    } else {
        SqlType::Numeric {
            precision: None,
            scale: None,
        }
    };
    input(&ty, &value.to_string())
}
/// Read an already-typed integral function argument without truncation.
fn integer(value: &Cell) -> Result<i64> {
    output(value)?
        .parse()
        .map_err(|_| error("sql.42883", "Integral function argument required"))
}
