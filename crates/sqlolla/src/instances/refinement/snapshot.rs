//! Resolve immutable labels across bundles that share the same bounded invocation basis.
use super::observation::Observation;
use super::*;
use std::collections::{BTreeMap, BTreeSet};
use std::time::{Duration, Instant};

const MAX_PACKET_BYTES: usize = 131_072;

/// One independently restored observation with the user's current label.
pub(super) struct Review {
    pub label: Label,
    pub observation: Observation,
}

/// A canonical bounded snapshot attested by the CLI, including its exact source revision.
pub(super) struct Snapshot {
    pub bundle_id: String,
    pub base: Bundle,
    pub feedback_sha256: String,
    pub cases: Vec<Review>,
    sources: BTreeMap<String, Value>,
}

impl Snapshot {
    /// Reopen each contributing journal before trusting any review labels or source artifacts.
    pub(super) fn load(pipeline: &mut Pipeline, store: &Store, input: Input) -> Outcome<Self> {
        let base = store.bundle(&input.bundle_id)?;
        validate_base(&input, &base)?;
        let feedback_sha256 = fingerprint(&json!({"bundle_id":input.bundle_id,
            "model_revision_sha256":base.model_revision_sha256,"feedback":input.feedback}));
        let mut snap = Self {
            bundle_id: input.bundle_id,
            base,
            feedback_sha256,
            cases: Vec::new(),
            sources: BTreeMap::new(),
        };
        let started = Instant::now();
        for group in input.feedback.chunk_by(|a, b| a.bundle_id == b.bundle_id) {
            snap.load_group(pipeline, store, group)?;
            within_budget(started)?;
        }
        if !snap.sources.contains_key(&snap.bundle_id) {
            session::verified_records(pipeline, store, &snap.bundle_id, &snap.base)?;
            within_budget(started)?;
        }
        validate_observations(&snap.cases)?;
        Ok(snap)
    }

    /// Use each archived SQL revision and command while preserving the base semantic context.
    fn load_group(
        &mut self,
        pipeline: &mut Pipeline,
        store: &Store,
        labels: &[Label],
    ) -> Outcome<()> {
        let id = &labels[0].bundle_id;
        let bundle = store.bundle(id)?;
        if !compatible(&self.base, &bundle) {
            return Err(failure(
                "refinement.incompatible_basis",
                "Reviewed archive differs in bounded analysis basis or output schema",
            ));
        }
        let records = session::verified_records(pipeline, store, id, &bundle)?;
        for label in labels {
            let review = load_review(&bundle, &records, label)?;
            self.cases.push(review);
        }
        self.sources.insert(id.clone(), json!({"sql":bundle.request.sql,
            "model_revision_sha256":bundle.model_revision_sha256,"assurance_kind":bundle.assurance_kind}));
        Ok(())
    }

    /// Identify the immutable base and labels in both successful response variants.
    pub(super) fn response(&self) -> Value {
        json!({"schema":SCHEMA,"status":"ok","bundle_id":self.bundle_id,
            "model_revision_sha256":self.base.model_revision_sha256,"feedback_sha256":self.feedback_sha256})
    }

    /// Return a complete packet or refuse when no complete packet fits the provider context cap.
    pub(super) fn context(&self) -> Outcome<Value> {
        let mut references = Vec::new();
        let feedback: Vec<_> = self
            .cases
            .iter()
            .enumerate()
            .map(|(index, review)| {
                let entry = packet_entry(index, review)?;
                references.extend(entry["references"].as_array().unwrap().iter().cloned());
                Ok(entry)
            })
            .collect::<Outcome<_>>()?;
        let packet = json!({"request":self.base.request,"output_schema":self.base.output_schema,
            "source_triple":{"sql":self.base.request.sql,"ir":self.base.ir,"alloy":self.base.model},
            "assurance_kind":self.base.assurance_kind,"provenance":self.base.provenance,
            "review_sources":self.sources,"feedback":feedback});
        if serde_json::to_vec(&packet).map_err(json_error)?.len() > MAX_PACKET_BYTES {
            return Err(failure(
                "resource.refinement_context",
                "Complete refinement context exceeds 128 KiB",
            ));
        }
        let mut response = self.response();
        response["packet"] = packet;
        response["references"] = json!(references);
        Ok(response)
    }
}

/// Validate active labels and sort by saved identity so transport ordering cannot change the digest.
pub(super) fn validate(input: &mut Input) -> Outcome<()> {
    if !input
        .feedback
        .iter()
        .any(|label| label.disposition != Disposition::Unsure)
    {
        return Err(failure(
            "refinement.feedback_required",
            "At least one active Yes or No is required",
        ));
    }
    if input.feedback.len() > MAX_INSTANCES {
        return Err(failure(
            "resource.refinement_feedback",
            "Active label count exceeds 256",
        ));
    }
    if !identifier(&input.bundle_id) || !identifier(&input.model_revision_sha256) {
        return Err(failure(
            "refinement.invalid_identity",
            "Invalid base bundle or revision identity",
        ));
    }
    unique_labels(&input.feedback)?;
    input
        .feedback
        .sort_by(|a, b| (&a.bundle_id, &a.instance_id).cmp(&(&b.bundle_id, &b.instance_id)));
    Ok(())
}

/// Keep canonical archive identity and bounded analysis separate from candidate claims.
fn validate_base(input: &Input, base: &Bundle) -> Outcome<()> {
    if input.model_revision_sha256 != base.model_revision_sha256 {
        return Err(failure(
            "refinement.base_revision",
            "Base archive revision changed",
        ));
    }
    if base.request.mode != "analyze" || base.request.property.is_none() {
        return Err(failure(
            "refinement.analysis_basis",
            "Refinement requires a bounded analysis with a property",
        ));
    }
    Ok(())
}

/// Refuse multiple active decisions for one immutable record or one event identity.
fn unique_labels(feedback: &[Label]) -> Outcome<()> {
    let mut keys = BTreeSet::new();
    let mut events = BTreeSet::new();
    for label in feedback {
        validate_label(label)?;
        if !keys.insert((&label.bundle_id, &label.instance_id)) || !events.insert(&label.event_id) {
            return Err(failure(
                "refinement.duplicate_feedback",
                "Duplicate active decision or event identity",
            ));
        }
    }
    Ok(())
}

/// Reject malformed identities and oversized user-supplied label text before archive access.
fn validate_label(label: &Label) -> Outcome<()> {
    let hashes = [
        &label.bundle_id,
        &label.instance_id,
        &label.model_revision_sha256,
        &label.semantic_sha256,
    ];
    if hashes.into_iter().any(|hash| !identifier(hash))
        || label.event_id.trim().is_empty()
        || label.event_id.len() > 256
        || label
            .rationale
            .as_ref()
            .is_some_and(|text| text.encode_utf16().count() > 500)
    {
        return Err(failure(
            "refinement.invalid_feedback",
            "Invalid active label identity or rationale",
        ));
    }
    Ok(())
}

/// A label's saved instance and both digests must match the regenerated archive.
fn load_review(bundle: &Bundle, records: &[Record], label: &Label) -> Outcome<Review> {
    if bundle.model_revision_sha256 != label.model_revision_sha256 {
        return Err(failure(
            "refinement.review_revision",
            "Reviewed archive revision differs from the label",
        ));
    }
    let instance = records
        .iter()
        .filter_map(|record| record.instance.as_ref())
        .find(|instance| instance["instance_id"] == label.instance_id)
        .ok_or_else(|| {
            failure(
                "refinement.instance_missing",
                "Reviewed instance is absent from the canonical journal",
            )
        })?;
    if instance["semantic_sha256"] != label.semantic_sha256 {
        return Err(failure(
            "refinement.semantic_identity",
            "Reviewed semantic identity differs from the label",
        ));
    }
    let observation = Observation::decode(bundle, &instance["semantic"])?;
    Ok(Review {
        label: label.clone(),
        observation,
    })
}

/// Source SQL may differ across refinement rounds while every invocation input stays fixed.
fn compatible(base: &Bundle, reviewed: &Bundle) -> bool {
    base.output_schema == reviewed.output_schema && basis(&base.request) == basis(&reviewed.request)
}

/// Ignore source and transport fields when comparing archived invocation bases.
fn basis(request: &Request) -> String {
    fingerprint(&Request {
        sql: String::new(),
        id: Value::Null,
        include_model: false,
        ..request.clone()
    })
}

/// An atom-equivalent observation with opposed active labels cannot be used for refinement.
fn validate_observations(cases: &[Review]) -> Outcome<()> {
    let mut labels = BTreeMap::new();
    for review in cases
        .iter()
        .filter(|r| r.label.disposition != Disposition::Unsure)
    {
        let key = fingerprint(&review.observation);
        if labels
            .insert(key, review.label.disposition)
            .is_some_and(|old| old != review.label.disposition)
        {
            return Err(failure(
                "refinement.conflicting_feedback",
                "The same reviewed observation has both a Yes and a No",
            ));
        }
    }
    Ok(())
}

/// Copy canonical review identities and issue packet-relative JSON Pointers for complete facts.
fn packet_entry(index: usize, review: &Review) -> Outcome<Value> {
    let observation = json!(review.observation);
    let prefix = format!("/feedback/{index}/observation");
    let mut references = vec![
        format!("{prefix}/database"),
        format!("{prefix}/outcome"),
        prefix.clone(),
    ];
    for fact in artifact::facts(&observation)? {
        let reference = fact["reference"]
            .as_str()
            .and_then(|path| path.strip_prefix("/semantic"))
            .filter(|path| observation.pointer(path).is_some())
            .ok_or_else(|| {
                failure(
                    "refinement.reference",
                    "Canonical fact reference failed to resolve",
                )
            })?;
        references.push(format!("{prefix}{reference}"));
    }
    let mut entry = json!(review.label);
    entry["observation"] = observation;
    entry["references"] = json!(references);
    Ok(entry)
}

/// Bound total multi-journal work in addition to the existing per-journal replay limit.
fn within_budget(started: Instant) -> Outcome<()> {
    if started.elapsed() > Duration::from_secs(45) {
        return Err(failure(
            "resource.refinement_replay",
            "Complete snapshot replay exceeded time capacity",
        ));
    }
    Ok(())
}
