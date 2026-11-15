//! Model context is derived only from immutable, independently reloaded host evidence.
use super::*;
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;

/// The original request is retained privately so model output cannot replace its analysis basis.
pub(super) struct Evidence {
    pub bundle: Value,
    pub instance: Value,
}
/// Only this bounded context and its exact fact-reference set are exposed to the model.
pub(super) struct Prompt {
    pub value: Value,
    pub hash: String,
    pub references: BTreeSet<String>,
}

/// Reload and replay the canonical bundle, then locate the exact requested saved instance.
pub(super) async fn load(state: &HostState, input: &Input) -> Result<Evidence> {
    let action = json!({"action":"instances.open","bundle_id":input.bundle_id});
    let opened = state
        .dispatch(serde_json::to_vec(&action).unwrap())
        .await
        .map_err(|_| {
            failure(
                "ai.evidence_unavailable",
                "Canonical evidence could not be reopened",
            )
        })?;
    opened_evidence(&opened, input)
}

/// Check the canonical revision before selecting an independently replayed instance.
fn opened_evidence(opened: &Value, input: &Input) -> Result<Evidence> {
    if opened["status"] != "ok"
        || opened["bundle_id"] != input.bundle_id
        || opened["model_revision_sha256"] != input.model_revision_sha256
    {
        return Err(failure(
            "ai.evidence_revision",
            "Canonical evidence revision does not match this request",
        ));
    }
    let instance = selected_instance(opened, input)?;
    Ok(Evidence {
        bundle: opened["bundle"].clone(),
        instance: instance.clone(),
    })
}

/// Select only the requested instance with confirmed deterministic replay.
fn selected_instance<'a>(opened: &'a Value, input: &Input) -> Result<&'a Value> {
    let instance = opened["records"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|record| record.get("instance"))
        .find(|instance| instance["instance_id"] == input.instance_id)
        .ok_or_else(|| {
            failure(
                "ai.instance_missing",
                "This instance is not in the canonical bundle",
            )
        })?;
    if instance["model_revision_sha256"] != input.model_revision_sha256
        || instance["validation"] != "RUST_REPLAY_CONFIRMED"
    {
        return Err(failure(
            "ai.instance_revision",
            "Canonical instance validation is unavailable",
        ));
    }
    Ok(instance)
}

/// Redaction omits complete source/payload structures instead of trying to scrub SQL literals.
pub(super) fn build(evidence: &Evidence, redacted: bool) -> Result<Prompt> {
    let facts = evidence.instance["facts"]
        .as_array()
        .ok_or_else(|| failure("ai.facts", "Canonical facts are unavailable"))?;
    validate_facts(facts, &evidence.instance)?;
    let observation = rules::observe(evidence).filter(|item| {
        item["witness_reference"]
            .as_str()
            .is_none_or(|reference| facts.iter().any(|fact| fact["reference"] == reference))
    });
    let preferred = observation
        .as_ref()
        .and_then(|item| item["witness_reference"].as_str());
    let supplied = select_facts(facts, preferred, redacted);
    let references = supplied
        .iter()
        .filter_map(|fact| fact["reference"].as_str().map(str::to_owned))
        .collect();
    let mut value = base_context(evidence, supplied, facts.len(), redacted);
    if let Some(observation) = observation {
        value["rule_observation"] = observation;
    }
    if !redacted {
        include_sources(&mut value, evidence)?;
    }
    finish(value, references)
}

/// Retain the canonical violating row even when other facts exhaust the prompt budget.
fn select_facts(facts: &[Value], preferred: Option<&str>, redacted: bool) -> Vec<Value> {
    let is_preferred =
        |fact: &Value| preferred.is_some_and(|reference| fact["reference"] == reference);
    facts
        .iter()
        .find(|fact| is_preferred(fact))
        .into_iter()
        .chain(facts.iter().filter(|fact| !is_preferred(fact)))
        .take(64)
        .map(|fact| fact_context(fact, redacted))
        .collect()
}

/// Distinguish canonical completeness from the deliberately bounded, possibly redacted prompt view.
fn base_context(evidence: &Evidence, supplied: Vec<Value>, count: usize, redacted: bool) -> Value {
    json!({"model_revision_sha256":evidence.instance["model_revision_sha256"],
        "instance_id":evidence.instance["instance_id"],"kind":evidence.instance["kind"],
        "assurance_kind":evidence.instance["assurance_kind"],"canonical_completeness":evidence.instance["completeness"],
        "prompt_view_complete":count <= 64 && !redacted,
        "source_triple_included":!redacted,"source_triple_omission":if redacted {json!("explicit redaction")} else {Value::Null},
        "facts":supplied,"omitted_fact_count":count.saturating_sub(64),"redacted":redacted,
        "schema":schema_context(&evidence.bundle["request"]["catalog"]),
        "bounds":evidence.bundle["request"]["bounds"],"analysis_mode":evidence.bundle["request"]["mode"]})
}

/// Include the exact persisted source triple, refusing an incomplete canonical artifact.
fn include_sources(value: &mut Value, evidence: &Evidence) -> Result<()> {
    let bundle = &evidence.bundle;
    if bundle["request"]["sql"].as_str().is_none()
        || bundle["ir"].is_null()
        || bundle["model"].is_null()
    {
        return Err(failure(
            "ai.source_triple",
            "The canonical SQL, relational representation or Alloy model is absent",
        ));
    }
    value["request"] = bundle["request"].clone();
    value["source_triple"] =
        json!({"sql":bundle["request"]["sql"],"ir":bundle["ir"],"alloy":bundle["model"]});
    value["provenance"] = bundle["provenance"].clone();
    Ok(())
}

/// Hash exactly the bounded serialized prompt sent to the provider.
fn finish(value: Value, references: BTreeSet<String>) -> Result<Prompt> {
    let bytes = serde_json::to_vec(&value).unwrap();
    if bytes.len() > 131_072 {
        return Err(failure(
            "ai.context_size",
            "Canonical assistance context exceeds 128 KiB",
        ));
    }
    Ok(Prompt {
        value,
        hash: format!("{:x}", Sha256::digest(&bytes)),
        references,
    })
}

/// Fact values are either complete canonical payloads or entirely absent.
fn fact_context(fact: &Value, redacted: bool) -> Value {
    let mut result =
        json!({"reference":fact["reference"],"kind":fact["kind"],"occurrence":fact["occurrence"]});
    if !redacted {
        result["value"] = fact["value"].clone();
    }
    result
}

/// Share declared column names/families without defaults, CHECK text, enum labels or SQL bodies.
fn schema_context(catalog: &Value) -> Vec<Value> {
    catalog["tables"].as_object().into_iter().flat_map(|tables| tables.values()).map(|table| {
        let columns: Vec<_> = table["columns"].as_array().into_iter().flatten().map(|column| {
            let ty = &column["ty"];
            let family = ty.as_str().or_else(|| ty.get("kind").and_then(Value::as_str)).unwrap_or("structured");
            json!({"name":column["name"],"type_family":family,"nullable":column["nullable"]})
        }).collect();
        json!({"schema":table["schema"],"name":table["name"],"columns":columns})
    }).collect()
}

/// Canonical references must resolve to the exact retained payload before entering any prompt.
fn validate_facts(facts: &[Value], instance: &Value) -> Result<()> {
    let mut seen = BTreeSet::new();
    for fact in facts {
        let reference = fact["reference"]
            .as_str()
            .ok_or_else(|| failure("ai.facts", "Malformed canonical fact reference"))?;
        if reference.len() > 4096
            || !seen.insert(reference)
            || instance.pointer(reference) != fact.get("value")
        {
            return Err(failure(
                "ai.facts",
                "Canonical fact does not match the retained instance",
            ));
        }
    }
    Ok(())
}
