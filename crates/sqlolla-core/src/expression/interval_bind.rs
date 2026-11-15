//! Interval ranges and named construction arguments are retained instead of erased during typing.
use super::*;
use crate::json::array;
use crate::scalar::{IntervalFields, IntervalQualifier};

/// Recognize only scalar interval types; array interval typmods remain a separate obligation.
pub(super) fn qualifier(node: &Value) -> Result<Option<IntervalQualifier>> {
    fields(node, &["name", "modifiers"])?;
    let name = &node["name"];
    let fields_value = match text(name, "tag")? {
        "interval_range_type" => {
            fields(name, &["tag", "fields"])?;
            serde_json::from_value(name["fields"].clone())
                .map_err(|_| error("invalid.interval_fields", "Unknown interval range"))?
        }
        "scalar_type" if name["name"] == "interval" => {
            fields(name, &["tag", "name"])?;
            IntervalFields::Full
        }
        _ => return Ok(None),
    };
    let precision = precision(array(node, "modifiers")?)?;
    Ok(Some(IntervalQualifier {
        fields: fields_value,
        precision,
    }))
}

/// PostgreSQL clamps larger precisions with a warning; that warning profile is not silently erased.
fn precision(modifiers: &[Value]) -> Result<Option<u8>> {
    match modifiers {
        [] => Ok(None),
        [value] => value
            .as_u64()
            .filter(|value| *value <= 6)
            .map(|value| Some(value as u8))
            .ok_or_else(|| {
                error(
                    "unsupported.interval_precision",
                    "Only warning-free precision0..6 is admitted",
                )
            }),
        _ => Err(error(
            "invalid.interval_precision",
            "Expected at most one interval precision",
        )),
    }
}

impl Expressions<'_> {
    /// Positional construction uses the same signature resolution as normalized named arguments.
    pub(super) fn interval_call(&self, node: &Value) -> Result<BoundExpr> {
        let nodes = array(node, "args")?;
        if nodes.len() > 7 {
            return Err(error("sql.42883", "Too many interval arguments"));
        }
        let mut slots = nodes.iter().cloned().map(Some).collect::<Vec<_>>();
        slots.resize(7, None);
        self.interval_arguments(slots)
    }
    /// Unknown strings use qualifier default units; already typed text uses ordinary seconds input.
    pub(super) fn interval_cast(
        &self,
        node: &Value,
        qualifier: IntervalQualifier,
    ) -> Result<BoundExpr> {
        if node["arg"]["tag"] == "text" {
            return scalar::interval_input(text(&node["arg"], "value")?, &qualifier).map(literal);
        }
        let arg = self.bind(&node["arg"], None)?;
        Ok(BoundExpr {
            ty: SqlType::Interval,
            nullable: arg.nullable,
            node: ExprNode::IntervalCast {
                arg: Box::new(arg),
                qualifier,
            },
        })
    }

    /// Named builtin arguments require unique names, legal order and exact positional signatures.
    pub(super) fn named_call(&self, node: &Value) -> Result<BoundExpr> {
        fields(node, &["tag", "name", "args"])?;
        let names = array(node, "name")?;
        if !matches!(names.as_slice(),[Value::String(name)] if name=="make_interval")
            && !matches!(names.as_slice(),[Value::String(schema),Value::String(name)] if schema=="pg_catalog"&&name=="make_interval")
        {
            return Err(error(
                "unsupported.named_function",
                "Named builtin not admitted",
            ));
        }
        let slots = argument_slots(array(node, "args")?)?;
        self.interval_arguments(slots)
    }

    /// Implicit function argument coercion does not perform assignment-only numeric narrowing.
    fn interval_arguments(&self, slots: Vec<Option<Value>>) -> Result<BoundExpr> {
        let args = slots
            .into_iter()
            .enumerate()
            .map(|(index, value)| {
                let ty = if index == 6 {
                    SqlType::Float64
                } else {
                    SqlType::Int32
                };
                match value {
                    Some(value) => self.interval_argument(&value, index, ty),
                    None => scalar::input(&ty, "0").map(literal),
                }
            })
            .collect::<Result<Vec<_>>>()?;
        Ok(BoundExpr {
            ty: SqlType::Interval,
            nullable: args.iter().any(|arg| arg.nullable),
            node: ExprNode::Call {
                name: "make_interval".into(),
                args,
            },
        })
    }

    /// PostgreSQL rejects bigint/numeric in integer slots even when a value would happen to fit.
    fn interval_argument(&self, node: &Value, index: usize, ty: SqlType) -> Result<BoundExpr> {
        if let Some(actual) = self.hint(node)? {
            let valid = if index < 6 {
                matches!(actual, SqlType::Int16 | SqlType::Int32)
            } else {
                matches!(
                    actual,
                    SqlType::Int16
                        | SqlType::Int32
                        | SqlType::Int64
                        | SqlType::Numeric { .. }
                        | SqlType::Float64
                )
            };
            if !valid {
                return Err(error("sql.42883", "No matching make_interval signature"));
            }
        }
        self.bind(node, Some(ty))
    }
}

/// Argument collisions distinguish repeated names from collision with an earlier positional slot.
fn argument_slots(args: &[Value]) -> Result<Vec<Option<Value>>> {
    let mut slots = vec![None; 7];
    let mut named = false;
    let mut names = std::collections::BTreeSet::new();
    for (position, arg) in args.iter().enumerate() {
        let index = match text(arg, "tag")? {
            "positional_argument" => {
                fields(arg, &["tag", "value"])?;
                if named {
                    return Err(error(
                        "sql.42601",
                        "Positional argument follows named argument",
                    ));
                }
                position
            }
            "named_argument" => {
                named = true;
                let (name, index) = named_position(arg)?;
                if !names.insert(name) {
                    return Err(error("sql.42601", "Duplicate named argument"));
                }
                index
            }
            _ => return Err(error("invalid.argument", "Unknown function argument shape")),
        };
        fill_slot(&mut slots, index, arg["value"].clone())?;
    }
    Ok(slots)
}

/// Resolve a named argument only after validating the exact syntax metadata.
fn named_position(arg: &Value) -> Result<(&str, usize)> {
    fields(arg, &["tag", "name", "value", "spelling"])?;
    if !matches!(text(arg, "spelling")?, "arrow" | "assignment") {
        return Err(error(
            "invalid.argument_spelling",
            "Unknown named argument syntax",
        ));
    }
    let name = text(arg, "name")?;
    let index = ["years", "months", "weeks", "days", "hours", "mins", "secs"]
        .iter()
        .position(|known| *known == name)
        .ok_or_else(|| error("sql.42883", "Unknown interval argument"))?;
    Ok((name, index))
}

/// Assignment collisions retain PostgreSQL's function-resolution SQLSTATE.
fn fill_slot(slots: &mut [Option<Value>], index: usize, value: Value) -> Result<()> {
    let slot = slots
        .get_mut(index)
        .ok_or_else(|| error("sql.42883", "Too many interval arguments"))?;
    if slot.is_some() {
        return Err(error(
            "sql.42883",
            "Named argument collides with positional argument",
        ));
    }
    *slot = Some(value);
    Ok(())
}
