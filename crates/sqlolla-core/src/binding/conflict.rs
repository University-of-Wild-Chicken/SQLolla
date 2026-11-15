//! Conflict target syntax and EXCLUDED scope are explicit, independent of ordinary INSERT values.
use super::*;

impl Binder<'_> {
    /// Bind conflict actions without guessing missing unique indexes from their column spelling.
    pub(super) fn conflict(
        &self,
        node: &Value,
        target: &MutationTarget,
    ) -> Result<Option<ConflictPlan>> {
        if node.is_null() {
            return Ok(None);
        }
        fields(node, &["target", "action"])?;
        let inference = self.conflict_target(&node["target"], &target.schema)?;
        let action = match text(&node["action"], "tag")? {
            "nothing" => {
                fields(&node["action"], &["tag"])?;
                ConflictActionPlan::Nothing
            }
            "update" => self.conflict_update(&node["action"], target)?,
            tag => return Err(error("unsupported.conflict_action", tag)),
        };
        if inference == ConflictTarget::Any && matches!(action, ConflictActionPlan::Update { .. }) {
            return Err(error(
                "sql.42601",
                "ON CONFLICT UPDATE requires an inference target",
            ));
        }
        Ok(Some(ConflictPlan {
            target: inference,
            action,
        }))
    }

    /// EXCLUDED is a second typed row scope; it never aliases the existing target occurrence.
    fn conflict_update(&self, node: &Value, target: &MutationTarget) -> Result<ConflictActionPlan> {
        fields(node, &["tag", "assignments", "where"])?;
        let mut columns = target.schema.clone();
        columns.extend(target.schema.iter().cloned().map(|mut column| {
            column.qualifier = Some("excluded".into());
            column.schema = None;
            column
        }));
        let assignments = self.assignments(array(node, "assignments")?, &columns)?;
        let predicate = if node["where"].is_null() {
            crate::expression::literal(Cell::Bool(true))
        } else {
            self.expressions(&columns)
                .bind(&node["where"], Some(SqlType::Bool))?
        };
        Ok(ConflictActionPlan::Update {
            assignments,
            predicate: Box::new(predicate),
        })
    }
}

impl Binder<'_> {
    /// Retain partial-index inference predicates independently from DO UPDATE's row predicate.
    fn conflict_target(&self, node: &Value, schema: &[Column]) -> Result<ConflictTarget> {
        if node.is_null() {
            return Ok(ConflictTarget::Any);
        }
        match text(node, "tag")? {
            "constraint" => {
                fields(node, &["tag", "name"])?;
                Ok(ConflictTarget::Constraint(text(node, "name")?.into()))
            }
            "columns" => {
                let columns = inference_columns(node, schema)?;
                if columns.is_empty() {
                    return Err(error("sql.42601", "Empty conflict inference"));
                }
                if node["where"].is_null() {
                    Ok(ConflictTarget::Columns(columns))
                } else {
                    Ok(ConflictTarget::PredicateColumns {
                        columns,
                        predicate: Box::new(
                            self.expressions(schema)
                                .bind(&node["where"], Some(SqlType::Bool))?,
                        ),
                    })
                }
            }
            tag => Err(error("unsupported.conflict_target", tag)),
        }
    }
}

/// Resolve all key positions and retain the predicate for the separately scoped binder.
fn inference_columns(node: &Value, schema: &[Column]) -> Result<Vec<usize>> {
    if node.get("where").is_some() {
        fields(node, &["tag", "names", "where"])?;
    } else {
        fields(node, &["tag", "names"])?;
    }
    array(node, "names")?
        .iter()
        .map(|value| {
            let name = value
                .as_str()
                .ok_or_else(|| error("invalid.conflict_column", "Column must be text"))?;
            super::mutation::column_index(name, schema)
        })
        .collect()
}
