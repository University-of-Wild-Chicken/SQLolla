//! Physical relation opening is collected before pruning and scalar planning.
use super::*;

impl Binder<'_> {
    /// Preserve only resolved physical identities; CTE scans cannot become lock targets.
    pub(super) fn remember_initial_relations(&self, tables: &[String]) -> Result<()> {
        if tables
            .iter()
            .any(|table| !self.catalog.tables.contains_key(table))
        {
            return Err(error(
                "unsupported.lock_origin",
                "Lock target is not a physical relation",
            ));
        }
        self.initial_relations
            .borrow_mut()
            .extend(tables.iter().cloned());
        Ok(())
    }

    /// A planner failure retains opening obligations already established during binding.
    pub(super) fn initialized_statement(
        &self,
        result: Result<StatementPlan>,
    ) -> Result<StatementPlan> {
        let bound = match result {
            Ok(plan) => plan,
            Err(failure) => return self.binding_failure(failure),
        };
        let planned = match crate::planning::statement(bound) {
            Ok(plan) => plan,
            Err(failure) => rejected_statement(failure)?,
        };
        let relations: Vec<_> = self.initial_relations.borrow().iter().cloned().collect();
        if relations.is_empty() {
            return Ok(planned);
        }
        Ok(StatementPlan::Initialized {
            relations,
            body: Box::new(planned),
        })
    }

    /// Binding-error precedence needs parse-analysis opening order, beyond this resolved profile.
    fn binding_failure(&self, failure: Error) -> Result<StatementPlan> {
        if self.locking_seen.get() && (failure.sql.is_some() || failure.code.starts_with("sql.")) {
            return Err(error(
                "unsupported.initialization_binding",
                "Relation-opening precedence for a failed binding is not modeled",
            ));
        }
        rejected_statement(failure)
    }
}
