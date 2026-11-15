//! Explicit single-writer heap-header observations, separate from transactional table values.
use crate::json::error;
use crate::*;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
mod journal;
mod transitions;
pub use journal::{rollback_tuple_observations, tuple_step};
mod rows;
pub use rows::rollback_tuple_events;
mod capacity;
pub use capacity::validate_tuple_witness_bytes;

/// Complete tuple-header journals refuse overflow instead of omitting effects.
pub const MAX_TUPLE_EVENTS: usize = 4096;

/// Sum of serialized event bytes per complete branch; event count alone does not bound large rows.
pub const MAX_TUPLE_WITNESS_BYTES: usize = 8 * 1024 * 1024;

/// Interpreted raw-xmax states admitted without MultiXact or another active transaction.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TupleXmaxState {
    /// No surviving lock/update; raw bytes may retain an invalidated old identity.
    Invalid,
    /// An active local row lock whose owner is this context's write transaction.
    OwnLock,
    /// An aborted updater or locker leaves observable raw bytes on the restored version.
    AbortedUpdater,
}

/// Visible system-column values and their explicitly supplied header interpretation.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TupleHeader {
    /// Inserting transaction of this version, including PostgreSQL reserved frozen identities.
    pub xmin: u32,
    /// Raw xmax identity, never converted to a command-kind Boolean.
    pub raw_xmax: u32,
    /// Admitted transaction/lock interpretation; this is not inferred from raw_xmax alone.
    pub state: TupleXmaxState,
}

/// Snapshot assumptions required before admitting physical header observations.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TupleContext {
    /// Attests all relevant tuple-header and actor state is supplied.
    pub complete: bool,
    /// No external writers/lockers, concurrent MultiXact or vacuum rewrite is admitted.
    pub exclusive_snapshot: bool,
    /// Actual writes occur in the captured top-level transaction, with no savepoints.
    pub top_level_no_savepoints: bool,
    /// Normal current write xid captured before the statement, not from its output.
    pub write_xid: u32,
    /// Input occurrence-aligned headers; table data and these identities share one snapshot.
    pub tables: BTreeMap<String, Vec<TupleHeader>>,
}

/// Applied transitions and post-abort physical residuals are different observations.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TupleObservationPhase {
    /// Transition while the statement was executing.
    Applied,
    /// Header left on the original visible row after the writer aborted.
    AbortedResidual,
}

/// A validated version journal entry retains the original input identity for rollback projection.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TupleVersionObservation {
    /// Stable catalog table identifier.
    pub table: String,
    /// Stable candidate slot before result canonicalization; input slots precede new slots.
    pub occurrence: usize,
    /// Whether this entry records execution or abort residue.
    pub phase: TupleObservationPhase,
    /// Input snapshot header; absent for a newly inserted occurrence.
    pub original: Option<TupleHeader>,
    /// Header immediately before this transition; absent only for insertion.
    pub before: Option<TupleHeader>,
    /// Resulting visible-version header in this observation phase.
    pub after: TupleHeader,
    /// Original-row residue after abort; absent for newly inserted visible versions.
    pub rollback: Option<TupleHeader>,
}

/// Row witnesses preserve the stable slot's lineage after published table bags are sorted.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TupleVersionEvent {
    /// Validated physical header transition, independent of SQL user-column values.
    pub transition: TupleVersionObservation,
    /// Input row belonging to the original header; absent for an inserted slot.
    pub original_row: Option<Row>,
    /// Row immediately before this transition; absent only for a fresh insertion.
    pub before_row: Option<Row>,
    /// Visible row immediately after this transition or abort residual.
    pub after_row: Row,
}

/// Physical transition operations compose independently of the surrounding SQL command spelling.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TupleAction {
    /// Create a fresh ordinary heap version.
    Insert,
    /// Acquire the local conflict-row lock before evaluating UPDATE WHERE.
    ConflictLock,
    /// Create the updated version, retaining an already held local lock when present.
    Update,
}

impl TupleContext {
    /// Validate one mutation target when the public execution API has no catalog parameter.
    pub(crate) fn validate_target(&self, table: &str, rows: &Bag) -> Result<()> {
        self.profile()?;
        let headers = self
            .tables
            .get(table)
            .ok_or_else(|| error("unsupported.tuple_context", "Target tuple headers missing"))?;
        if headers.len() != rows.len() {
            return Err(error(
                "invalid.tuple_snapshot",
                "Target occurrence counts disagree",
            ));
        }
        headers
            .iter()
            .try_for_each(|header| header.validate(self.write_xid))
    }
    /// Validate explicit profile assumptions and occurrence alignment before execution.
    pub fn validate(&self, catalog: &Catalog, database: &Database) -> Result<()> {
        self.profile()?;
        for (table, headers) in &self.tables {
            if !catalog.tables.contains_key(table)
                || database.get(table).map(Vec::len) != Some(headers.len())
            {
                return Err(error(
                    "invalid.tuple_snapshot",
                    "Tuple headers disagree with the declared table snapshot",
                ));
            }
            for header in headers {
                header.validate(self.write_xid)?;
            }
        }
        Ok(())
    }

    /// A scalar xid cannot attest exclusivity, MultiXact absence or actual write-subtransaction identity.
    fn profile(&self) -> Result<()> {
        if !self.complete || !self.exclusive_snapshot || !self.top_level_no_savepoints {
            return Err(error(
                "unsupported.tuple_profile",
                "Complete exclusive top-level heap-header context required",
            ));
        }
        if self.write_xid < 3 {
            return Err(error(
                "invalid.write_xid",
                "Writer xid must be a normal transaction identity",
            ));
        }
        Ok(())
    }
}

impl TupleHeader {
    /// Validate state interpretation without guessing transaction status from a nonzero raw value.
    fn validate(&self, writer: u32) -> Result<()> {
        if self.xmin == 0
            || (self.state == TupleXmaxState::OwnLock && self.raw_xmax != writer)
            || (self.state == TupleXmaxState::AbortedUpdater
                && (self.raw_xmax < 3 || self.raw_xmax == writer))
        {
            return Err(error(
                "invalid.tuple_header",
                "Tuple transaction interpretation contradicts its identities",
            ));
        }
        Ok(())
    }
}
