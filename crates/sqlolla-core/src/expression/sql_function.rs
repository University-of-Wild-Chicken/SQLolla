//! Deterministic scalar-body expansion for explicit immutable SQL definitions and inert arguments.
use super::*;
use crate::json::array;
use serde_json::json;
mod resolution;

impl Expressions<'_> {
    /// Resolve declared and builtin signatures before expanding any SQL body.
    pub(super) fn declared_function(&self, node: &Value) -> Result<Option<BoundExpr>> {
        let Some(functions) = self.functions else {
            return Ok(None);
        };
        crate::sql_function::validate(functions)?;
        let Some(function) = resolution::resolve(self, functions, node)? else {
            return Ok(None);
        };
        if !function.can_execute || !function.can_use_schema {
            return Err(error(
                "sql.42501",
                "Function execution or schema usage permission denied",
            ));
        }
        let nodes = array(node, "args")?;
        if nodes.len() != function.parameters.len() {
            return Err(error("sql.42883", "Function argument arity mismatch"));
        }
        let args = nodes
            .iter()
            .zip(&function.parameters)
            .map(|(node, parameter)| self.inert_argument(node, &parameter.ty))
            .collect::<Result<Vec<_>>>()?;
        let mut body = prepared_body(function, &args)?;
        substitute(&mut body, &args)?;
        Ok(Some(return_cast(body, &function.returns)?))
    }

    /// Select a signature before contextualizing unknown input; typed arguments use implicit casts.
    fn inert_argument(&self, node: &Value, parameter: &SqlType) -> Result<BoundExpr> {
        let hint = self.hint(node)?;
        let expected = hint.is_none().then(|| parameter.clone());
        let value = inert_literal_cast(self.bind(node, expected)?)?;
        if !crate::function_registry::implicit_cast(&value.ty, parameter) {
            return Err(error(
                "sql.42883",
                "No function overload accepts this argument type",
            ));
        }
        inert_conversion(value, parameter)
    }
}

/// Explicit literal casts are evaluated before substitution so an unused parameter cannot erase errors.
fn inert_literal_cast(value: BoundExpr) -> Result<BoundExpr> {
    if let ExprNode::Cast { arg } = &value.node {
        if let ExprNode::Literal(cell) = &arg.node {
            return scalar::cast(cell, &value.ty).map(crate::expression::literal);
        }
    }
    Ok(value)
}

/// Only inert actuals may be substituted, and widening casts must themselves be total.
fn inert_conversion(value: BoundExpr, parameter: &SqlType) -> Result<BoundExpr> {
    if !matches!(
        value.node,
        ExprNode::Literal(_) | ExprNode::Column(_) | ExprNode::OuterColumn { .. }
    ) {
        return Err(error(
            "unsupported.sql_function_argument",
            "Function expansion requires inert typed arguments",
        ));
    }
    if crate::function_registry::nominal(&value.ty) == crate::function_registry::nominal(parameter)
    {
        return Ok(value);
    }
    let total = total_widening(&value.ty, parameter);
    if !total {
        return Err(error(
            "unsupported.sql_function_argument",
            "Implicit conversion requires an effect-valued argument boundary",
        ));
    }
    Ok(BoundExpr {
        ty: parameter.clone(),
        nullable: value.nullable,
        node: ExprNode::Cast {
            arg: Box::new(value),
        },
    })
}

/// Function identifiers remain parsed components and are never recovered by splitting SQL text.
fn identifiers(node: &Value) -> Result<Vec<&str>> {
    array(node, "name")?
        .iter()
        .map(|part| {
            part.as_str()
                .ok_or_else(|| error("invalid.function_name", "Nontext function identifier"))
        })
        .collect()
}

/// Body binding has no function environment or query scope, preventing recursion and external reads.
fn prepared_body(function: &ImmutableSqlFunction, arguments: &[BoundExpr]) -> Result<BoundExpr> {
    let report = function.prepared.as_ref().ok_or_else(|| {
        error(
            "unsupported.sql_function_binding",
            "SQL body parser evidence missing",
        )
    })?;
    let report = parameters(report, &function.parameters)?;
    let mut columns = function.parameters.clone();
    for (column, argument) in columns.iter_mut().zip(arguments) {
        if matches!(column.ty, SqlType::Bit { .. }) {
            column.ty = argument.ty.clone();
        }
    }
    let mut body = bind_metadata_expression(&report, &columns)?;
    validate_pure(&mut body)?;
    Ok(body)
}

/// Numbered function parameters become named formal columns, never caller query parameters.
fn parameters(node: &Value, columns: &[Column]) -> Result<Value> {
    if node["tag"] == "numbered_parameter" {
        fields(node, &["tag", "index"])?;
        let index = node["index"]
            .as_u64()
            .and_then(|value| usize::try_from(value).ok())
            .and_then(|value| value.checked_sub(1));
        let column = index
            .and_then(|index| columns.get(index))
            .ok_or_else(|| error("sql.42P02", "Function parameter outside declaration"))?;
        return Ok(json!({"tag":"column","name":column.name,"qualifier":null,"schema":null}));
    }
    match node {
        Value::Array(values) => values
            .iter()
            .map(|value| parameters(value, columns))
            .collect(),
        Value::Object(values) => {
            let mut result = serde_json::Map::new();
            for (name, value) in values {
                result.insert(name.clone(), parameters(value, columns)?);
            }
            Ok(Value::Object(result))
        }
        _ => Ok(node.clone()),
    }
}

/// An IMMUTABLE declaration alone does not justify admitting session-dependent bodies.
fn validate_pure(expression: &mut BoundExpr) -> Result<()> {
    crate::function_registry::require_immutable(expression, "unsupported.sql_function_body")
}

/// Replace each formal column with its already bound inert actual value or outer-row reference.
fn substitute(expression: &mut BoundExpr, arguments: &[BoundExpr]) -> Result<()> {
    if let ExprNode::Column(index) = expression.node {
        *expression = arguments.get(index).cloned().ok_or_else(|| {
            error(
                "invalid.sql_function_column",
                "Function parameter outside scope",
            )
        })?;
        return Ok(());
    }
    crate::rewrite::scalar_children(expression, &mut |value| substitute(value, arguments))
}

/// Function return conversion remains a typed expression rather than relabeling a value's family.
fn return_cast(body: BoundExpr, returns: &SqlType) -> Result<BoundExpr> {
    if &body.ty == returns {
        return Ok(body);
    }
    if !scalar::compatible(&body.ty, returns) {
        return Err(error(
            "sql.42P13",
            "SQL body disagrees with declared return type",
        ));
    }
    Ok(BoundExpr {
        ty: returns.clone(),
        nullable: body.nullable,
        node: ExprNode::Cast {
            arg: Box::new(body),
        },
    })
}

/// These widening conversions cannot fail or produce observable effects.
fn total_widening(source: &SqlType, target: &SqlType) -> bool {
    let character = |ty: &SqlType| {
        matches!(
            ty,
            SqlType::Text
                | SqlType::Varchar { .. }
                | SqlType::Char { .. }
                | SqlType::Bpchar
                | SqlType::Name
        )
    };
    if character(source) && character(target) {
        return crate::function_registry::implicit_cast(source, target);
    }
    matches!(
        (source, target),
        (
            SqlType::Int16,
            SqlType::Int32 | SqlType::Int64 | SqlType::Float64 | SqlType::Numeric { .. }
        ) | (
            SqlType::Int32,
            SqlType::Int64 | SqlType::Float64 | SqlType::Numeric { .. }
        ) | (SqlType::Int64, SqlType::Float64 | SqlType::Numeric { .. })
    )
}
