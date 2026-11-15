//! Typed scalar binding, contextual unknown literals and exact name resolution.
use crate::json::{boolean, error, fields, optional, text};
use crate::*;
use serde_json::Value;
mod array;
mod array_functions;
mod assignment;
mod collections;
mod context;
mod extended;
mod interval_bind;
mod json_access;
mod labels;
mod numeric_calls;
mod predicates;
mod quantified;
mod regex;
mod row;
mod sequence;
mod session_effects;
mod sql_function;
mod string_collections;
mod subqueries;
mod syntax;
mod types;
mod xid;
pub(crate) use labels::output_label;

/// Query-bearing scalar syntax delegates lexical scope to the relational binder.
pub(crate) trait QueryScope {
    /// Resolve nominal cast types from the declared catalog, independently of projected columns.
    fn nominal_type(&self, name: &str) -> Result<Option<SqlType>>;
    /// Bind a nested query with the current schema as its nearest outer scope.
    fn bind_query(&self, node: &Value, current_columns: &[Column]) -> Result<Plan>;
    /// Resolve a missing local column against explicit outer scopes.
    fn outer_column(&self, node: &Value, current_columns: &[Column]) -> Result<Option<BoundExpr>>;
}

/// Scalar binding inputs are immutable and scoped to the current relation.
pub(crate) struct Expressions<'a> {
    /// Explicit relation identities for strict bind-time regclass input.
    pub lookup: Option<&'a CatalogLookup>,
    /// Optional declared SQL function environment; absent in a function body to block recursion.
    pub functions: Option<&'a SqlFunctions>,
    pub columns: &'a [Column],
    pub params: &'a Parameters,
    pub extended: bool,
    /// None marks metadata scopes where query/correlation is unavailable.
    pub scope: Option<&'a dyn QueryScope>,
}

impl Expressions<'_> {
    /// Infer known scalar types while retaining PostgreSQL unknown literals.
    pub(crate) fn hint(&self, node: &Value) -> Result<Option<SqlType>> {
        if self.extended {
            return self.extended_hint(node);
        }
        match text(node, "tag")? {
            "sql_null" | "text" => Ok(None),
            "integer" => Ok(Some(SqlType::Int32)),
            "boolean" | "is_null" | "binary" => Ok(Some(SqlType::Bool)),
            "column" => Ok(Some(self.columns[self.column(node)?].ty.clone())),
            "numbered_parameter" => Ok(Some(self.parameter(node)?.sql_type())),
            "unary" if text(node, "op")? == "not" => Ok(Some(SqlType::Bool)),
            "unary" => Ok(Some(SqlType::Int32)),
            tag => Err(error("unsupported.expression", tag)),
        }
    }

    /// Resolve a scalar expression with an optional contextual SQL type.
    pub(crate) fn bind(&self, node: &Value, expected: Option<SqlType>) -> Result<BoundExpr> {
        if self.extended {
            return self.extended_bind(node, expected);
        }
        let expr = match text(node, "tag")? {
            "column" => self.bound_column(node),
            "numbered_parameter" => Ok(literal(self.parameter(node)?)),
            "binary" => self.binary(node),
            "unary" => self.unary(node),
            "is_null" => self.is_null(node),
            _ => bind_literal(node, expected.clone()),
        }?;
        if expected.is_some_and(|ty| ty != expr.ty) {
            return Err(error(
                "invalid.type",
                "Expression type does not match its context",
            ));
        }
        Ok(expr)
    }

    /// Resolve qualifiers and schema names without falling back to outer scopes.
    fn column(&self, node: &Value) -> Result<usize> {
        fields(node, &["tag", "name", "qualifier", "schema"])?;
        let (name, qualifier, schema) = (
            text(node, "name")?,
            optional(node, "qualifier")?,
            optional(node, "schema")?,
        );
        qualifier_scope(&qualifier, &schema, self.columns)?;
        let matches: Vec<_> = self
            .columns
            .iter()
            .enumerate()
            .filter(|(_, column)| column_matches(column, name, &qualifier, &schema))
            .map(|(index, _)| index)
            .collect();
        match matches.as_slice() {
            [index] => Ok(*index),
            [] => Err(error("invalid.column", format!("Unknown column {name}"))),
            _ => Err(error(
                "invalid.ambiguous_column",
                format!("Ambiguous column {name}"),
            )),
        }
    }

    /// Copy column type/nullability while replacing names with a positional index.
    fn bound_column(&self, node: &Value) -> Result<BoundExpr> {
        let index = match self.column(node) {
            Ok(index) => index,
            Err(failure) if self.extended && failure.code == "invalid.column" => {
                if let Some(scope) = self.scope {
                    if let Some(expression) = scope.outer_column(node, self.columns)? {
                        return Ok(expression);
                    }
                }
                return Err(failure);
            }
            Err(failure) => return Err(failure),
        };
        let column = &self.columns[index];
        Ok(BoundExpr {
            ty: column.ty.clone(),
            nullable: column.nullable,
            node: ExprNode::Column(index),
        })
    }

    /// Require an explicitly declared PostgreSQL numbered parameter.
    fn parameter(&self, node: &Value) -> Result<Cell> {
        fields(node, &["tag", "index"])?;
        let index = node["index"]
            .as_u64()
            .and_then(|value| u32::try_from(value).ok())
            .filter(|value| (1..=65535).contains(value))
            .ok_or_else(|| error("invalid.parameter", "Invalid parameter index"))?;
        self.params.get(&index).cloned().ok_or_else(|| {
            error(
                "invalid.parameter",
                format!("Undeclared parameter ${index}"),
            )
        })
    }

    /// Bind Boolean connectives and comparisons with one common operand type.
    fn binary(&self, node: &Value) -> Result<BoundExpr> {
        fields(node, &["tag", "op", "left", "right"])?;
        let op = binary_op(text(node, "op")?)?;
        let ty = if matches!(op, BinaryOp::And | BinaryOp::Or) {
            SqlType::Bool
        } else {
            common(self.hint(&node["left"])?, self.hint(&node["right"])?)?
        };
        if matches!(
            op,
            BinaryOp::Lt | BinaryOp::Le | BinaryOp::Gt | BinaryOp::Ge
        ) && ty != SqlType::Int32
        {
            return Err(error(
                "unsupported.order_type",
                "Ordered comparisons require Int32",
            ));
        }
        let left = self.bind(&node["left"], Some(ty.clone()))?;
        let right = self.bind(&node["right"], Some(ty))?;
        Ok(BoundExpr {
            ty: SqlType::Bool,
            nullable: left.nullable || right.nullable,
            node: ExprNode::Binary {
                op,
                left: Box::new(left),
                right: Box::new(right),
            },
        })
    }

    /// Bind NOT, or signed integer literals without admitting general arithmetic.
    fn unary(&self, node: &Value) -> Result<BoundExpr> {
        fields(node, &["tag", "op", "arg"])?;
        match text(node, "op")? {
            "not" => {
                let arg = self.bind(&node["arg"], Some(SqlType::Bool))?;
                Ok(BoundExpr {
                    ty: SqlType::Bool,
                    nullable: arg.nullable,
                    node: ExprNode::Not(Box::new(arg)),
                })
            }
            "positive" | "negative" => signed(node),
            op => Err(error("unsupported.unary", op)),
        }
    }

    /// IS NULL and IS NOT NULL are total Boolean predicates.
    fn is_null(&self, node: &Value) -> Result<BoundExpr> {
        fields(node, &["tag", "arg", "negated"])?;
        Ok(BoundExpr {
            ty: SqlType::Bool,
            nullable: false,
            node: ExprNode::IsNull {
                arg: Box::new(self.bind(&node["arg"], None)?),
                negated: boolean(node, "negated")?,
            },
        })
    }
}

/// Match exact parser-resolved names without case folding quoted identifiers.
fn column_matches(
    column: &Column,
    name: &str,
    qualifier: &Option<String>,
    schema: &Option<String>,
) -> bool {
    column.name == name
        && (qualifier.is_some() || visible_column(column))
        && qualifier
            .as_ref()
            .is_none_or(|q| column.qualifier.as_ref() == Some(q))
        && schema
            .as_ref()
            .is_none_or(|s| column.schema.as_ref() == Some(s))
}

/// Compiler-owned ranking/set slots cannot shadow ordinary unqualified SQL columns.
pub(crate) fn visible_column(column: &Column) -> bool {
    !column
        .qualifier
        .as_ref()
        .is_some_and(|name| name.starts_with("\0sqlolla."))
}

/// A qualified reference resolves a relation before resolving one of its columns.
pub(crate) fn qualifier_scope(
    qualifier: &Option<String>,
    schema: &Option<String>,
    columns: &[Column],
) -> Result<()> {
    if schema.is_some() && qualifier.is_none() {
        return Err(error(
            "invalid.qualifier",
            "Schema needs relation qualifier",
        ));
    }
    if let Some(qualifier) = qualifier {
        let namespaces: std::collections::BTreeSet<_> = columns
            .iter()
            .filter(|column| column.qualifier.as_ref() == Some(qualifier))
            .map(|column| &column.schema)
            .collect();
        if schema.is_none() && namespaces.len() > 1 {
            return Err(error(
                "invalid.ambiguous_qualifier",
                "Relation qualifier is ambiguous",
            ));
        }
    }
    Ok(())
}

/// Resolve common scalar type; two unknowns become text, as in PostgreSQL SELECT.
pub(crate) fn common(left: Option<SqlType>, right: Option<SqlType>) -> Result<SqlType> {
    if left.is_some() && right.is_some() && left != right {
        return Err(error("invalid.type", "Incompatible SQL scalar types"));
    }
    Ok(left.or(right).unwrap_or(SqlType::Text))
}

/// Construct an immutable, typed bound literal.
pub(crate) fn literal(value: Cell) -> BoundExpr {
    BoundExpr {
        ty: value.sql_type(),
        nullable: value.is_null(),
        node: ExprNode::Literal(value),
    }
}

/// Admit only explicitly supported scalar literal tags and exact fields.
fn bind_literal(node: &Value, expected: Option<SqlType>) -> Result<BoundExpr> {
    let value = match text(node, "tag")? {
        "sql_null" => {
            fields(node, &["tag"])?;
            Cell::Null(expected.unwrap_or(SqlType::Text))
        }
        "integer" => {
            fields(node, &["tag", "magnitude"])?;
            integer(text(node, "magnitude")?, false)?
        }
        "boolean" => {
            fields(node, &["tag", "value"])?;
            Cell::Bool(boolean(node, "value")?)
        }
        "text" => {
            fields(node, &["tag", "value"])?;
            contextual_text(text(node, "value")?, expected)?
        }
        tag => return Err(error("unsupported.expression", tag)),
    };
    Ok(literal(value))
}

/// Interpret a decimal magnitude with an optional sign inside Int32 bounds.
fn integer(magnitude: &str, negative: bool) -> Result<Cell> {
    if magnitude.is_empty() || !magnitude.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(error(
            "invalid.integer",
            "Expected unsigned decimal magnitude",
        ));
    }
    let value = magnitude
        .parse::<i64>()
        .map_err(|_| error("unsupported.integer_range", "Integer outside Int32"))?;
    let signed = if negative { -value } else { value };
    i32::try_from(signed)
        .map(Cell::Int32)
        .map_err(|_| error("unsupported.integer_range", "Integer outside Int32"))
}

/// Fold a sign only when applied directly to an integer literal.
fn signed(node: &Value) -> Result<BoundExpr> {
    let arg = &node["arg"];
    if text(arg, "tag")? != "integer" {
        return Err(error(
            "unsupported.arithmetic",
            "Only integer literal signs are admitted",
        ));
    }
    fields(arg, &["tag", "magnitude"])?;
    Ok(literal(integer(
        text(arg, "magnitude")?,
        text(node, "op")? == "negative",
    )?))
}

/// Convert an unknown string literal only through the admitted input profiles.
fn contextual_text(value: &str, expected: Option<SqlType>) -> Result<Cell> {
    match expected.unwrap_or(SqlType::Text) {
        SqlType::Text => crate::scalar::input(&SqlType::Text, value),
        SqlType::Int32 => input_trim(value)
            .parse::<i32>()
            .map(Cell::Int32)
            .map_err(|_| {
                error(
                    "invalid.literal_coercion",
                    "String is not an admitted Int32 input",
                )
            }),
        SqlType::Int64 => input_trim(value)
            .parse::<i64>()
            .map(Cell::Int64)
            .map_err(|_| error("invalid.literal_coercion", "String is not an Int64 input")),
        SqlType::Bool => boolean_text(value).map(Cell::Bool),
        ty => crate::scalar::input(&ty, value),
    }
}

/// PostgreSQL Boolean input accepts unambiguous case-insensitive prefixes.
fn boolean_text(value: &str) -> Result<bool> {
    let value = input_trim(value).to_ascii_lowercase();
    if value == "1" {
        return Ok(true);
    }
    if value == "0" {
        return Ok(false);
    }
    let yes = ["true", "yes", "on"]
        .iter()
        .any(|word| word.starts_with(&value));
    let no = ["false", "no", "off"]
        .iter()
        .any(|word| word.starts_with(&value));
    if !value.is_empty() && yes != no {
        Ok(yes)
    } else {
        Err(error(
            "invalid.literal_coercion",
            "String is not Boolean input",
        ))
    }
}

/// PostgreSQL input whitespace is ASCII; Unicode NBSP is not silently removed.
fn input_trim(value: &str) -> &str {
    value.trim_matches(|ch| matches!(ch, ' ' | '\t' | '\n' | '\r' | '\u{000B}' | '\u{000C}'))
}

/// Translate only the explicitly supported operation labels.
fn binary_op(op: &str) -> Result<BinaryOp> {
    match op {
        "eq" => Ok(BinaryOp::Eq),
        "ne" => Ok(BinaryOp::Ne),
        "lt" => Ok(BinaryOp::Lt),
        "le" => Ok(BinaryOp::Le),
        "gt" => Ok(BinaryOp::Gt),
        "ge" => Ok(BinaryOp::Ge),
        "and" => Ok(BinaryOp::And),
        "or" => Ok(BinaryOp::Or),
        _ => Err(error("unsupported.operator", op)),
    }
}
