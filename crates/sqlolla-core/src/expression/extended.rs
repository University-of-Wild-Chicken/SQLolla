//! Typed scalar extensions retain explicit casts, conditional values and SQL errors.
use super::*;
use crate::json::array;

impl Expressions<'_> {
    /// Unknown literals retain contextual typing; other nodes expose resolved result families.
    pub(super) fn extended_hint(&self, node: &Value) -> Result<Option<SqlType>> {
        match text(node, "tag")? {
            "sql_null" | "text" => Ok(None),
            _ => self.extended_bind(node, None).map(|expr| Some(expr.ty)),
        }
    }

    /// Bind only enumerated node families and coerce through an admitted common type.
    pub(super) fn extended_bind(
        &self,
        node: &Value,
        expected: Option<SqlType>,
    ) -> Result<BoundExpr> {
        if text(node, "tag")? == "array" {
            return coerce(self.array(node, expected.clone())?, expected);
        }
        let value = match text(node, "tag")? {
            "column" => self.bound_column(node),
            "numbered_parameter" => Ok(literal(self.parameter(node)?)),
            "integer" => integer_value(node),
            "decimal" => decimal_value(node),
            "sql_null" | "text" | "boolean" => bind_literal(node, expected.clone()),
            "binary" => self.extended_binary(node),
            "unary" => self.extended_unary(node),
            "is_null" => self.is_null(node),
            "cast" => self.cast(node),
            "call" => self.call(node),
            "coalesce" => self.coalesce(node),
            "null_if" => self.null_if(node),
            "case" => self.case(node),
            tag => self.extended_special(node, tag),
        }?;
        coerce(value, expected)
    }

    /// Keep special SQL grammar and query-bearing expressions on explicit typed adapters.
    fn extended_special(&self, node: &Value, tag: &str) -> Result<BoundExpr> {
        match tag {
            "json_access" => self.json_access(node),
            "json_existence" => self.json_existence(node),
            "subscript" => self.subscript(node),
            "context_value" => self.context_value(node),
            "in_list" => self.in_list(node),
            "between" => self.between(node),
            "distinct_from" => self.distinct_from(node),
            "quantified_comparison" => self.quantified(node),
            "pattern" => self.pattern(node),
            "regex" => self.regex(node),
            "extract" => self.extract(node),
            "named_call" => self.named_call(node),
            "substring" => self.substring(node),
            "scalar_subquery" | "exists" | "in_query" | "quantified_query" | "array_query" => {
                self.subquery(node)
            }
            tag => Err(error("unsupported.expression", tag)),
        }
    }

    /// Comparisons drop storage typmods before contextual literal conversion.
    fn extended_binary(&self, node: &Value) -> Result<BoundExpr> {
        fields(node, &["tag", "op", "left", "right"])?;
        let op = text(node, "op")?;
        if node["left"]["tag"] == "row" || node["right"]["tag"] == "row" {
            return self.row_compare(node);
        }
        if matches!(op, "add" | "subtract" | "multiply" | "divide" | "concat") {
            return self.arithmetic(node);
        }
        let operation = binary_op(op)?;
        if let Some(value) = self.xid_comparison(node, operation)? {
            return Ok(value);
        }
        let ty = if matches!(operation, BinaryOp::And | BinaryOp::Or) {
            SqlType::Bool
        } else {
            comparison(self.hint(&node["left"])?, self.hint(&node["right"])?)?
        };
        crate::comparison::operator(operation, &ty)?;
        let left = self.bind(&node["left"], Some(ty.clone()))?;
        let right = self.bind(&node["right"], Some(ty))?;
        Ok(comparison_node(operation, left, right))
    }

    /// Arithmetic keeps operand families so calendar/numeric overloads resolve correctly.
    fn arithmetic(&self, node: &Value) -> Result<BoundExpr> {
        let left_type = self.hint(&node["left"])?;
        let right_type = self.hint(&node["right"])?;
        let left = self.bind(&node["left"], left_type.clone().or(right_type.clone()))?;
        let right = self.bind(&node["right"], right_type.or(left_type))?;
        let op = text(node, "op")?.to_owned();
        let ty = scalar::operation_type(&op, &left.ty, &right.ty)?;
        Ok(BoundExpr {
            ty,
            nullable: left.nullable || right.nullable,
            node: ExprNode::Arithmetic {
                op,
                left: Box::new(left),
                right: Box::new(right),
            },
        })
    }

    /// Numeric signs apply to expressions; NOT remains a three-valued Boolean operation.
    fn extended_unary(&self, node: &Value) -> Result<BoundExpr> {
        fields(node, &["tag", "op", "arg"])?;
        let op = text(node, "op")?;
        if op == "not" {
            return self.unary(node);
        }
        if !matches!(op, "positive" | "negative") {
            return Err(error("unsupported.unary", op));
        }
        if op == "negative" && node["arg"]["tag"] == "integer" {
            return signed_value(&node["arg"]);
        }
        let arg = self.bind(&node["arg"], None)?;
        let result = scalar::operation_type("subtract", &arg.ty, &arg.ty)?;
        let ty = if matches!(arg.ty, SqlType::Numeric { .. }) {
            result
        } else {
            arg.ty.clone()
        };
        if op == "positive" {
            return coerce(arg, Some(ty));
        }
        Ok(BoundExpr {
            ty,
            nullable: arg.nullable,
            node: ExprNode::Negate(Box::new(arg)),
        })
    }

    /// Explicit casts preserve complete type syntax and input conversion failures.
    fn cast(&self, node: &Value) -> Result<BoundExpr> {
        fields(node, &["tag", "arg", "type"])?;
        if let Some(qualifier) = super::interval_bind::qualifier(&node["type"])? {
            return self.interval_cast(node, qualifier);
        }
        let ty = super::types::data_type(&node["type"], self)?;
        let arg = if node["arg"]["tag"] == "array" {
            self.array(&node["arg"], Some(ty.clone()))?
        } else {
            self.bind(&node["arg"], None)?
        };
        if node["arg"]["tag"] == "text" {
            return text_cast(&ty, text(&node["arg"], "value")?, self.lookup);
        }
        Ok(BoundExpr {
            ty,
            nullable: arg.nullable,
            node: ExprNode::Cast { arg: Box::new(arg) },
        })
    }

    /// Immutable function admission uses typed signatures rather than accepting arbitrary names.
    fn call(&self, node: &Value) -> Result<BoundExpr> {
        fields(node, &["tag", "name", "args"])?;
        if let Some(value) = self.declared_function(node)? {
            return Ok(value);
        }
        let name = function_name(node)?;
        if let Some(value) = self.special_call(node, &name)? {
            return Ok(value);
        }
        let args = array(node, "args")?
            .iter()
            .map(|arg| self.bind(arg, None))
            .collect::<Result<Vec<_>>>()?;
        if let Some(value) = super::context::call(&name, &args)? {
            return Ok(value);
        }
        let types = args.iter().map(|arg| arg.ty.clone()).collect::<Vec<_>>();
        let ty = scalar::return_type(&name, &types)?;
        Ok(BoundExpr {
            ty,
            nullable: true,
            node: ExprNode::Call { name, args },
        })
    }

    /// Dedicated typed families resolve polymorphic and effectful arguments before common dispatch.
    fn special_call(&self, node: &Value, name: &str) -> Result<Option<BoundExpr>> {
        if name == "abs" {
            return self.absolute_call(node).map(Some);
        }
        if name == "make_interval" {
            return self.interval_call(node).map(Some);
        }
        if let Some(value) = self.sequence_call(node, name)? {
            return Ok(Some(value));
        }
        if let Some(value) = self.session_effect_call(node, name)? {
            return Ok(Some(value));
        }
        if let Some(value) = self.string_collection_call(node, name)? {
            return Ok(Some(value));
        }
        if let Some(value) = self.array_call(node, name)? {
            return Ok(Some(value));
        }
        self.collection_call(node, name)
    }

    /// COALESCE resolves one common type before runtime branch selection.
    fn coalesce(&self, node: &Value) -> Result<BoundExpr> {
        fields(node, &["tag", "args"])?;
        let args = array(node, "args")?;
        if args.is_empty() {
            return Err(error("sql.42601", "COALESCE requires arguments"));
        }
        let ty = self.common_arguments(args)?;
        let args = args
            .iter()
            .map(|arg| self.bind(arg, Some(ty.clone())))
            .collect::<Result<Vec<_>>>()?;
        Ok(BoundExpr {
            ty,
            nullable: args.iter().all(|arg| arg.nullable),
            node: ExprNode::Coalesce(args),
        })
    }

    /// NULLIF keeps a nullable result of the comparison's common value type.
    fn null_if(&self, node: &Value) -> Result<BoundExpr> {
        fields(node, &["tag", "left", "right"])?;
        super::xid::composite(self.hint(&node["left"])?, self.hint(&node["right"])?)?;
        let ty = comparison(self.hint(&node["left"])?, self.hint(&node["right"])?)?;
        let left = self.bind(&node["left"], Some(ty.clone()))?;
        let right = self.bind(&node["right"], Some(ty.clone()))?;
        Ok(BoundExpr {
            ty,
            nullable: true,
            node: ExprNode::NullIf {
                left: Box::new(left),
                right: Box::new(right),
            },
        })
    }

    /// CASE conditions are Boolean; simple CASE compares its operand against each WHEN.
    fn case(&self, node: &Value) -> Result<BoundExpr> {
        fields(node, &["tag", "operand", "branches", "else"])?;
        let branches = array(node, "branches")?;
        let mut values = branches
            .iter()
            .map(|branch| branch["then"].clone())
            .collect::<Vec<_>>();
        values.push(if node["else"].is_null() {
            serde_json::json!({"tag":"sql_null"})
        } else {
            node["else"].clone()
        });
        let ty = self.common_arguments(&values)?;
        let otherwise = Box::new(self.bind(values.last().unwrap(), Some(ty.clone()))?);
        let branches = branches
            .iter()
            .map(|branch| self.case_branch(branch, &node["operand"], &ty))
            .collect::<Result<Vec<_>>>()?;
        Ok(BoundExpr {
            ty,
            nullable: true,
            node: ExprNode::Case {
                branches,
                otherwise,
            },
        })
    }

    /// A simple CASE operand is compared under SQL NULL/3VL, never Rust Option equality.
    fn case_branch(
        &self,
        node: &Value,
        operand: &Value,
        ty: &SqlType,
    ) -> Result<(BoundExpr, BoundExpr)> {
        fields(node, &["when", "then"])?;
        let condition = if operand.is_null() {
            node["when"].clone()
        } else {
            serde_json::json!({"tag":"binary","op":"eq","left":operand,"right":node["when"]})
        };
        Ok((
            self.bind(&condition, Some(SqlType::Bool))?,
            self.bind(&node["then"], Some(ty.clone()))?,
        ))
    }

    /// Resolve unknown literals only after considering every known argument type.
    fn common_arguments(&self, nodes: &[Value]) -> Result<SqlType> {
        let mut ty = None;
        for node in nodes {
            if let Some(next) = self.hint(node)? {
                ty = Some(match ty {
                    None => next,
                    Some(ref old) => scalar::common_type(old, &next)?,
                });
            }
        }
        Ok(ty.unwrap_or(SqlType::Text))
    }
}

/// Unknown string conversion precedes planning, while character/numeric typmods remain casts.
fn text_cast(ty: &SqlType, value: &str, lookup: Option<&CatalogLookup>) -> Result<BoundExpr> {
    if *ty == SqlType::Regclass {
        return crate::catalog_lookup::input(&Cell::Text(value.into()), lookup).map(literal);
    }
    let base = match ty {
        SqlType::Char { .. } | SqlType::Varchar { .. } => SqlType::Text,
        SqlType::Numeric { .. } => SqlType::Numeric {
            precision: None,
            scale: None,
        },
        _ => ty.clone(),
    };
    let arg = literal(scalar::input(&base, value)?);
    Ok(BoundExpr {
        ty: ty.clone(),
        nullable: false,
        node: ExprNode::Cast { arg: Box::new(arg) },
    })
}

/// Contextual conversion is explicit in typed IR rather than changing a column annotation.
fn coerce(expr: BoundExpr, expected: Option<SqlType>) -> Result<BoundExpr> {
    let Some(ty) = expected else {
        return Ok(expr);
    };
    if ty == expr.ty {
        return Ok(expr);
    }
    if !scalar::compatible(&expr.ty, &ty) {
        return Err(error("sql.42804", "Expression type disagrees with context"));
    }
    Ok(BoundExpr {
        ty,
        nullable: expr.nullable,
        node: ExprNode::Cast {
            arg: Box::new(expr),
        },
    })
}

/// Operator type inference removes storage typmods before coercing unknown input literals.
fn comparison(left: Option<SqlType>, right: Option<SqlType>) -> Result<SqlType> {
    let left = left.or_else(|| right.clone()).unwrap_or(SqlType::Text);
    scalar::comparison_type(&left, &right.unwrap_or_else(|| left.clone()))
}

/// Numeric integer literals choose Int32, then Int64, then exact PostgreSQL numeric.
fn integer_value(node: &Value) -> Result<BoundExpr> {
    fields(node, &["tag", "magnitude"])?;
    number(text(node, "magnitude")?)
}

/// A sign is folded before range selection so the signed minimum keeps its true type.
fn signed_value(node: &Value) -> Result<BoundExpr> {
    fields(node, &["tag", "magnitude"])?;
    number(&format!("-{}", text(node, "magnitude")?))
}

/// Parse exact decimal text through the canonical scalar layer.
fn decimal_value(node: &Value) -> Result<BoundExpr> {
    fields(node, &["tag", "lexeme"])?;
    scalar::input(
        &SqlType::Numeric {
            precision: None,
            scale: None,
        },
        text(node, "lexeme")?,
    )
    .map(literal)
}

/// Literal range selection never truncates an out-of-range integer to Int32.
fn number(value: &str) -> Result<BoundExpr> {
    if let Ok(value) = value.parse::<i32>() {
        return Ok(literal(Cell::Int32(value)));
    }
    if let Ok(value) = value.parse::<i64>() {
        return Ok(literal(Cell::Int64(value)));
    }
    scalar::input(
        &SqlType::Numeric {
            precision: None,
            scale: None,
        },
        value,
    )
    .map(literal)
}

/// Only builtin/unqualified or explicit pg_catalog calls are resolved by this signature table.
fn function_name(node: &Value) -> Result<String> {
    let parts = array(node, "name")?
        .iter()
        .map(|value| {
            value
                .as_str()
                .ok_or_else(|| error("invalid.function_name", "Function identifier must be text"))
        })
        .collect::<Result<Vec<_>>>()?;
    match parts.as_slice() {
        [name] | ["pg_catalog", name] if crate::function_registry::builtin_name(name) => {
            Ok((*name).into())
        }
        [name] | ["pg_catalog", name] => Err(error("unsupported.function", *name)),
        _ => Err(error("unsupported.function_namespace", parts.join("."))),
    }
}

/// Keep a scalar comparison's nullable result and exact operand types in one structural node.
fn comparison_node(op: BinaryOp, left: BoundExpr, right: BoundExpr) -> BoundExpr {
    BoundExpr {
        ty: SqlType::Bool,
        nullable: left.nullable || right.nullable,
        node: ExprNode::Binary {
            op,
            left: Box::new(left),
            right: Box::new(right),
        },
    }
}
