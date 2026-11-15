//! Complete finite statement evaluation; resource refusal never truncates behavior.
mod conflict;
mod cte_consumption;
mod cte_demand;
mod cte_lineage;
mod cte_schedule;
mod ddl;
pub use ddl::{evaluate_with_catalog, merged_catalog_context};
mod distinct_on;
mod effects;
mod grounded;
mod grouping;
mod index_arbiter;
mod initialization;
mod insert_defaults;
mod insert_effects;
mod joined_mutation;
mod lock_demand;
mod lock_join;
mod lock_lineage;
mod lock_orders;
mod lock_source;
mod mutation;
mod ordering;
mod rel_effects;
mod relational;
mod rows_effects;
mod runtime;
mod scalar_effects;
mod sequence_calls;
mod set_returning;
mod subquery;
mod tuple_effects;
mod window;
mod with_statement;
use runtime::Runtime;
pub use runtime::MAX_EXECUTION_STEPS;
/// Maximum explicitly admitted intermediate rows; the default remains 256 occurrences.
pub const MAX_INTERMEDIATE_ROWS: usize = 4096;
pub(crate) mod session;
use crate::json::error;
use crate::*;

/// Evaluate one scalar with shared statement-local fuel and explicit database context.
fn scalar(expr: &BoundExpr, row: &Row, db: &Database, context: &Runtime<'_>) -> Result<Cell> {
    scalar_effects::pure(expr, row, db, context)
}

/// One scalar branch returns only events produced by this invocation, excluding its inherited input.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct ScalarObservation {
    pub(super) value: Cell,
    pub(super) effects: Vec<EffectEvent>,
}

/// SQL failure preserves the local nontransactional delta; admission/resource errors stay outside.
pub(super) type ScalarOutcome = std::result::Result<ScalarObservation, Box<SqlFailure>>;

/// Evaluate every scalar alternative with explicit inherited state and lazy branch selection.
fn scalar_observations(
    expr: &BoundExpr,
    row: &Row,
    db: &Database,
    context: &Runtime<'_>,
) -> Result<Vec<ScalarOutcome>> {
    scalar_effects::evaluate(expr, row, db, context)
}

/// One complete relational observation before statement-level state is attached.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct RelationResult {
    rows: Bag,
    ordering: ResultOrdering,
    effects: Vec<EffectEvent>,
}

/// SQL failures are alternatives; resource and coverage failures remain outer errors.
type RelationOutcome = std::result::Result<RelationResult, Box<SqlFailure>>;

impl Default for ExecutionContext {
    /// Default limits are explicit finite capacities, not truncation requests.
    fn default() -> Self {
        Self {
            tuples: None,
            sequences: None,
            session_effects: None,
            backend_signals: None,
            functions: None,
            max_outcomes: 256,
            max_rows: 256,
            settings: Default::default(),
            table_metadata: Default::default(),
            locks: None,
            catalog_lookup: None,
        }
    }
}

/// Evaluate every admitted statement behavior or refuse an incomplete enumeration.
pub fn evaluate_statement(
    plan: &StatementPlan,
    database: &Database,
    context: &ExecutionContext,
) -> Result<OutcomeSet> {
    validate_context(context)?;
    database
        .values()
        .flatten()
        .flatten()
        .try_for_each(Cell::validate)?;
    if let StatementPlan::Rejected { failure } = plan {
        return Ok(OutcomeSet {
            outcomes: vec![StatementOutcome::SqlError(failure.clone())],
        });
    }
    evaluate_resolved(plan, database, &Runtime::new(context))
}

/// Evaluate a statement after its stable context references have been validated.
fn evaluate_resolved(
    plan: &StatementPlan,
    database: &Database,
    context: &Runtime<'_>,
) -> Result<OutcomeSet> {
    if let StatementPlan::Initialized { relations, body } = plan {
        return initialization::execute(relations, body, database, context);
    }
    if let StatementPlan::Rejected { failure } = plan {
        return Ok(OutcomeSet {
            outcomes: vec![StatementOutcome::SqlError(failure.clone())],
        });
    }
    if let StatementPlan::With(plan) = plan {
        return with_statement::execute(plan, database, context);
    }
    if let StatementPlan::LockedQuery(plan) = plan {
        return effects::execute(plan, database, context);
    }
    let StatementPlan::Query(query) = plan else {
        return mutation::execute(plan, database, context);
    };
    query_outcomes(query, database, context)
}

/// Attach unchanged database state and SELECT command tags to complete relational outcomes.
fn query_outcomes(query: &Plan, database: &Database, context: &Runtime<'_>) -> Result<OutcomeSet> {
    let results = relational::query(query, database, context)?;
    let outcomes = results
        .into_iter()
        .map(|result| match result {
            Ok(result) => StatementOutcome::Succeeded(StatementSuccess {
                catalog: None,
                database: database.clone(),
                affected: result.rows.len() as u64,
                command_tag: format!("SELECT {}", result.rows.len()),
                rows: result.rows,
                ordering: result.ordering,
                effects: result.effects,
            }),
            Err(failure) => StatementOutcome::SqlError(*failure),
        })
        .collect();
    Ok(OutcomeSet { outcomes })
}

/// Refuse unbounded or empty resource declarations before recursive evaluation.
pub(crate) fn validate_context(context: &ExecutionContext) -> Result<()> {
    if let Some(backends) = &context.backend_signals {
        crate::validate_backend_signal_context(backends)?;
    }
    if let Some(session) = &context.session_effects {
        crate::validate_session_effect_context(session)?;
    }
    if !(1..=256).contains(&context.max_outcomes)
        || !(1..=MAX_INTERMEDIATE_ROWS).contains(&context.max_rows)
    {
        return Err(error(
            "resource.execution_context",
            "Outcome limit must be 1..256 and row limit 1..4096",
        ));
    }
    context.settings.values().try_for_each(Cell::validate)?;
    for default in context
        .table_metadata
        .values()
        .flat_map(|table| table.defaults.values())
    {
        if let DefaultSpec::Literal { value } = default {
            value.validate()?;
        }
    }
    Ok(())
}

/// Add a distinct behavior only if the entire resulting set fits the declared capacity.
fn push<T: PartialEq>(values: &mut Vec<T>, value: T, context: &Runtime<'_>) -> Result<()> {
    if !values.contains(&value) {
        if values.len() >= context.max_outcomes {
            return Err(error(
                "resource.outcome_count",
                "Complete outcome enumeration exceeds capacity",
            ));
        }
        values.push(value);
    }
    Ok(())
}

/// Enforce every intermediate relation's occurrence capacity without silently slicing.
fn check_rows(count: usize, context: &Runtime<'_>) -> Result<()> {
    if count > context.max_rows {
        return Err(error(
            "resource.row_count",
            "Complete intermediate relation exceeds capacity",
        ));
    }
    Ok(())
}

/// Promote only explicitly classified SQL failures into execution outcomes.
fn execution_error(failure: Error) -> Result<RelationOutcome> {
    if let Some(sql) = failure.sql {
        return Ok(Err(sql));
    }
    let Some(sqlstate) = failure.code.strip_prefix("sql.") else {
        return Err(failure);
    };
    Ok(Err(Box::new(SqlFailure {
        sqlstate: sqlstate.into(),
        phase: FailurePhase::Statement,
        constraint_name: None,
        column_name: if sqlstate == "23502" {
            Some(failure.message.clone())
        } else {
            None
        },
        exposed_rows: Vec::new(),
        effects: Vec::new(),
    })))
}

/// Normalize only unordered observations; sorting ordered rows would erase behavior.
fn result(mut rows: Bag, ordering: ResultOrdering) -> RelationOutcome {
    if ordering == ResultOrdering::Bag {
        rows.sort();
    }
    Ok(RelationResult {
        rows,
        ordering,
        effects: Vec::new(),
    })
}
