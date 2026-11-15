//! Ranking windows retain one shared ordering choice across every repeated function reference.
use crate::{BoundExpr, OrderKey};
use serde::{Deserialize, Serialize};

/// Integral ranking builtins whose peer and occurrence semantics are explicitly modeled.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WindowFunction {
    /// One-based position in a complete chosen partition ordering.
    RowNumber,
    /// One plus the number of rows preceding the current peer group.
    Rank,
    /// One plus the number of peer groups preceding the current peer group.
    DenseRank,
}

/// One partition/order specification shared by all admitted ranking slots in a SELECT scope.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WindowSpec {
    /// SQL duplicate-equivalence keys; NULL values belong to one partition class.
    pub partition_by: Vec<BoundExpr>,
    /// Lexicographic ordering with explicit NULL placement and direction.
    pub order_by: Vec<OrderKey>,
    /// Deduplicated output slots in first-reference order.
    pub functions: Vec<WindowFunction>,
}
