//! Carry each distinct joined base occurrence behind the original visible column positions.
use super::*;

/// Every transformed relation preserves its visible prefix and appends one identity per origin.
pub(super) struct Traced {
    pub plan: Plan,
    pub origins: Vec<String>,
}

/// Trace only bag-preserving direct origins; no DISTINCT or aggregate may invent lock identity.
pub(super) fn augment(plan: &Plan, db: &Database, context: &Runtime<'_>) -> Result<Traced> {
    context.charge()?;
    match &plan.node {
        RelNode::Scan { table } => scan(plan, table, db, context),
        RelNode::Filter { input, .. }
        | RelNode::Project { input, .. }
        | RelNode::Order { input, .. }
        | RelNode::Unordered { input } => unary(plan, augment(input, db, context)?),
        RelNode::Join {
            kind: JoinKind::Inner,
            left,
            right,
            ..
        }
        | RelNode::Product { left, right } => binary(
            plan,
            left.schema.len(),
            right.schema.len(),
            augment(left, db, context)?,
            augment(right, db, context)?,
        ),
        _ => Err(error(
            "unsupported.lock_origin",
            "Operator cannot retain joined row identity",
        )),
    }
}

/// Number duplicate-valued physical occurrences before joins can multiply their visible rows.
fn scan(plan: &Plan, table: &str, db: &Database, context: &Runtime<'_>) -> Result<Traced> {
    let source = db
        .get(table)
        .ok_or_else(|| error("invalid.database", "Missing lock origin"))?;
    check_rows(source.len(), context)?;
    let rows = source
        .iter()
        .enumerate()
        .map(|(index, row)| {
            let mut row = row.clone();
            row.push(Cell::Int64(index as i64));
            row
        })
        .collect();
    let extra = vec![Column {
        name: "SQLolla joined occurrence".into(),
        ty: SqlType::Int64,
        nullable: false,
        qualifier: None,
        schema: None,
    }];
    traced(plan, RelNode::Values { rows }, &extra, vec![table.into()])
}

/// Filter and sort expressions keep original indexes; projection explicitly carries hidden cells.
fn unary(original: &Plan, child: Traced) -> Result<Traced> {
    let first = child.plan.schema.len() - child.origins.len();
    let extra = child.plan.schema[first..].to_vec();
    let node = unary_node(&original.node, child.plan, first)?;
    traced(original, node, &extra, child.origins)
}

/// Rebuild one unary node while keeping compiler-owned identities outside the SQL projection.
fn unary_node(original: &RelNode, input: Plan, first: usize) -> Result<RelNode> {
    Ok(match original {
        RelNode::Filter { predicate, .. } => RelNode::Filter {
            input: Box::new(input),
            predicate: predicate.clone(),
        },
        RelNode::Order { keys, .. } => RelNode::Order {
            input: Box::new(input),
            keys: keys.clone(),
        },
        RelNode::Unordered { .. } => RelNode::Unordered {
            input: Box::new(input),
        },
        RelNode::Project { expressions, .. } => {
            let mut expressions = expressions.clone();
            expressions.extend((first..input.schema.len()).map(|index| column(&input, index)));
            RelNode::Project {
                input: Box::new(input),
                expressions,
            }
        }
        _ => {
            return Err(error(
                "invalid.lock_trace",
                "Expected lineage-preserving unary operator",
            ))
        }
    })
}

/// Reorder joined hidden identities after visible cells so every parent expression stays unchanged.
fn binary(
    original: &Plan,
    left_width: usize,
    right_width: usize,
    left: Traced,
    right: Traced,
) -> Result<Traced> {
    let split = left.plan.schema.len();
    let mut origins = left.origins;
    origins.extend(right.origins);
    let joined = joined_plan(&original.node, left.plan, right.plan, left_width)?;
    let indexes: Vec<_> = (0..left_width)
        .chain(split..split + right_width)
        .chain(left_width..split)
        .chain(split + right_width..joined.schema.len())
        .collect();
    restore(joined, &indexes, origins)
}

/// Materialize the raw joined layout before its private cells are moved behind visible cells.
fn joined_plan(node: &RelNode, left: Plan, right: Plan, width: usize) -> Result<Plan> {
    let shift = left.schema.len() - width;
    let schema = left.schema.iter().chain(&right.schema).cloned().collect();
    let node = joined_node(node, left, right, width, shift)?;
    Ok(Plan { schema, node })
}

/// Project a permutation without changing multiplicity, ordering metadata or scalar identity.
fn restore(joined: Plan, indexes: &[usize], origins: Vec<String>) -> Result<Traced> {
    let expressions = indexes
        .iter()
        .map(|index| column(&joined, *index))
        .collect();
    let schema = indexes
        .iter()
        .map(|index| joined.schema[*index].clone())
        .collect();
    checked(Traced {
        plan: Plan {
            schema,
            node: RelNode::Project {
                input: Box::new(joined),
                expressions,
            },
        },
        origins,
    })
}

/// Shift only right visible ON columns across left hidden identities before restoring the layout.
fn joined_node(
    node: &RelNode,
    left: Plan,
    right: Plan,
    split: usize,
    shift: usize,
) -> Result<RelNode> {
    let left = Box::new(left);
    let right = Box::new(right);
    match node {
        RelNode::Product { .. } => Ok(RelNode::Product { left, right }),
        RelNode::Join { on, .. } => {
            let mut on = on.clone();
            shift_columns(&mut on, split, shift)?;
            Ok(RelNode::Join {
                kind: JoinKind::Inner,
                left,
                right,
                on,
            })
        }
        _ => Err(error(
            "invalid.lock_trace",
            "Expected inner join or product",
        )),
    }
}

/// Correlated ON subqueries need their own scope-aware remapping and cannot be silently shifted.
fn shift_columns(expr: &mut BoundExpr, split: usize, shift: usize) -> Result<()> {
    match &mut expr.node {
        ExprNode::Column(index) if *index >= split => *index += shift,
        ExprNode::Subquery { .. } => {
            return Err(error(
                "unsupported.lock_join_subquery",
                "ON subquery lineage needs scoped index remapping",
            ))
        }
        _ => (),
    }
    crate::rewrite::scalar_children(expr, &mut |child| shift_columns(child, split, shift))
}

/// Copy the exact source type and nullability for a compiler-owned projection position.
fn column(plan: &Plan, index: usize) -> BoundExpr {
    BoundExpr {
        ty: plan.schema[index].ty.clone(),
        nullable: plan.schema[index].nullable,
        node: ExprNode::Column(index),
    }
}

/// Retain original visible output metadata before appending private lineage positions.
fn traced(
    original: &Plan,
    node: RelNode,
    extra: &[Column],
    origins: Vec<String>,
) -> Result<Traced> {
    let schema = original.schema.iter().chain(extra).cloned().collect();
    checked(Traced {
        plan: Plan { schema, node },
        origins,
    })
}

/// Hidden identities participate in the same hard schema capacity as visible cells.
fn checked(value: Traced) -> Result<Traced> {
    if value.plan.schema.len() > 64 {
        return Err(error(
            "resource.lock_columns",
            "Joined occurrence identity exceeds column capacity",
        ));
    }
    Ok(value)
}
