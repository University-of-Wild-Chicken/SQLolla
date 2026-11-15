//! Query-bearing scalars retain their query plan and explicit cardinality obligations.
use super::*;

impl Expressions<'_> {
    /// Bind a nested query while preserving explicit lexical correlation.
    pub(super) fn subquery(&self, node: &Value) -> Result<BoundExpr> {
        let scope = self.scope.ok_or_else(|| {
            error(
                "unsupported.subquery_scope",
                "Subqueries are unavailable in this expression scope",
            )
        })?;
        let kind = subquery_kind(node)?;
        let query = scope.bind_query(&node["query"], self.columns)?;
        if kind == SubqueryKind::Exists {
            return exists(query);
        }
        match kind {
            SubqueryKind::Scalar | SubqueryKind::Array => value_query(kind, query),
            _ => self.comparison_subquery(node, kind, query),
        }
    }

    /// IN is equality ANY; quantified comparisons keep their operator and 3VL policy.
    fn comparison_subquery(
        &self,
        node: &Value,
        kind: SubqueryKind,
        query: Plan,
    ) -> Result<BoundExpr> {
        let column = single_column(&query)?;
        let left_type = self.hint(&node["arg"])?;
        super::xid::composite(left_type.clone(), Some(column.ty.clone()))?;
        let ty =
            scalar::comparison_type(&left_type.unwrap_or_else(|| column.ty.clone()), &column.ty)?;
        let left = self.bind(&node["arg"], Some(ty.clone()))?;
        let op = comparison_operator(kind, node)?;
        crate::comparison::operator(op, &ty)?;
        let query = cast_query(query, &ty);
        Ok(query_expression(
            kind,
            query,
            Some(left),
            Some(op),
            SqlType::Bool,
            true,
        ))
    }
}

/// Scalar subqueries preserve type; ARRAY over array-valued columns needs a separate accumulator.
fn value_query(kind: SubqueryKind, query: Plan) -> Result<BoundExpr> {
    let ty = single_column(&query)?.ty.clone();
    let array = kind == SubqueryKind::Array;
    if array && matches!(ty, SqlType::Array(_)) {
        return Err(error(
            "unsupported.subquery_array_element",
            "Array-valued ARRAY subquery requires separate dimensional rules",
        ));
    }
    let ty = if array {
        SqlType::Array(Box::new(ty))
    } else {
        ty
    };
    Ok(query_expression(kind, query, None, None, ty, !array))
}

/// Quantified subqueries admit only actual comparison operators.
fn comparison_operator(kind: SubqueryKind, node: &Value) -> Result<BinaryOp> {
    let op = if kind == SubqueryKind::In {
        BinaryOp::Eq
    } else {
        binary_op(text(node, "op")?)?
    };
    if matches!(op, BinaryOp::And | BinaryOp::Or) {
        return Err(error(
            "invalid.subquery_operator",
            "Expected comparison operator",
        ));
    }
    Ok(op)
}

/// Validate every syntax field before choosing the scalar subquery contract.
fn subquery_kind(node: &Value) -> Result<SubqueryKind> {
    let tag = text(node, "tag")?;
    match tag {
        "scalar_subquery" | "exists" | "array_query" => fields(node, &["tag", "query"])?,
        "in_query" => fields(node, &["tag", "arg", "query"])?,
        "quantified_query" => fields(node, &["tag", "op", "quantifier", "arg", "query"])?,
        _ => return Err(error("unsupported.subquery", tag)),
    }
    match tag {
        "scalar_subquery" => Ok(SubqueryKind::Scalar),
        "exists" => Ok(SubqueryKind::Exists),
        "array_query" => Ok(SubqueryKind::Array),
        "in_query" => Ok(SubqueryKind::In),
        _ => match text(node, "quantifier")? {
            "any" => Ok(SubqueryKind::Any),
            "all" => Ok(SubqueryKind::All),
            value => Err(error("unsupported.quantifier", value)),
        },
    }
}

/// Scalar, array and first-stage quantified subqueries require one output column.
fn single_column(query: &Plan) -> Result<&Column> {
    match query.schema.as_slice() {
        [column] => Ok(column),
        _ => Err(error(
            "sql.42601",
            "Subquery must return exactly one column",
        )),
    }
}

/// EXISTS retains binding checks but drops independently verified unobserved targets.
fn exists(query: Plan) -> Result<BoundExpr> {
    let query = crate::binding::subquery::prepare_exists(query)?;
    Ok(query_expression(
        SubqueryKind::Exists,
        query,
        None,
        None,
        SqlType::Bool,
        false,
    ))
}

/// Coerce one output position to the comparison operator's resolved input family.
fn cast_query(query: Plan, ty: &SqlType) -> Plan {
    if query.schema[0].ty == *ty {
        return query;
    }
    let column = &query.schema[0];
    let source = BoundExpr {
        ty: column.ty.clone(),
        nullable: column.nullable,
        node: ExprNode::Column(0),
    };
    let expression = BoundExpr {
        ty: ty.clone(),
        nullable: column.nullable,
        node: ExprNode::Cast {
            arg: Box::new(source),
        },
    };
    let mut output = column.clone();
    output.ty = ty.clone();
    Plan {
        schema: vec![output],
        node: RelNode::Project {
            input: Box::new(query),
            expressions: vec![expression],
        },
    }
}

/// Construct the bound scalar without evaluating any query during binding.
fn query_expression(
    kind: SubqueryKind,
    query: Plan,
    left: Option<BoundExpr>,
    op: Option<BinaryOp>,
    ty: SqlType,
    nullable: bool,
) -> BoundExpr {
    BoundExpr {
        ty,
        nullable,
        node: ExprNode::Subquery {
            kind,
            query: Box::new(query),
            left: left.map(Box::new),
            op,
        },
    }
}
