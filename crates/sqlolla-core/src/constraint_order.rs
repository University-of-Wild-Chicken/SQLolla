//! Physical uniqueness order is supplied metadata, never inferred from constraint kind.
use crate::json::error;
use crate::*;
use std::collections::BTreeSet;
#[cfg(test)]
mod tests;

/// One physical uniqueness check, retaining partial membership and primary NULL behavior.
pub(crate) enum Rule<'a> {
    /// Primary or ordinary unique constraint.
    Constraint(&'a KeyConstraint, bool),
    /// Standalone unique index.
    Index(&'a UniqueIndex),
}

impl Rule<'_> {
    /// Diagnostic names identify ordered rules within one table.
    pub(crate) fn key(&self) -> &KeyConstraint {
        match self {
            Self::Constraint(key, _) => key,
            Self::Index(index) => &index.key,
        }
    }
    /// Execute one rule in the supplied physical order.
    fn check(&self, rows: &Bag, schema: &[Column], context: &ExecutionContext) -> Result<()> {
        match self {
            Self::Constraint(key, primary) => crate::constraints::unique(rows, key, *primary),
            Self::Index(index) => crate::unique_index::validate_one(rows, index, schema, context),
        }
    }
}

/// Build a complete, identity-validated ordered list including deferred rules.
pub(crate) fn rules(metadata: &TableSemantics) -> Result<Vec<Rule<'_>>> {
    let mut rules = metadata
        .primary_key
        .iter()
        .map(|key| Rule::Constraint(key, true))
        .chain(
            metadata
                .unique
                .iter()
                .map(|key| Rule::Constraint(key, false)),
        )
        .chain(metadata.unique_indexes.iter().map(Rule::Index))
        .collect::<Vec<_>>();
    let order = &metadata.unique_enforcement_order;
    validate_names(&rules, order)?;
    if !order.is_empty() {
        rules.sort_by_key(|rule| order.iter().position(|name| name == &rule.key().name));
    }
    Ok(rules)
}

/// Supplied order is complete and unambiguous across every uniqueness rule kind.
fn validate_names(rules: &[Rule<'_>], order: &[String]) -> Result<()> {
    let names = rules
        .iter()
        .map(|rule| &rule.key().name)
        .collect::<BTreeSet<_>>();
    if names.len() != rules.len()
        || (!order.is_empty()
            && (order.len() != rules.len() || order.iter().collect::<BTreeSet<_>>() != names))
    {
        return Err(error(
            "invalid.constraint_order",
            "Uniqueness identities/order must list each declared rule exactly once",
        ));
    }
    Ok(())
}

/// Stop at the first ordered rejection; absent order is accepted only if all orders agree.
pub(crate) fn immediate(
    rows: &Bag,
    metadata: &TableSemantics,
    schema: &[Column],
    context: &ExecutionContext,
) -> Result<()> {
    let mut failure = None;
    for rule in rules(metadata)?
        .into_iter()
        .filter(|rule| !rule.key().deferrable)
    {
        if let Err(error) = rule.check(rows, schema, context) {
            if !metadata.unique_enforcement_order.is_empty() {
                return Err(error);
            }
            if failure.is_some() {
                return Err(crate::json::error(
                    "unsupported.constraint_order",
                    "Competing uniqueness failures require physical index order",
                ));
            }
            failure = Some(error);
        }
    }
    failure.map_or(Ok(()), Err)
}
