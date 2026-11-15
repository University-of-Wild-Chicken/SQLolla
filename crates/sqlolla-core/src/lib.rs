//! Deterministic binding and bag interpretation of an admitted SQLeanParser AST.
//! This crate contains no SQL text parser, database I/O or solver invocation.
#![warn(missing_docs)]

mod aggregation;
mod backend_signal;
mod binding;
mod catalog_lookup;
mod comparison;
mod constraint_order;
mod constraints;
mod context;
mod cte;
mod ddl;
mod domain;
mod effects;
mod evaluate;
mod execution;
mod expression;
mod function_registry;
mod json;
mod metadata;
mod outcome;
mod pattern;
mod planning;
mod rewrite;
mod row_eval;
pub mod scalar;
mod scalar_eval;
pub mod sequence;
mod session_effect;
#[cfg(test)]
mod session_effect_tests;
mod sql_function;
mod systemcolumns;
mod unique_index;
mod value;
#[cfg(test)]
mod value_tests;
mod window;
pub use aggregation::*;
pub use backend_signal::{
    evaluate_backend_signal, validate_backend_signal_context, BackendSignalContext,
    BackendSignalTarget,
};
pub use binding::{
    bind, bind_metadata_context, bind_metadata_expression, bind_metadata_functions, bind_statement,
    bind_statement_with_source,
};
pub use catalog_lookup::{CatalogLookup, RelationIdentity};
pub use context::{admissible_initial_state, validate_execution_context};
pub use ddl::*;
pub use domain::*;
pub use effects::*;
pub use evaluate::{eval, eval_expr, sql_bag_equal, sql_row_equal, validate_database};
pub use execution::{
    evaluate_statement, evaluate_with_catalog, merged_catalog_context, MAX_EXECUTION_STEPS,
    MAX_INTERMEDIATE_ROWS,
};
pub use metadata::*;
pub use outcome::*;
pub use sequence::{SequenceAllocation, SequenceContext, SequenceSnapshot, SequenceState};
pub use session_effect::{
    delivered_notifications, evaluate_session_effect, is_session_effect,
    validate_session_effect_context, AdvisoryHeld, AdvisoryKey, ScalarEffectStep,
    SessionEffectContext,
};
pub use sql_function::{ImmutableSqlFunction, SqlFunctions};
pub use systemcolumns::*;
pub use value::*;
pub use window::*;
