//! Nested queries bind against immutable, explicitly ordered lexical outer scopes.
use super::*;
use crate::expression::QueryScope;
use crate::json::optional;

impl QueryScope for Binder<'_> {
    /// Cast names remain available after a projection has hidden every nominal-typed column.
    fn nominal_type(&self, name: &str) -> Result<Option<SqlType>> {
        super::nominal::resolve(self.catalog, name)
    }
    /// Preserve CTE visibility and shared binding fuel while adding one correlation scope.
    fn bind_query(&self, node: &Value, current_columns: &[Column]) -> Result<Plan> {
        if self.outer.len() >= 16 {
            return Err(error(
                "resource.correlation_depth",
                "Subquery nesting exceeds 16 scopes",
            ));
        }
        let mut child = self.clone();
        child.outer.insert(0, current_columns.to_vec());
        child.scoped_query(node)
    }

    /// A visible local alias hides an outer alias even when its column is absent.
    fn outer_column(&self, node: &Value, current_columns: &[Column]) -> Result<Option<BoundExpr>> {
        fields(node, &["tag", "name", "qualifier", "schema"])?;
        let qualifier = optional(node, "qualifier")?;
        let schema = optional(node, "schema")?;
        if qualifier.is_some() && visible(current_columns, &qualifier, &schema) {
            return Ok(None);
        }
        for (depth, columns) in self.outer.iter().enumerate() {
            if qualifier.is_some() && !visible(columns, &qualifier, &schema) {
                continue;
            }
            let matches = positions(columns, text(node, "name")?, &qualifier, &schema);
            match matches.as_slice() {
                [index] => return Ok(Some(outer_reference(depth, *index, &columns[*index]))),
                [] if qualifier.is_none() => continue,
                [] => {
                    return Err(error(
                        "sql.42703",
                        "Column does not exist in visible outer relation",
                    ));
                }
                _ => return Err(error("sql.42702", "Outer column reference is ambiguous")),
            }
        }
        Ok(None)
    }
}

impl Binder<'_> {
    /// Preserve locked sublinks even when scalar planning later removes their row demand.
    fn scoped_query(&self, node: &Value) -> Result<Plan> {
        if array(node, "locking")?.is_empty() {
            return self.query(&normalize::query(node)?, None);
        }
        let locked = self.locked_query(node)?;
        Ok(Plan {
            schema: locked.schema.clone(),
            node: RelNode::Locked(Box::new(locked)),
        })
    }
}

/// Qualification is exact; aliases suppress the original schema/name pair.
fn visible(columns: &[Column], qualifier: &Option<String>, schema: &Option<String>) -> bool {
    columns.iter().any(|column| {
        qualifier
            .as_ref()
            .is_none_or(|q| column.qualifier.as_ref() == Some(q))
            && schema
                .as_ref()
                .is_none_or(|s| column.schema.as_ref() == Some(s))
    })
}

/// Resolve matching positions without conflating duplicate output labels.
fn positions(
    columns: &[Column],
    name: &str,
    qualifier: &Option<String>,
    schema: &Option<String>,
) -> Vec<usize> {
    columns
        .iter()
        .enumerate()
        .filter(|(_, column)| {
            column.name == name
                && (qualifier.is_some() || crate::expression::visible_column(column))
                && qualifier
                    .as_ref()
                    .is_none_or(|q| column.qualifier.as_ref() == Some(q))
                && schema
                    .as_ref()
                    .is_none_or(|s| column.schema.as_ref() == Some(s))
        })
        .map(|(index, _)| index)
        .collect()
}

/// Depth zero addresses the immediate outer row; further depths remain explicit.
fn outer_reference(depth: usize, index: usize, column: &Column) -> BoundExpr {
    BoundExpr {
        ty: column.ty.clone(),
        nullable: column.nullable,
        node: ExprNode::OuterColumn { depth, index },
    }
}

/// EXISTS target pruning is limited to verified wrappers over ordinary relational sources.
pub(crate) fn prepare_exists(plan: Plan) -> Result<Plan> {
    let result = match plan.node {
        RelNode::Project { input, .. }
        | RelNode::Order { input, .. }
        | RelNode::Distinct { input } => prepare_exists(*input)?,
        RelNode::Slice { .. } | RelNode::Aggregate { .. } | RelNode::Set { .. } => {
            return Err(error(
                "unsupported.exists_shape",
                "EXISTS aggregate/set/pagination needs separate planning rules",
            ));
        }
        _ => plan,
    };
    check_exists_source(&result)?;
    Ok(result)
}

/// Derived aggregate or set sources need their own projection-pruning proof.
fn check_exists_source(plan: &Plan) -> Result<()> {
    match &plan.node {
        RelNode::Filter { input, .. }
        | RelNode::Distinct { input }
        | RelNode::Order { input, .. }
        | RelNode::Unordered { input } => check_exists_source(input),
        RelNode::Project { input, expressions } => {
            simple_exists_values(expressions)?;
            check_exists_source(input)
        }
        RelNode::Product { left, right } | RelNode::Join { left, right, .. } => {
            check_exists_source(left)?;
            check_exists_source(right)
        }
        RelNode::EvaluateValues { rows } => {
            for row in rows {
                simple_exists_values(row)?;
            }
            Ok(())
        }
        RelNode::Values { .. } | RelNode::Scan { .. } => Ok(()),
        _ => Err(error(
            "unsupported.exists_shape",
            "EXISTS source needs additional planning semantics",
        )),
    }
}

/// Unused derived/VALUES expressions need pruning semantics unless they are already total leaves.
fn simple_exists_values(expressions: &[BoundExpr]) -> Result<()> {
    if expressions.iter().all(|expression| {
        matches!(
            expression.node,
            ExprNode::Literal(_) | ExprNode::Column(_) | ExprNode::OuterColumn { .. }
        )
    }) {
        return Ok(());
    }
    Err(error(
        "unsupported.exists_projection",
        "Derived EXISTS targets require additional pruning rules",
    ))
}
