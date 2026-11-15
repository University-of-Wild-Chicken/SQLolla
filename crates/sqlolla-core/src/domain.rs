//! Pure data contracts shared by binding, interpretation and Alloy encoding.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

use crate::{Cell, SqlType};

/// Ordered cells within one row occurrence.
pub type Row = Vec<Cell>;
/// Unordered row occurrences; identical rows remain separate occurrences.
pub type Bag = Vec<Row>;
/// Database data keyed by stable catalog table identifiers.
pub type Database = BTreeMap<String, Bag>;
/// Explicit numbered parameters; typed NULLs must declare their resolved type.
pub type Parameters = BTreeMap<u32, Cell>;

/// A named and typed output position with optional binding provenance.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Column {
    /// Resolved, case-sensitive output label.
    pub name: String,
    /// SQL scalar family.
    pub ty: SqlType,
    /// Whether a valid output cell may be NULL.
    pub nullable: bool,
    /// Visible table name or alias within the current binding scope.
    #[serde(default)]
    pub qualifier: Option<String>,
    /// Visible schema; aliases deliberately hide this qualifier.
    #[serde(default)]
    pub schema: Option<String>,
}

/// Catalog metadata; the enclosing map key is the stable table identifier.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Table {
    /// Exact schema name.
    pub schema: String,
    /// Exact relation name, separate from the stable identifier.
    pub name: String,
    /// Ordered physical columns and their declared types.
    pub columns: Vec<Column>,
}

/// Closed catalog and explicit PostgreSQL-style schema search order.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Catalog {
    /// Observed PostgreSQL creation namespace, separate from effective lookup search order.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub creation_schema: Option<crate::CreationSchema>,
    /// Complete relation identities used by bind-time object-reference input conversion.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub relation_lookup: Option<Box<crate::CatalogLookup>>,
    /// Declared immutable scalar SQL bodies and their exact lexical search path.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub functions: Option<crate::SqlFunctions>,
    /// Complete namespace evidence when catalog-changing commands are admitted.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub namespace: Option<crate::CatalogNamespace>,
    /// Stored view definitions retain exact source and receive trusted parser preparation.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub views: BTreeMap<String, crate::ViewDefinition>,
    /// Persisted table behavior created by DDL; explicit execution metadata must agree.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub table_semantics: BTreeMap<String, crate::TableSemantics>,
    /// Search order for unqualified table references.
    pub search_path: Vec<String>,
    /// Stable identifiers mapped to unique schema/name definitions.
    pub tables: BTreeMap<String, Table>,
}

/// A bound relation with an ordered output schema.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Plan {
    /// Ordered type/nullability metadata for every result position.
    pub schema: Vec<Column>,
    /// Bound operator and its already-resolved inputs.
    pub node: RelNode,
}

/// Relational operators preserve bag semantics unless DISTINCT is explicit.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum RelNode {
    /// Fixed-snapshot locking relation with explicit ownership observations.
    Locked(Box<crate::LockedQueryPlan>),
    /// Constant bag, including a single empty tuple for SELECT without FROM.
    Values {
        /// Distinct occurrences may contain identical cell values.
        rows: Bag,
    },
    /// Scalar VALUES rows retained until planning and explicit-context execution.
    EvaluateValues {
        /// Typed expressions in occurrence and output-column order.
        rows: Vec<Vec<BoundExpr>>,
    },
    /// Read a declared database table.
    Scan {
        /// Stable catalog identifier, not a SQL identifier string.
        table: String,
    },
    /// Keep rows whose predicate evaluates to TRUE.
    Filter {
        /// Source relation.
        input: Box<Plan>,
        /// Boolean expression; FALSE and UNKNOWN both discard a row.
        predicate: BoundExpr,
    },
    /// Evaluate each expression for every input occurrence.
    Project {
        /// Source relation.
        input: Box<Plan>,
        /// Output expressions in result-column order.
        expressions: Vec<BoundExpr>,
    },
    /// Ranking slots share one complete partition/order choice for every original occurrence.
    Window {
        /// Original source relation, retained as the schema prefix.
        input: Box<Plan>,
        /// Partition, order and correlated ranking functions.
        spec: crate::WindowSpec,
    },
    /// Expand sibling array-returning projections in lockstep, padding shorter arrays with NULL.
    ExpandSets {
        /// Input occurrences repeated once per aligned element ordinal.
        input: Box<Plan>,
        /// Array expressions evaluated against the original input row; elements append to its columns.
        arrays: Vec<BoundExpr>,
    },
    /// Cartesian product of row occurrences.
    Product {
        /// Left input, occupying the result prefix.
        left: Box<Plan>,
        /// Right input, occupying the result suffix.
        right: Box<Plan>,
    },
    /// ON join with explicit outer null-extension semantics.
    Join {
        /// Join preservation policy.
        kind: JoinKind,
        /// Left input relation.
        left: Box<Plan>,
        /// Right input relation.
        right: Box<Plan>,
        /// Predicate indexed against left columns followed by right columns.
        on: BoundExpr,
    },
    /// Retain one occurrence of each whole-row duplicate-equivalence class.
    Distinct {
        /// Source relation whose multiplicities are discarded.
        input: Box<Plan>,
    },
    /// Keep one representative per semantic key, respecting each admitted input ordering.
    DistinctOn {
        /// Source relation, optionally ordered before representative selection.
        input: Box<Plan>,
        /// Semantic duplicate keys evaluated against source positions.
        keys: Vec<BoundExpr>,
    },
    /// Positional set operation with explicit bag or distinct semantics.
    Set {
        /// Union, intersection or difference.
        op: SetOp,
        /// Whether to preserve the operation's SQL ALL multiplicities.
        all: bool,
        /// Left input supplies output labels.
        left: Box<Plan>,
        /// Right input with identical resolved scalar types.
        right: Box<Plan>,
    },
    /// Ordered results preserve every permitted tie order in complete evaluation.
    Order {
        /// Input occurrences before sorting.
        input: Box<Plan>,
        /// Lexicographic ordering expressions and NULL policies.
        keys: Vec<crate::OrderKey>,
    },
    /// Positional pagination retains the order of each possible input result.
    Slice {
        /// Input whose permitted orders must all be represented.
        input: Box<Plan>,
        /// Resolved nonnegative OFFSET.
        offset: usize,
        /// Resolved nonnegative LIMIT, with None representing ALL or NULL.
        limit: Option<usize>,
    },
    /// Forget presentation order at a derived-table or CTE reference boundary.
    Unordered {
        /// Input alternatives retain their row multiplicities.
        input: Box<Plan>,
    },
    /// Group occurrences by SQL duplicate equivalence before evaluating aggregate calls.
    Aggregate {
        /// Rows after FROM/WHERE evaluation.
        input: Box<Plan>,
        /// Group expressions occupying output positions before aggregate values.
        groups: Vec<BoundExpr>,
        /// Explicit admitted aggregate calls in output-column order.
        aggregates: Vec<crate::AggregateExpr>,
    },
}

/// Join null-extension policy; CROSS JOIN uses Product.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum JoinKind {
    /// Matching pairs only.
    Inner,
    /// Matching pairs and unmatched left occurrences.
    Left,
    /// Matching pairs and unmatched right occurrences.
    Right,
    /// Matching pairs and unmatched occurrences from both sides.
    Full,
}

/// Set-operation family, with multiplicity controlled separately by ALL.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum SetOp {
    /// Sum occurrence multiplicities before optional deduplication.
    Union,
    /// Minimum occurrence multiplicity.
    Intersect,
    /// Subtract multiplicities for ALL; exclude present right values for DISTINCT.
    Except,
}

/// A resolved typed scalar expression; parameters are bound to literal values.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BoundExpr {
    /// Resolved scalar family.
    pub ty: SqlType,
    /// Whether this expression may yield a typed NULL.
    pub nullable: bool,
    /// Bound expression tree without unresolved names.
    pub node: ExprNode,
}

/// Side-effect-free expressions in the admitted scalar fragment.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ExprNode {
    /// Typed constant, including explicitly bound parameter values.
    Literal(Cell),
    /// Zero-based position in the current input row.
    Column(usize),
    /// A correlated column in a lexically enclosing query scope.
    OuterColumn {
        /// Zero denotes the nearest enclosing query row.
        depth: usize,
        /// Output position within that enclosing row.
        index: usize,
    },
    /// A deterministically bound nested query with explicit comparison/cardinality semantics.
    Subquery {
        /// Scalar, existence, quantified membership or array construction.
        kind: SubqueryKind,
        /// Child relation with lexically resolved outer references.
        query: Box<Plan>,
        /// Comparison operand for IN/ANY/ALL, absent for scalar/existence/array forms.
        left: Option<Box<BoundExpr>>,
        /// Comparison operator, when required by the query form.
        op: Option<BinaryOp>,
    },
    /// A declared session value; missing evidence is never fabricated.
    Context {
        /// Exact execution-context setting key.
        key: String,
        /// Whether an explicitly absent setting produces SQL NULL.
        missing_ok: bool,
    },
    /// A typed SQL ARRAY constructor, with explicit nested-element shape.
    Array {
        /// Resolved operand family, possibly itself an array for another dimension.
        element: SqlType,
        /// Ordered constructor operands, including typed NULLs.
        values: Vec<BoundExpr>,
    },
    /// ANY/SOME or ALL compares a scalar with every row-major array element under SQL 3VL.
    Quantified {
        /// Validated equality or ordered comparison.
        op: BinaryOp,
        /// True selects ALL; false selects ANY/SOME.
        all: bool,
        /// Scalar comparison operand.
        left: Box<BoundExpr>,
        /// Typed array operand, possibly NULL or empty.
        right: Box<BoundExpr>,
    },
    /// SQL row comparison preserves component-wise NULL and lexicographic stopping rules.
    RowCompare {
        /// Equality/inequality or an ordered comparison.
        op: BinaryOp,
        /// Ordered left scalar components.
        left: Vec<BoundExpr>,
        /// Corresponding right scalar components.
        right: Vec<BoundExpr>,
    },
    /// SQL LIKE/ILIKE under the explicitly admitted text/collation profile.
    Pattern {
        /// Text value to match.
        arg: Box<BoundExpr>,
        /// Pattern using SQL percent/underscore/default escape semantics.
        pattern: Box<BoundExpr>,
        /// True uses the supported case-insensitive character profile.
        insensitive: bool,
    },
    /// Three-valued Boolean negation.
    Not(Box<BoundExpr>),
    /// Admitted comparison or Boolean connective.
    Binary {
        /// Operator with validated operand types.
        op: BinaryOp,
        /// Left scalar operand.
        left: Box<BoundExpr>,
        /// Right scalar operand.
        right: Box<BoundExpr>,
    },
    /// Total NULL test, optionally negated.
    IsNull {
        /// Scalar being tested.
        arg: Box<BoundExpr>,
        /// True implements IS NOT NULL.
        negated: bool,
    },
    /// Checked typed arithmetic; SQL errors remain observable execution outcomes.
    Arithmetic {
        /// Whitelisted operation name from the typed parser AST.
        op: String,
        /// Left evaluated operand.
        left: Box<BoundExpr>,
        /// Right evaluated operand.
        right: Box<BoundExpr>,
    },
    /// Checked numeric negation.
    Negate(Box<BoundExpr>),
    /// Explicit or admitted implicit cast to this expression's result type.
    Cast {
        /// Input value before the target type conversion.
        arg: Box<BoundExpr>,
    },
    /// Assignment coercion enforces typmods without explicit-cast truncation.
    AssignmentCast {
        /// Input before assignment to this expression's target type.
        arg: Box<BoundExpr>,
    },
    /// Interval conversion preserves range and fractional precision without changing value family.
    IntervalCast {
        /// Typed operand before interval typmod enforcement.
        arg: Box<BoundExpr>,
        /// Exact admitted field range and fractional precision.
        qualifier: crate::scalar::IntervalQualifier,
    },
    /// An admitted immutable scalar function, never an arbitrary database UDF.
    Call {
        /// Validated function identifier.
        name: String,
        /// Ordered scalar arguments.
        args: Vec<BoundExpr>,
    },
    /// Searched CASE branches retain conditional runtime evaluation.
    Case {
        /// Boolean conditions paired with their result expressions.
        branches: Vec<(BoundExpr, BoundExpr)>,
        /// Result when no condition evaluates to TRUE.
        otherwise: Box<BoundExpr>,
    },
    /// Return the first non-NULL operand without evaluating later operands at runtime.
    Coalesce(Vec<BoundExpr>),
    /// Return typed NULL when the two arguments compare equal.
    NullIf {
        /// Result value unless equality succeeds.
        left: Box<BoundExpr>,
        /// Comparison value.
        right: Box<BoundExpr>,
    },
}

/// Boolean and comparison operators; ordered comparison is integer-only.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum BinaryOp {
    /// SQL equality; NULL operands yield UNKNOWN.
    Eq,
    /// SQL inequality; NULL operands yield UNKNOWN.
    Ne,
    /// Integer less-than comparison.
    Lt,
    /// Integer less-than-or-equal comparison.
    Le,
    /// Integer greater-than comparison.
    Gt,
    /// Integer greater-than-or-equal comparison.
    Ge,
    /// Three-valued Boolean conjunction.
    And,
    /// Three-valued Boolean disjunction.
    Or,
}

/// Observation made by a bound nested query; distinct SQL forms cannot silently collapse.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum SubqueryKind {
    /// Zero rows gives NULL, one row its value, multiple rows SQLSTATE 21000.
    Scalar,
    /// Whether the child produces at least one row.
    Exists,
    /// Equality membership with SQL three-valued truth.
    In,
    /// Disjunction of comparison truth values.
    Any,
    /// Conjunction of comparison truth values.
    All,
    /// Ordered array construction from a single output column.
    Array,
}

/// Structured failures distinguish unsupported syntax from invalid inputs.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Error {
    /// Stable machine-readable failure classification.
    pub code: String,
    /// Human-readable explanation; no raw SQL or credentials are included.
    pub message: String,
    /// Structured SQL diagnostics, absent for admission and engine failures.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sql: Option<Box<crate::SqlFailure>>,
}

/// Fallible pure-core operation result.
pub type Result<T> = std::result::Result<T, Error>;

impl std::fmt::Display for Error {
    /// Render the stable code alongside a human-readable diagnostic.
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for Error {}
