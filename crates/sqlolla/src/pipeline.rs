//! Versioned parsing, pure binding/encoding and independently replayed solver evidence.
use crate::alloy::{self, Encoded, Inputs, Property};
use crate::bridge::{self, Bridge};
use crate::process::{parse_sql, tool};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use sqlolla_core::{Catalog, Cell, Database, ExecutionContext, Parameters, Plan, StatementPlan};
use std::collections::BTreeMap;

/// Preserve explicit coverage, input and engine failure categories across stages.
pub(crate) type Failure = (String, String);

/// Carry a typed stage result without converting refusals into successful evidence.
pub(crate) type Outcome<T> = Result<T, Failure>;

/// One offline analysis request; unknown fields are rejected to prevent ignored intent.
#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    /// Caller correlation identifier.
    #[serde(default)]
    pub id: Value,
    /// Exactly one PostgreSQL statement, parsed only by SQLeanParser.
    pub sql: String,
    /// Explicit closed catalog with type/nullability metadata.
    pub catalog: Catalog,
    /// Concrete occurrence bags for compare or compile mode.
    #[serde(default)]
    pub database: Database,
    /// Explicit typed numbered parameter values.
    #[serde(default)]
    pub parameters: Parameters,
    /// compare, compile or analyze; no implicit database execution.
    #[serde(default = "compare_mode")]
    pub mode: String,
    /// Required failure property for symbolic analysis.
    #[serde(default)]
    pub property: Option<Property>,
    /// Per-table upper bounds for symbolic inputs.
    #[serde(default)]
    pub bounds: BTreeMap<String, usize>,
    /// Exact finite input values; NULL includes its SQL type.
    #[serde(default)]
    pub domain: Vec<Cell>,
    /// Include generated artifacts for local inspection when requested.
    #[serde(default)]
    pub include_model: bool,
    /// Explicit session, schema behavior and finite evaluation context.
    #[serde(default)]
    pub execution: Option<ExecutionContext>,
}

impl Request {
    /// Reject fields belonging to a different workflow before starting parser or solver work.
    pub(crate) fn validate(&self) -> Outcome<()> {
        match self.mode.as_str() {
            "compare" | "compile" => self.validate_concrete(),
            "analyze" => self.validate_symbolic(),
            _ => Err((
                "unsupported_mode".into(),
                "use compare, compile or analyze".into(),
            )),
        }
    }

    /// Require a complete closed database and validate even unreferenced tables.
    fn validate_concrete(&self) -> Outcome<()> {
        if self.property.is_some() || !self.bounds.is_empty() || !self.domain.is_empty() {
            return Err((
                "invalid.workflow_fields".into(),
                "concrete modes do not accept property, bounds or domain".into(),
            ));
        }
        sqlolla_core::validate_database(&self.catalog, &self.database)
            .map_err(|e| (e.code, e.message))?;
        if self
            .catalog
            .tables
            .keys()
            .any(|table| !self.database.contains_key(table))
        {
            return Err((
                "invalid.database".into(),
                "concrete modes require every catalog table; use an empty bag explicitly".into(),
            ));
        }
        Ok(())
    }

    /// Keep symbolic domains separate from fixed data and reject unknown bound identifiers.
    fn validate_symbolic(&self) -> Outcome<()> {
        if !self.database.is_empty() {
            return Err((
                "invalid.workflow_fields".into(),
                "analyze does not accept a concrete database".into(),
            ));
        }
        if self.property.is_none() {
            return Err((
                "invalid.property".into(),
                "analyze requires property".into(),
            ));
        }
        if self
            .bounds
            .keys()
            .any(|table| !self.catalog.tables.contains_key(table))
        {
            return Err((
                "invalid.bounds".into(),
                "bound refers to an unknown catalog table".into(),
            ));
        }
        Ok(())
    }
}

/// Persistent local orchestration; cached parser syntax is never a cached semantic verdict.
pub struct Pipeline {
    pub(crate) bridge: Bridge,
    syntax: BTreeMap<String, Value>,
}

impl Pipeline {
    /// Start the private solver process without exposing a network service.
    pub fn new() -> Result<Self, String> {
        Ok(Self {
            bridge: Bridge::new()?,
            syntax: BTreeMap::new(),
        })
    }

    /// Always return explicit request failures, not empty output or a false safety result.
    pub fn handle(&mut self, bytes: &[u8]) -> Value {
        if let Ok(value) = serde_json::from_slice::<Value>(bytes)
            && let Some(action) = value.get("action")
        {
            if matches!(
                action.as_str(),
                Some("refinement.context" | "refinement.check")
            ) {
                return crate::instances::refinement::handle(self, &value, bytes);
            }
            return crate::instances::handle(self, bytes);
        }
        let mut request: Request = match serde_json::from_slice(bytes) {
            Ok(request) => request,
            Err(error) => return malformed_request(bytes, &error),
        };
        match self.run(&mut request) {
            Ok(mut response) => {
                response["id"] = request.id;
                response
            }
            Err((code, message)) => {
                json!({"id":request.id,"status":"error","code":code,"message":message})
            }
        }
    }

    /// Bind and encode once; choose only a declared workflow.
    fn run(&mut self, request: &mut Request) -> Outcome<Value> {
        request.validate()?;
        self.prepare_metadata(request)?;
        let ast = self.parse(&request.sql)?;
        let statement = sqlolla_core::bind_statement_with_source(
            &ast,
            &request.sql,
            &request.catalog,
            &request.parameters,
        )
        .map_err(|e| (e.code, e.message))?;
        let StatementPlan::Query(plan) = &statement else {
            return crate::extended::run(&mut self.bridge, request, &statement);
        };
        if !native_profile(request, plan) {
            return crate::extended::run(&mut self.bridge, request, &statement);
        }
        match request.mode.as_str() {
            "compare" => self.compare(request, plan),
            "compile" => self.compile(request, plan),
            "analyze" => self.analyze(request, plan),
            _ => Err((
                "unsupported_mode".into(),
                "use compare, compile or analyze".into(),
            )),
        }
    }

    /// Regenerate metadata syntax from its source; caller-supplied ASTs are never trusted.
    pub(crate) fn prepare_metadata(&mut self, request: &mut Request) -> Outcome<()> {
        self.prepare_functions(request.catalog.functions.as_mut())?;
        for view in request.catalog.views.values_mut() {
            view.prepared = Some(self.parse(&view.source_sql)?);
        }
        self.prepare_table_metadata(&mut request.catalog.table_semantics)?;
        if let Some(context) = &mut request.execution {
            self.prepare_functions(context.functions.as_mut())?;
            self.prepare_table_metadata(&mut context.table_metadata)?;
        }
        Ok(())
    }

    /// Declared SQL bodies are source inputs; serialized parser fields can never replace parsing.
    fn prepare_functions(
        &mut self,
        functions: Option<&mut sqlolla_core::SqlFunctions>,
    ) -> Outcome<()> {
        if let Some(functions) = functions {
            for function in &mut functions.definitions {
                function.prepared = Some(self.parse(&function.body_sql)?);
            }
        }
        Ok(())
    }

    /// Reparse every persisted or execution-local default and CHECK through the same pinned parser.
    fn prepare_table_metadata(
        &mut self,
        metadata: &mut BTreeMap<String, sqlolla_core::TableSemantics>,
    ) -> Outcome<()> {
        for table in metadata.values_mut() {
            for check in table.checks.iter_mut().chain(
                table
                    .unique_indexes
                    .iter_mut()
                    .filter_map(|index| index.predicate.as_mut()),
            ) {
                check.parser = Some(self.parse(&format!("SELECT {}", check.sql))?);
            }
            for default in table.defaults.values_mut() {
                if let sqlolla_core::DefaultSpec::Expression { sql, parser } = default {
                    *parser = Some(self.parse(&format!("SELECT {sql}"))?);
                }
            }
        }
        Ok(())
    }

    /// Accept only the pinned adapter protocol and an explicitly exported syntax tree.
    pub(crate) fn parse(&mut self, sql: &str) -> Outcome<Value> {
        if sql.len() > 65_536 {
            return Err(("resource.source_bytes".into(), "SQL exceeds 64 KiB".into()));
        }
        if let Some(ast) = self.syntax.get(sql) {
            return Ok(ast.clone());
        }
        let ast = fetch_syntax(sql)?;
        if self.syntax.len() >= 64 {
            self.syntax.clear();
        }
        self.syntax.insert(sql.into(), ast.clone());
        Ok(ast)
    }

    /// Compare independent Rust and Alloy execution of one concrete input database.
    fn compare(&mut self, request: &Request, plan: &Plan) -> Outcome<Value> {
        let encoded = concrete_model(request, plan)?;
        let mut rust =
            sqlolla_core::eval(plan, &request.database).map_err(|e| (e.code, e.message))?;
        rust.sort();
        let solution = self.bridge.solve(&encoded.model, 0).map_err(engine)?;
        if solution["status"] != "SAT" {
            return Err(engine("concrete execution unexpectedly UNSAT".into()));
        }
        let alloy = bridge::decode(&encoded, &solution, &encoded.output).map_err(engine)?;
        if rust != alloy {
            return Err(engine(format!(
                "interpreter/Alloy mismatch: Rust={rust:?}; Alloy={alloy:?}"
            )));
        }
        let mut result = evidence(request, plan, &encoded);
        result["rust_bag"] = json!(rust);
        result["alloy_bag"] = json!(alloy);
        result["alloy_status"] = json!("SAT");
        result["matches"] = json!(true);
        Ok(result)
    }

    /// Export deterministic artifacts after independently validating concrete input shapes.
    fn compile(&self, request: &Request, plan: &Plan) -> Outcome<Value> {
        let encoded = concrete_model(request, plan)?;
        sqlolla_core::eval(plan, &request.database).map_err(|e| (e.code, e.message))?;
        let mut result = evidence(request, plan, &encoded);
        result["model"] = json!(encoded.model);
        result["ir"] = json!(plan);
        Ok(result)
    }

    /// Separate satisfiable assumptions from failure search in the declared finite domain.
    fn analyze(&mut self, request: &Request, plan: &Plan) -> Outcome<Value> {
        let property = request
            .property
            .as_ref()
            .ok_or_else(|| encoding("analyze requires property".into()))?;
        let encoded = symbolic_model(request, plan, property)?;
        let admissible = self.bridge.solve(&encoded.model, 0).map_err(engine)?;
        if admissible["status"] != "SAT" {
            return Err(encoding("inconsistent assumptions within bounds".into()));
        }
        let solution = self.bridge.check(&encoded.model, 1).map_err(engine)?;
        let mut result = analysis_evidence(request, plan, &encoded, property);
        if solution["status"] == "SAT" {
            add_witness(&mut result, request, plan, &encoded, &solution, property)?;
        } else {
            result["analysis"] = json!("NO_COUNTEREXAMPLE_WITHIN_BOUNDS");
        }
        Ok(result)
    }
}

/// Native relational encoding applies only when no extended context semantics are declared.
pub(crate) fn native_profile(request: &Request, plan: &Plan) -> bool {
    alloy::supports(plan)
        && request.execution.is_none()
        && request.catalog.table_semantics.is_empty()
        && request.catalog.functions.is_none()
        && request.catalog.relation_lookup.is_none()
        && !matches!(request.property, Some(Property::NoSqlError))
}

/// Retain a readable correlation ID even when strict request deserialization rejects fields.
fn malformed_request(bytes: &[u8], error: &serde_json::Error) -> Value {
    let raw = serde_json::from_slice::<Value>(bytes).unwrap_or(Value::Null);
    json!({"id":raw.get("id").cloned().unwrap_or(Value::Null), "status":"error",
        "code":"invalid_request", "message":error.to_string()})
}

/// Run the platform-native parser and retain its complete versioned envelope for binding.
fn fetch_syntax(sql: &str) -> Outcome<Value> {
    let name = if cfg!(windows) {
        "sqlolla_parser.exe"
    } else {
        "sqlolla_parser"
    };
    let executable = tool("SQLolla_PARSER", format!("parser/.lake/build/bin/{name}"));
    let response = parse_sql(&executable, sql).map_err(engine)?;
    let envelope: Value = serde_json::from_str(&response).map_err(|e| engine(e.to_string()))?;
    validate_parser_identity(&envelope)?;
    let result = &envelope["result"];
    if result["parse_status"] != "parsed" || result["export_status"] != "exported" {
        return Err(parser_refusal(result));
    }
    if !result["ast"].is_object() || result.as_object().is_none_or(|fields| fields.len() != 3) {
        return Err(engine("malformed exported parser result".into()));
    }
    Ok(envelope)
}

/// Validate parser identity and assurance before accepting cached syntax.
fn validate_parser_identity(envelope: &Value) -> Outcome<()> {
    if !matches!(
        envelope["protocol"].as_str(),
        Some("sqlolla.parser.v1" | "sqlolla.parser.v2")
    ) || envelope["parser_commit"] != "ad15c585155ff7fc2c5fc4c9746905e4959ff5eb"
        || envelope["assurance"] != "syntax-only"
        || envelope["semantic_preservation"] != false
        || envelope.as_object().is_none_or(|fields| fields.len() != 5)
    {
        return Err(engine("parser protocol/version/assurance mismatch".into()));
    }
    Ok(())
}

/// Preserve unsupported syntax and resource refusals instead of relabeling them engine faults.
fn parser_refusal(result: &Value) -> Failure {
    let Some(code) = result["diagnostic"].as_str() else {
        return engine("parser refusal omitted its diagnostic".into());
    };
    let allowed = ["unsupported.", "resource.", "input.", "syntax."];
    if result["export_status"] != "unsupported"
        || !allowed.iter().any(|prefix| code.starts_with(prefix))
    {
        return engine("malformed parser refusal".into());
    }
    (
        code.into(),
        "SQL could not be exported under the supported parser profile".into(),
    )
}

/// Check encoder capacity before the interpreter can materialize a large Cartesian product.
fn concrete_model(request: &Request, plan: &Plan) -> Outcome<Encoded> {
    alloy::encode(plan, Inputs::Concrete(&request.database), None).map_err(encoding)
}

/// Build symbolic inputs without consulting an ignored concrete database.
fn symbolic_model(request: &Request, plan: &Plan, property: &Property) -> Outcome<Encoded> {
    let input = Inputs::Symbolic {
        bounds: &request.bounds,
        domain: &request.domain,
    };
    alloy::encode(plan, input, Some(property)).map_err(encoding)
}

/// Attach the exact finite analysis domain separately from any solver-selected witness.
fn analysis_evidence(
    request: &Request,
    plan: &Plan,
    encoded: &Encoded,
    property: &Property,
) -> Value {
    let mut result = evidence(request, plan, encoded);
    result["bounds"] = json!(request.bounds);
    result["domain"] = json!(request.domain);
    result["property"] = json!(property);
    result["assumptions"] = json!("SATISFIABLE");
    result["property_check"] =
        json!({"command":"check", "assertion":"QueryProperty", "command_index":1});
    result
}

/// Replay a decoded counterexample before exposing it as evidence.
fn add_witness(
    result: &mut Value,
    request: &Request,
    plan: &Plan,
    encoded: &Encoded,
    solution: &Value,
    property: &Property,
) -> Result<(), (String, String)> {
    let decoded = bridge::database(encoded, solution).map_err(engine)?;
    let database = complete_database(&request.catalog, decoded)?;
    let output = bridge::decode(encoded, solution, &encoded.output).map_err(engine)?;
    let mut replay = sqlolla_core::eval(plan, &database).map_err(|e| (e.code, e.message))?;
    replay.sort();
    if replay != output || !alloy::violated(property, &replay) {
        return Err(engine("counterexample replay mismatch".into()));
    }
    result["analysis"] = json!("COUNTEREXAMPLE");
    let instance = json!({"kind":"counterexample","database":database,"output":output,
        "validation":"RUST_REPLAY_CONFIRMED","relations":solution["relations"],"complete":true,
        "completeness_scope":"encoded relations and complete declared catalog"});
    result["instance_sha256"] = json!(fingerprint(&instance));
    result["instance"] = instance;
    Ok(())
}

/// Extend an encoded dependency cone with empty bags to form a complete admissible catalog.
fn complete_database(catalog: &Catalog, mut database: Database) -> Outcome<Database> {
    for table in catalog.tables.keys() {
        database.entry(table.clone()).or_default();
    }
    sqlolla_core::validate_database(catalog, &database)
        .map_err(|error| engine(format!("invalid decoded witness: {}", error.message)))?;
    Ok(database)
}

/// Produce immutable artifact fingerprints separately from solver-selected instances.
fn evidence(request: &Request, plan: &Plan, encoded: &Encoded) -> Value {
    let mut result = context_fields(request);
    result["context_sha256"] = json!(fingerprint(&result));
    result["status"] = json!("ok");
    result["schema"] = json!(plan.schema);
    result["ir_sha256"] = json!(fingerprint(plan));
    result["model_sha256"] = json!(hash(encoded.model.as_bytes()));
    result["claim"] = json!("finite supported profile only");
    if request.include_model {
        result["model"] = json!(encoded.model);
        result["ir"] = json!(plan);
    }
    result
}

/// Bind the complete declared context, including catalog and parameters unused by the query.
fn context_fields(request: &Request) -> Value {
    json!({"profile":"sqlolla.m1.relational.v1", "mode":request.mode,
        "sql_sha256":hash(request.sql.as_bytes()), "catalog_sha256":fingerprint(&request.catalog),
        "parameters_sha256":fingerprint(&request.parameters),
        "input_sha256":fingerprint(&workflow_input(request))})
}

/// Fingerprint the actual concrete database or the complete declared symbolic search input.
fn workflow_input(request: &Request) -> Value {
    if request.mode == "analyze" {
        json!({"bounds":request.bounds, "domain":request.domain, "property":request.property})
    } else {
        json!({"database":request.database})
    }
}

/// Hash typed serialization with stable struct fields and ordered map keys.
pub(crate) fn fingerprint<T: Serialize>(value: &T) -> String {
    let bytes = serde_json::to_vec(value).expect("typed evidence serialization cannot fail");
    hash(&bytes)
}

/// Stable SHA256 for canonical source and intermediate artifacts.
pub(crate) fn hash(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

/// Default requests compare concrete execution without any database connection.
fn compare_mode() -> String {
    "compare".into()
}

/// Preserve engine failures as errors rather than safety claims.
pub(crate) fn engine(message: String) -> (String, String) {
    ("engine_error".into(), message)
}

/// Preserve unsupported scopes and encoding limits as explicit coverage failures.
pub(crate) fn encoding(message: String) -> (String, String) {
    ("inconclusive_encoding".into(), message)
}

#[cfg(test)]
mod tests {
    use super::{Request, complete_database, context_fields, fingerprint, workflow_input};
    use serde_json::json;
    use sqlolla_core::Cell;

    /// Construct a minimal request without invoking the parser or solver.
    fn request() -> Request {
        serde_json::from_value(json!({"sql":"SELECT 1",
            "catalog":{"search_path":["public"],"tables":{}}}))
        .unwrap()
    }

    /// Semantically identical object-key order does not change an evidence content digest.
    #[test]
    fn fingerprints_ignore_json_object_insertion_order() {
        let left: serde_json::Value = serde_json::from_str(r#"{"a":1,"b":2}"#).unwrap();
        let right: serde_json::Value = serde_json::from_str(r#"{"b":2,"a":1}"#).unwrap();
        assert_eq!(fingerprint(&left), fingerprint(&right));
    }

    /// Unused declared catalog or parameter changes still invalidate context identity.
    #[test]
    fn context_covers_unused_declarations() {
        let mut request = request();
        let initial = context_fields(&request);
        request.catalog.search_path.push("unused_schema".into());
        assert_ne!(
            initial["catalog_sha256"],
            context_fields(&request)["catalog_sha256"]
        );
        request.parameters.insert(99, Cell::Int32(7));
        assert_ne!(
            initial["parameters_sha256"],
            context_fields(&request)["parameters_sha256"]
        );
    }

    /// Symbolic properties and bounds are part of the input fingerprint even before solving.
    #[test]
    fn symbolic_context_includes_search_property() {
        let mut request = request();
        request.mode = "analyze".into();
        let initial = fingerprint(&workflow_input(&request));
        request.property = Some(crate::alloy::Property::MaxRows { max: 0 });
        assert_ne!(initial, fingerprint(&workflow_input(&request)));
    }

    /// Symbolic witnesses retain encoded bags and explicitly include unused catalog tables.
    #[test]
    fn witness_completion_preserves_rows_and_fills_unused_tables() {
        let catalog = serde_json::from_value(json!({"search_path":["public"],"tables":{
            "used":{"schema":"public","name":"used","columns":[
                {"name":"n","ty":"int32","nullable":false}]},
            "unused":{"schema":"public","name":"unused","columns":[
                {"name":"n","ty":"int32","nullable":false}]}}}))
        .unwrap();
        let rows = vec![vec![Cell::Int32(7)], vec![Cell::Int32(7)]];
        let decoded = [("used".into(), rows.clone())].into_iter().collect();
        let complete = complete_database(&catalog, decoded).unwrap();
        assert_eq!(complete.len(), 2);
        assert_eq!(complete["used"], rows);
        assert!(complete["unused"].is_empty());
        assert_ne!(
            fingerprint(&complete),
            fingerprint(&serde_json::json!({"used":rows}))
        );
    }
}
