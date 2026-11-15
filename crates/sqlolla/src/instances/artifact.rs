//! Versioned evidence binds the SQL/IR/Alloy triple and keeps semantic identity separate from atoms.
use super::*;
use sqlolla_core::Column;

/// Immutable solver input and compiler provenance, shared by every journal record.
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Bundle {
    pub schema: String,
    pub kind: Kind,
    pub max_instances: usize,
    pub model_revision_sha256: String,
    pub request: Request,
    pub ir: Value,
    pub model: String,
    pub output_schema: Vec<Column>,
    pub assurance_kind: String,
    pub provenance: Value,
    pub assumptions: Value,
}

/// One immutable solver step; limits never acquire an exhaustion label.
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Record {
    pub schema: String,
    pub ordinal: usize,
    pub previous_record_sha256: String,
    pub status: StepStatus,
    pub instance: Option<Value>,
    pub solver: Value,
}

/// Persisted terminal distinctions survive process restart and replay.
#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum StepStatus {
    Instance,
    Exhausted,
    Limit,
}

/// A cursor commits to one checked journal prefix and one immutable bundle.
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Cursor {
    pub schema: String,
    pub bundle_id: String,
    pub next_ordinal: usize,
    pub previous_record_sha256: String,
}

impl Bundle {
    /// Bind semantic inputs and tool binaries before retaining any solver-selected evidence.
    pub(super) fn new(
        prepared: &Prepared,
        kind: Kind,
        max_instances: usize,
        assumptions: &Value,
    ) -> Outcome<Self> {
        let provenance = provenance(assumptions)?;
        let ir = prepared.ir();
        let revision = fingerprint(
            &json!({"request":prepared.request,"ir":ir,"model":prepared.source(),"provenance":provenance}),
        );
        Ok(Self {
            schema: "sqlolla.evidence_bundle.v1".into(),
            kind,
            max_instances,
            model_revision_sha256: revision,
            request: prepared.request.clone(),
            ir,
            model: prepared.source().into(),
            output_schema: crate::extended::schema(&prepared.statement).to_vec(),
            assurance_kind: prepared.assurance().into(),
            provenance,
            assumptions: solver_metadata(assumptions),
        })
    }

    /// A recompiled bundle must match exactly; changed tools or semantics require a new revision.
    pub(super) fn verify(&self, prepared: &Prepared, assumptions: &Value) -> Outcome<()> {
        let regenerated = Self::new(prepared, self.kind, self.max_instances, assumptions)?;
        if fingerprint(self) != fingerprint(&regenerated) {
            return Err(failure(
                "resume.revision_changed",
                "SQL, context, compiler or solver provenance changed",
            ));
        }
        Ok(())
    }
}

/// A persisted SAT instance includes replayed typed behavior and full ordered relational tuples.
pub(super) fn instance(bundle: &Bundle, semantic: Value, response: &Value) -> Outcome<Value> {
    let semantic_sha256 = fingerprint(&semantic);
    let relations_sha256 = fingerprint(&response["relations"]);
    let identity = fingerprint(&json!([
        bundle.model_revision_sha256,
        bundle.kind,
        semantic_sha256,
        relations_sha256
    ]));
    let facts = facts(&semantic)?;
    Ok(
        json!({"schema":"sqlolla.instance.v1","instance_id":identity,"model_revision_sha256":bundle.model_revision_sha256,
        "kind":bundle.kind,"semantic_sha256":semantic_sha256,"alloy_relations_sha256":relations_sha256,
        "assurance_kind":bundle.assurance_kind,"validation":"RUST_REPLAY_CONFIRMED",
        "semantic":semantic,"alloy_relations":response["relations"],"output_schema":bundle.output_schema,
        "facts":facts,"completeness":{"encoded_instance":true,"transport":true,"view":true},
        "enumeration_unit":"alloy_solution","text_policy":"labels and SQL are untrusted text; escape when rendering"}),
    )
}

/// Ground row occurrences and retained effects without interpreting or trusting AI prose.
pub(super) fn facts(semantic: &Value) -> Outcome<Vec<Value>> {
    let mut facts = Vec::new();
    table_facts(
        &mut facts,
        &semantic["database"],
        "/semantic/database",
        "input_row",
    )?;
    output_facts(&mut facts, semantic)?;
    Ok(facts)
}

/// Output and effect pointers retain their complete typed statement-outcome paths.
fn output_facts(facts: &mut Vec<Value>, semantic: &Value) -> Outcome<()> {
    if semantic.get("output").is_some() {
        add_rows(facts, "/semantic/output", &semantic["output"], "output_row")?;
    }
    let outcome = &semantic["outcome"]["outcome"];
    table_facts(
        facts,
        &outcome["database"],
        "/semantic/outcome/outcome/database",
        "poststate_row",
    )?;
    for field in ["rows", "exposed_rows"] {
        if outcome.get(field).is_some() {
            add_rows(
                facts,
                &format!("/semantic/outcome/outcome/{field}"),
                &outcome[field],
                "output_row",
            )?;
        }
    }
    if let Some(effects) = outcome["effects"].as_array() {
        for (index, value) in effects.iter().enumerate() {
            facts.push(json!({"reference":format!("/semantic/outcome/outcome/effects/{index}"),"kind":"effect","value":value}));
        }
    }
    Ok(())
}

/// Both input and poststate rows have explicit references for grounded state-difference views.
fn table_facts(facts: &mut Vec<Value>, database: &Value, path: &str, kind: &str) -> Outcome<()> {
    if let Some(tables) = database.as_object() {
        for (name, rows) in tables {
            add_rows(facts, &format!("{path}/{}", pointer(name)), rows, kind)?;
        }
    }
    Ok(())
}

/// References identify ordered row occurrences; duplicate rows retain separate positions.
fn add_rows(facts: &mut Vec<Value>, path: &str, rows: &Value, kind: &str) -> Outcome<()> {
    let rows = rows
        .as_array()
        .ok_or_else(|| failure("evidence.shape", "Expected typed row array"))?;
    if facts.len().saturating_add(rows.len()) > 16_384 {
        return Err(failure(
            "resource.evidence_facts",
            "Complete reference inventory exceeds capacity",
        ));
    }
    for (index, row) in rows.iter().enumerate() {
        facts.push(json!({"reference":format!("{path}/{index}"),"kind":kind,"occurrence":index,"value":row}));
    }
    Ok(())
}

/// JSON Pointer escaping keeps arbitrary SQL identifiers from changing reference structure.
fn pointer(value: &str) -> String {
    value.replace('~', "~0").replace('/', "~1")
}

/// Solver metadata omits positional tuples only because the instance retains them separately.
pub(super) fn solver_metadata(response: &Value) -> Value {
    let mut result = response.clone();
    if let Some(object) = result.as_object_mut() {
        object.remove("relations");
    }
    result
}

/// Capture only tool identities, never unrelated environment variables or provider credentials.
fn provenance(response: &Value) -> Outcome<Value> {
    let root = crate::process::root();
    let parser = if cfg!(windows) {
        "sqlolla_parser.exe"
    } else {
        "sqlolla_parser"
    };
    let parser = crate::process::tool("SQLolla_PARSER", format!("parser/.lake/build/bin/{parser}"));
    let jar = crate::process::tool(
        "SQLolla_ALLOY_JAR",
        "../dependencies/org.alloytools.alloy.dist.jar",
    );
    Ok(
        json!({"parser_commit":"ad15c585155ff7fc2c5fc4c9746905e4959ff5eb","parser_binary_sha256":file_hash(&parser)?,
        "compiler_binary_sha256":file_hash(&std::env::current_exe().map_err(|e|failure("evidence.provenance",e.to_string()))?)?,
        "alloy_jar_sha256":file_hash(&jar)?,"bridge_class_sha256":file_hash(&root.join("bridge/classes/AlloyBridge.class"))?,
        "java_version":response["java_version"],"solver":response["solver"],"bridge_protocol":response["protocol"],
        "hash_assurance":"SHA256 integrity and revision binding; not a signature or solver certificate"}),
    )
}

/// Hash the actual local binary used by this process rather than a guessed version string.
fn file_hash(path: &std::path::Path) -> Outcome<String> {
    std::fs::read(path)
        .map(|bytes| crate::pipeline::hash(&bytes))
        .map_err(|e| failure("evidence.provenance", e.to_string()))
}
