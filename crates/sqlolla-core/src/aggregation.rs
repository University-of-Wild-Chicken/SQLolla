//! Explicit aggregate operators; no function name is resolved during execution.
use crate::{BoundExpr, SqlType};
use serde::{Deserialize, Serialize};

/// Admitted aggregate families with distinct NULL and return-type rules.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum AggregateFunction {
    /// Count all occurrences or non-NULL argument occurrences.
    Count,
    /// Exact sum of non-NULL numeric values.
    Sum,
    /// Exact numeric sum divided by its non-NULL occurrence count.
    Avg,
    /// Least non-NULL value under the declared SQL comparison profile.
    Min,
    /// Greatest non-NULL value under the declared SQL comparison profile.
    Max,
}

/// Bound aggregate inputs; FILTER precedes argument evaluation and DISTINCT.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AggregateExpr {
    /// Resolved built-in aggregate behavior.
    pub function: AggregateFunction,
    /// None denotes COUNT(*); every other aggregate requires an argument.
    pub argument: Option<BoundExpr>,
    /// Apply SQL duplicate equality after removing NULL argument values.
    pub distinct: bool,
    /// Optional Boolean predicate evaluated against each original input row.
    pub filter: Option<BoundExpr>,
    /// PostgreSQL aggregate result type, independent of argument storage typmods.
    pub ty: SqlType,
}
