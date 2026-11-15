//! Streaming byte accounting refuses oversized witnesses without allocating another serialized copy.
use super::*;
use std::io::{Error as IoError, Write};

/// Validate a complete prospective journal; equality with the byte bound remains admitted.
pub fn validate_tuple_witness_bytes(
    events: &[TupleVersionEvent],
    next: &TupleVersionEvent,
) -> Result<()> {
    let mut budget = Bytes { used: 0 };
    for event in events.iter().chain(std::iter::once(next)) {
        serde_json::to_writer(&mut budget, event).map_err(|_| {
            error(
                "resource.tuple_witness_bytes",
                "Complete tuple witnesses exceed 8MiB",
            )
        })?;
    }
    Ok(())
}

/// A writer counts exact JSON bytes while storing none of the serialized text.
struct Bytes {
    used: usize,
}

impl Write for Bytes {
    /// Crossing the bound refuses the entire event rather than retaining a shortened witness.
    fn write(&mut self, value: &[u8]) -> std::io::Result<usize> {
        let count = self
            .used
            .checked_add(value.len())
            .filter(|next| *next <= MAX_TUPLE_WITNESS_BYTES)
            .ok_or_else(|| IoError::other("Tuple witness byte capacity"))?;
        self.used = count;
        Ok(value.len())
    }

    /// Counting has no buffered side effect to flush.
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Exact capacity is accepted; the following byte fails without corrupting the count.
    #[test]
    fn capacity_boundary_is_not_truncation() {
        let mut count = Bytes {
            used: MAX_TUPLE_WITNESS_BYTES - 1,
        };
        assert_eq!(count.write(b"x").unwrap(), 1);
        assert!(count.write(b"x").is_err());
        assert_eq!(count.used, MAX_TUPLE_WITNESS_BYTES);
    }

    /// Large duplicated row witnesses are counted even when their event count is tiny.
    #[test]
    fn full_serialized_witnesses_share_one_budget() {
        let row = vec![Cell::Text("x".repeat(65_536)); 32];
        let header = TupleHeader {
            xmin: 10,
            raw_xmax: 20,
            state: TupleXmaxState::OwnLock,
        };
        let event = TupleVersionEvent {
            transition: TupleVersionObservation {
                table: "t".into(),
                occurrence: 0,
                phase: TupleObservationPhase::Applied,
                original: Some(header.clone()),
                before: Some(header.clone()),
                after: header.clone(),
                rollback: Some(header),
            },
            original_row: Some(row.clone()),
            before_row: Some(row.clone()),
            after_row: row,
        };
        assert!(validate_tuple_witness_bytes(&[], &event).is_ok());
        let failure =
            validate_tuple_witness_bytes(std::slice::from_ref(&event), &event).unwrap_err();
        assert_eq!(failure.code, "resource.tuple_witness_bytes");
    }
}
