//! Bind locking only when every returned occurrence retains unambiguous base-table origins.
use super::*;

impl Binder<'_> {
    /// Retain the complete locking modifier while binding a pure source for later acquisition.
    pub(super) fn locked_query(&self, node: &Value) -> Result<LockedQueryPlan> {
        self.locking_seen.set(true);
        let clause = locking_clause(node)?;
        let mut pure = node.clone();
        pure["locking"] = serde_json::json!([]);
        let query = self.locking_source(&pure)?;
        let schema = query.schema.clone();
        let (source, offset, limit) = separate_page(query);
        let mut tables = Vec::new();
        scans(&source, &mut tables)?;
        validate_origins(&tables, offset, limit)?;
        self.remember_initial_relations(&tables)?;
        let table = tables.remove(0);
        Ok(LockedQueryPlan {
            source,
            table,
            joined_tables: tables,
            clause,
            offset,
            limit,
            schema,
        })
    }

    /// Stored view expansion must retain WITH boundaries before outer row marks may propagate.
    fn locking_source(&self, pure: &Value) -> Result<Plan> {
        let mut scope = self.clone();
        scope.view_uses = Default::default();
        let result = scope.query(&normalize::query(pure)?, None);
        let views = scope.view_uses.borrow();
        self.view_uses.borrow_mut().extend(views.iter().cloned());
        if !views.is_empty() {
            return Err(error(
                "unsupported.lock_view_origin",
                "View row marks require preserved query boundaries",
            ));
        }
        result
    }
}

/// Reject ambiguous origins and unknown targets before removing any modifier from syntax.
fn locking_clause(node: &Value) -> Result<RowLockClause> {
    let from = direct_source(node)?;
    let [clause] = array(node, "locking")?.as_slice() else {
        return Err(error(
            "unsupported.lock_clauses",
            "Multiple clauses require target-wise strength merging",
        ));
    };
    fields(clause, &["strength", "targets", "wait_policy"])?;
    validate_targets(from, clause)?;
    serde_json::from_value(
        serde_json::json!({"strength":clause["strength"],"wait_policy":clause["wait_policy"]}),
    )
    .map_err(|_| {
        error(
            "unsupported.lock_clause",
            "Unknown row lock strength or policy",
        )
    })
}

/// A direct base relation gives every output occurrence a unique lock origin.
fn direct_source(node: &Value) -> Result<&Value> {
    if !array(node, "ctes")?.is_empty() || text(&node["body"], "tag")? != "select" {
        return Err(error(
            "unsupported.lock_origin",
            "Locking CTE/set-query origins need an explicit provenance model",
        ));
    }
    let from = &node["body"]["source"];
    if !matches!(text(from, "tag")?, "table" | "join" | "product") {
        return Err(error(
            "unsupported.lock_origin",
            "Locking requires direct base-relation origins",
        ));
    }
    Ok(from)
}

/// OF targets use the visible alias rather than a hidden original relation name.
fn validate_targets(from: &Value, clause: &Value) -> Result<()> {
    if text(from, "tag")? != "table" {
        return if array(clause, "targets")?.is_empty() {
            Ok(())
        } else {
            Err(error(
                "unsupported.lock_targets",
                "Joined OF targets need per-origin clauses",
            ))
        };
    }
    let visible = from["alias"].as_str().unwrap_or(text(from, "name")?);
    if array(clause, "targets")?
        .iter()
        .any(|target| target.as_str() != Some(visible))
    {
        return Err(error(
            "invalid.lock_target",
            "OF target is not the visible base-relation name",
        ));
    }
    Ok(())
}

/// Pagination consumes the locked stream, including OFFSET rows, instead of an eager bag.
fn separate_page(plan: Plan) -> (Plan, usize, Option<usize>) {
    match plan {
        Plan {
            node:
                RelNode::Slice {
                    input,
                    offset,
                    limit,
                },
            ..
        } => (*input, offset, limit),
        other => (other, 0, None),
    }
}

/// Allowed operators retain exactly one original row identity through filtering and projection.
fn scans(plan: &Plan, tables: &mut Vec<String>) -> Result<()> {
    match &plan.node {
        RelNode::Scan { table } => {
            tables.push(table.clone());
            Ok(())
        }
        RelNode::Filter { input, .. }
        | RelNode::Project { input, .. }
        | RelNode::Order { input, .. }
        | RelNode::Unordered { input } => scans(input, tables),
        RelNode::Join {
            kind: JoinKind::Inner,
            left,
            right,
            ..
        }
        | RelNode::Product { left, right } => {
            scans(left, tables)?;
            scans(right, tables)
        }
        _ => Err(error(
            "unsupported.lock_origin",
            "This operator needs additional lock-row provenance",
        )),
    }
}

/// Distinct table origins and unpaginated joins avoid inventing partial row-mark acquisition order.
fn validate_origins(tables: &[String], offset: usize, limit: Option<usize>) -> Result<()> {
    let unique: std::collections::BTreeSet<_> = tables.iter().collect();
    if tables.is_empty() || unique.len() != tables.len() {
        return Err(error(
            "unsupported.lock_self_join",
            "Lock aliases need separate physical origin tracking",
        ));
    }
    if tables.len() > 1 && (offset > 0 || limit.is_some_and(|count| count > 0)) {
        return Err(error(
            "unsupported.lock_join_pagination",
            "Joined lock prefixes need partial-acquisition alternatives",
        ));
    }
    Ok(())
}
