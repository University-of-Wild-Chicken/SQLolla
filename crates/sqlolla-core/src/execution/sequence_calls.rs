//! Sequence invocation consumes only explicit branch state and returns an allocation delta.
use super::*;

/// Only explicitly admitted sequence operations enter this state-transition handler.
pub(super) fn handles(name: &str) -> bool {
    name == "nextval"
}

/// A NULL argument is strict; a present identity allocates against this branch's inherited trace.
pub(super) fn evaluate(
    name: &str,
    args: &[Cell],
    context: &Runtime<'_>,
) -> Result<ScalarObservation> {
    if !handles(name) || args.len() != 1 || args[0].sql_type() != SqlType::Regclass {
        return Err(error(
            "sql.42883",
            "No sequence function with these arguments",
        ));
    }
    if args[0].is_null() {
        return Ok(ScalarObservation {
            value: Cell::Null(SqlType::Int64),
            effects: vec![],
        });
    }
    let sequences = context.sequences.as_ref().ok_or_else(|| {
        error(
            "unsupported.sequence_context",
            "Explicit sequence state is required",
        )
    })?;
    let identity = resolve(&args[0], sequences)?;
    let event = crate::sequence::next_event(sequences, context.inherited_effects(), identity)?;
    observation(event)
}

/// Object-reference transport holds numeric identity independently of its SQL display name.
fn resolve<'a>(value: &Cell, sequences: &'a SequenceContext) -> Result<&'a str> {
    let Cell::Scalar(identity) = value else {
        return Err(error("invalid.sequence_oid", "Expected regclass identity"));
    };
    let oid = identity
        .value
        .parse::<u32>()
        .map_err(|_| error("invalid.sequence_oid", "Invalid regclass identity"))?;
    sequences
        .sequences
        .iter()
        .find(|(_, value)| value.oid == oid)
        .map(|(identity, _)| identity.as_str())
        .ok_or_else(|| {
            error(
                "unsupported.sequence_identity",
                "Object is not a declared sequence",
            )
        })
}

/// Convert one allocation delta into its typed scalar return without duplicating inherited events.
fn observation(event: EffectEvent) -> Result<ScalarObservation> {
    let EffectEvent::SequenceValue { value, .. } = &event else {
        return Err(error("invalid.sequence_event", "Expected allocation event"));
    };
    Ok(ScalarObservation {
        value: Cell::Int64(*value),
        effects: vec![event],
    })
}
