//! Build one physical target tuple without evaluating an unused default or losing allocation effects.
use super::*;

/// Plan every applicable default before any source expression or proposal can allocate effects.
pub(super) fn admit(plan: &InsertPlan, context: &Runtime<'_>) -> Result<()> {
    let metadata = crate::context::mutation_metadata(&plan.target, context)?;
    for (index, column) in plan.target.schema.iter().enumerate() {
        let used = !plan.columns.contains(&index)
            || plan.default_columns.iter().any(|row| row.contains(&index));
        if used {
            default(metadata.defaults.get(&index), &column.ty, context)?;
        }
    }
    Ok(())
}

/// Scheduling sees applicable default effects even when their calls are outside the statement AST.
pub(super) fn ordered(plan: &InsertPlan, context: &Runtime<'_>) -> Result<bool> {
    let metadata = crate::context::mutation_metadata(&plan.target, context)?;
    let mut effects = false;
    for (index, column) in plan.target.schema.iter().enumerate() {
        let used = !plan.columns.contains(&index)
            || plan.default_columns.iter().any(|row| row.contains(&index));
        if used {
            effects |= scalar_effects::effectful(&default(
                metadata.defaults.get(&index),
                &column.ty,
                context,
            )?);
        }
    }
    Ok(effects)
}

/// Physical target positions determine expression evaluation and retain explicit DEFAULT markers.
pub(super) fn expressions(
    plan: &InsertPlan,
    source: &[BoundExpr],
    ordinal: usize,
    context: &Runtime<'_>,
) -> Result<Vec<BoundExpr>> {
    plan.target
        .schema
        .iter()
        .enumerate()
        .map(|(index, column)| {
            let value = column_value(plan, source, ordinal, index, context)?;
            Ok(BoundExpr {
                ty: column.ty.clone(),
                nullable: value.nullable,
                node: ExprNode::AssignmentCast {
                    arg: Box::new(value),
                },
            })
        })
        .collect()
}

/// Resolve a supplied expression or exactly one applicable default without evaluating either.
fn column_value(
    plan: &InsertPlan,
    source: &[BoundExpr],
    ordinal: usize,
    index: usize,
    context: &Runtime<'_>,
) -> Result<BoundExpr> {
    let defaults = plan
        .default_columns
        .get(ordinal)
        .map(Vec::as_slice)
        .unwrap_or(&[]);
    let supplied = plan
        .columns
        .iter()
        .position(|value| *value == index)
        .filter(|_| !defaults.contains(&index));
    if let Some(position) = supplied {
        return source
            .get(position)
            .cloned()
            .ok_or_else(|| error("invalid.insert_width", "Missing supplied column"));
    }
    let metadata = crate::context::mutation_metadata(&plan.target, context)?;
    default(
        metadata.defaults.get(&index),
        &plan.target.schema[index].ty,
        context,
    )
}

/// Default expressions share the same binder and branch-aware scalar execution as query expressions.
fn default(
    specification: Option<&DefaultSpec>,
    ty: &SqlType,
    context: &Runtime<'_>,
) -> Result<BoundExpr> {
    let mut expression = bound_default(specification, ty, context)?;
    crate::planning::expression(&mut expression)?;
    Ok(expression)
}

/// Bind only declared default metadata; planning and execution remain distinct later stages.
fn bound_default(
    specification: Option<&DefaultSpec>,
    ty: &SqlType,
    context: &Runtime<'_>,
) -> Result<BoundExpr> {
    match specification {
        None => Ok(crate::expression::literal(Cell::Null(ty.clone()))),
        Some(DefaultSpec::Literal { value }) => Ok(crate::expression::literal(value.clone())),
        Some(DefaultSpec::Expression {
            parser: Some(parser),
            ..
        }) => bind_metadata_context(
            parser,
            &[],
            context.functions.as_ref(),
            context.catalog_lookup.as_ref(),
        ),
        Some(DefaultSpec::Sequence { sequence }) => sequence_default(sequence, context),
        _ => Err(error(
            "unsupported.default_effect",
            "Default parser/context evidence is missing",
        )),
    }
}

/// A declared sequence default resolves to its actual object identity before allocating at runtime.
fn sequence_default(identity: &str, context: &Runtime<'_>) -> Result<BoundExpr> {
    let sequence = context
        .sequences
        .as_ref()
        .and_then(|value| value.sequences.get(identity))
        .ok_or_else(|| {
            error(
                "unsupported.sequence_context",
                "Default sequence metadata omitted",
            )
        })?;
    let value = scalar::input(&SqlType::Regclass, &sequence.oid.to_string())?;
    Ok(BoundExpr {
        ty: SqlType::Int64,
        nullable: false,
        node: ExprNode::Call {
            name: "nextval".into(),
            args: vec![crate::expression::literal(value)],
        },
    })
}
