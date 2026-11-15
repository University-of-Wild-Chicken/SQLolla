//! Append private input occurrence ordinals without changing any visible SQL expression index.
use super::*;

/// Replace a sole base scan with numbered concrete rows and propagate its private column.
pub(super) fn augment(
    plan: &Plan,
    table: &str,
    db: &Database,
    context: &Runtime<'_>,
) -> Result<Plan> {
    if plan.schema.len() >= 64 {
        return Err(error(
            "resource.lock_columns",
            "Private lock identity exceeds column capacity",
        ));
    }
    let node = augmented_node(plan, table, db, context)?;
    let mut schema = plan.schema.clone();
    schema.push(identity_column());
    Ok(Plan { schema, node })
}

/// Rewrite one lineage-preserving operator while retaining every original expression index.
fn augmented_node(
    plan: &Plan,
    table: &str,
    db: &Database,
    context: &Runtime<'_>,
) -> Result<RelNode> {
    Ok(match &plan.node {
        RelNode::Scan { table: target } if target == table => numbered(target, db, context)?,
        RelNode::Filter { input, predicate } => RelNode::Filter {
            input: Box::new(augment(input, table, db, context)?),
            predicate: predicate.clone(),
        },
        RelNode::Order { input, keys } => RelNode::Order {
            input: Box::new(augment(input, table, db, context)?),
            keys: keys.clone(),
        },
        RelNode::Project { input, expressions } => project(input, expressions, table, db, context)?,
        RelNode::Unordered { input } => RelNode::Unordered {
            input: Box::new(augment(input, table, db, context)?),
        },
        _ => {
            return Err(error(
                "unsupported.lock_origin",
                "Lock identity cannot traverse this operator",
            ));
        }
    })
}

/// Duplicate-valued rows receive separate occurrence identities before filtering.
fn numbered(table: &str, db: &Database, context: &Runtime<'_>) -> Result<RelNode> {
    let input = db
        .get(table)
        .ok_or_else(|| error("invalid.database", "Lock source is absent"))?;
    check_rows(input.len(), context)?;
    let rows = input
        .iter()
        .enumerate()
        .map(|(index, row)| {
            let mut row = row.clone();
            row.push(Cell::Int64(index as i64));
            row
        })
        .collect();
    Ok(RelNode::Values { rows })
}

/// Ordinary projection cannot hide the compiler-owned identity from the locking boundary.
fn project(
    input: &Plan,
    expressions: &[BoundExpr],
    table: &str,
    db: &Database,
    context: &Runtime<'_>,
) -> Result<RelNode> {
    let mut expressions = expressions.to_vec();
    expressions.push(BoundExpr {
        ty: SqlType::Int64,
        nullable: false,
        node: ExprNode::Column(input.schema.len()),
    });
    Ok(RelNode::Project {
        input: Box::new(augment(input, table, db, context)?),
        expressions,
    })
}

/// Internal output metadata is stripped before exposing a user result schema.
fn identity_column() -> Column {
    Column {
        name: "SQLolla occurrence identity".into(),
        ty: SqlType::Int64,
        nullable: false,
        qualifier: None,
        schema: None,
    }
}
