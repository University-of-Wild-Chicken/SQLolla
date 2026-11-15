//! Catalog-transition regressions preserve source provenance, name identity and physical framing.
use serde_json::{json, Value};
use sqlolla_core::*;

/// Pin syntax-only parser envelopes without adding a SQL parser to the Rust core.
fn report(ast: Value) -> Value {
    json!({"protocol":"sqlolla.parser.v2","parser_commit":"ad15c585155ff7fc2c5fc4c9746905e4959ff5eb",
        "assurance":"syntax-only","semantic_preservation":false,"result":{"parse_status":"parsed","export_status":"exported","ast":ast}})
}

/// Empty public namespace is explicit evidence, distinct from an omitted namespace.
fn catalog() -> Catalog {
    serde_json::from_value(json!({"search_path":["public"],"tables":{},"namespace":{
        "complete":true,"schemas":{"public":[]},"views":{},"dependents":{}}}))
    .unwrap()
}

/// One complete integer declaration allows key/nullability and empty-bag checks.
fn table(name: &str, skip: bool) -> Value {
    json!({"tag":"create_table","name":name,"if_not_exists":skip,"constraints":[],"columns":[{
        "name":"x","type":{"name":{"tag":"scalar_type","name":"int"},"modifiers":[]},
        "nullable":null,"default":null,"primary_key":true,"unique":false}]})
}

/// Construct a complete standalone query whose target can deliberately fail at execution.
fn query(value: Value) -> Value {
    json!({"tag":"query","body":{"tag":"select","source":null,"where":null,"distinct":false,
        "distinct_on":[],"group_by":[],"having":null,"items":[{"tag":"expression","value":value,"alias":"x"}]},
        "ctes":[],"recursive":false,"order_by":[],"limit":null,"offset":null,"offset_first":false,"locking":[]})
}

/// View options remain explicit even when every supported option is disabled.
fn view(name: &str, query: Value) -> Value {
    json!({"tag":"create_view","name":[name],"columns":[],"query":query,"recursive":false,
        "replace":false,"temporary":false,"options":[],"check_option":null})
}

/// Extract the sole catalog-aware success; SQL failures fail the regression visibly.
fn execute(ast: Value, sql: &str, catalog: &Catalog, database: &Database) -> StatementSuccess {
    let plan = bind_statement_with_source(&report(ast), sql, catalog, &Parameters::new()).unwrap();
    let result =
        evaluate_with_catalog(&plan, catalog, database, &ExecutionContext::default()).unwrap();
    match result.outcomes.into_iter().next().unwrap() {
        StatementOutcome::Succeeded(value) => value,
        other => panic!("{other:?}"),
    }
}

/// New tables preserve key metadata and receive an empty physical bag.
#[test]
fn create_table_records_empty_state_and_primary_key() {
    let result = execute(
        table("t", false),
        "CREATE TABLE t(x int PRIMARY KEY)",
        &catalog(),
        &Database::new(),
    );
    let catalog = result.catalog.unwrap();
    assert!(result.database["public.t"].is_empty());
    assert!(!catalog.tables["public.t"].columns[0].nullable);
    assert_eq!(
        catalog.table_semantics["public.t"]
            .primary_key
            .as_ref()
            .unwrap()
            .name,
        "t_pkey"
    );
    assert!(catalog.namespace.unwrap().schemas["public"].contains("t_pkey"));
}

/// Implicit pg_catalog lookup does not redirect an unqualified CREATE away from current_schema().
#[test]
fn creation_schema_is_distinct_from_effective_lookup() {
    let mut declared = catalog();
    declared.search_path.insert(0, "pg_catalog".into());
    declared
        .namespace
        .as_mut()
        .unwrap()
        .schemas
        .insert("pg_catalog".into(), Default::default());
    declared.creation_schema = Some(CreationSchema {
        name: Some("public".into()),
    });
    let value = execute(
        table("t", false),
        "CREATE TABLE t(x int PRIMARY KEY)",
        &declared,
        &Database::new(),
    );
    assert!(value.catalog.unwrap().tables.contains_key("public.t"));
}

/// A recorded empty creation path yields the PostgreSQL diagnostic without guessing public.
#[test]
fn known_empty_creation_schema_returns_sqlstate() {
    let mut declared = catalog();
    declared.creation_schema = Some(CreationSchema { name: None });
    let plan = bind_statement_with_source(
        &report(table("t", false)),
        "CREATE TABLE t(x int)",
        &declared,
        &Parameters::new(),
    )
    .unwrap();
    let result = evaluate_with_catalog(
        &plan,
        &declared,
        &Database::new(),
        &ExecutionContext::default(),
    )
    .unwrap();
    let StatementOutcome::SqlError(failure) = &result.outcomes[0] else {
        panic!("Expected missing schema")
    };
    assert_eq!(failure.sqlstate, "3F000");
}

/// Contradictory creation evidence fails input validation even for a query that does not create objects.
#[test]
fn creation_schema_must_exist_in_declared_context() {
    let mut declared = catalog();
    declared.creation_schema = Some(CreationSchema {
        name: Some("absent".into()),
    });
    let error = bind_statement_with_source(
        &report(query(json!({"tag":"integer","magnitude":"1"}))),
        "SELECT 1",
        &declared,
        &Parameters::new(),
    )
    .unwrap_err();
    assert_eq!(error.code, "invalid.creation_schema");
}

/// Complete lookup input carries a distinct explicit creation schema observation.
fn lookup_catalog() -> Catalog {
    let mut declared = catalog();
    declared.creation_schema = Some(CreationSchema {
        name: Some("public".into()),
    });
    declared.relation_lookup = Some(Box::new(
        serde_json::from_value(json!({
        "complete":true,"current_database":"postgres","search_path":["public"],
        "schema_usage":{"public":true},"relations":[]}))
        .unwrap(),
    ));
    declared
}

/// Fresh relation identities must be acquired again after an actual namespace transition.
#[test]
fn ddl_invalidates_lookup_only_when_namespace_changes() {
    let mut declared = lookup_catalog();
    let created = execute(
        table("t", false),
        "CREATE TABLE t(x int)",
        &declared,
        &Database::new(),
    );
    assert!(created.catalog.unwrap().relation_lookup.is_none());
    declared
        .namespace
        .as_mut()
        .unwrap()
        .schemas
        .get_mut("public")
        .unwrap()
        .insert("t".into());
    let skipped = execute(
        table("t", true),
        "CREATE TABLE IF NOT EXISTS t(x int)",
        &declared,
        &Database::new(),
    );
    assert_eq!(
        skipped.catalog.unwrap().relation_lookup,
        declared.relation_lookup
    );
}

/// An effective path cannot reveal whether its implicit first system namespace was explicitly selected.
#[test]
fn effective_lookup_requires_creation_evidence() {
    let mut declared = lookup_catalog();
    declared.creation_schema = None;
    let failure = bind_statement_with_source(
        &report(table("t", false)),
        "CREATE TABLE t(x int)",
        &declared,
        &Parameters::new(),
    )
    .unwrap_err();
    assert_eq!(failure.code, "unsupported.ddl_creation_context");
}

/// IF NOT EXISTS against a sequence skips even an otherwise unsupported column type.
#[test]
fn occupied_skip_preserves_namespace_and_skips_type_analysis() {
    let mut catalog = catalog();
    catalog
        .namespace
        .as_mut()
        .unwrap()
        .schemas
        .get_mut("public")
        .unwrap()
        .insert("t".into());
    let mut ast = table("t", true);
    ast["columns"][0]["type"]["name"]["name"] = json!("unknown_type");
    let result = execute(
        ast,
        "CREATE TABLE IF NOT EXISTS t(x unknown_type)",
        &catalog,
        &Database::new(),
    );
    assert_eq!(result.catalog.as_deref(), Some(&catalog));
    assert!(result.database.is_empty());
}

/// A view is stored without planning division by zero, and internal preparation is never accepted on the wire.
#[test]
fn view_creation_is_not_query_execution_and_preparation_is_internal() {
    let value = json!({"tag":"binary","op":"divide","left":{"tag":"integer","magnitude":"1"},"right":{"tag":"integer","magnitude":"0"}});
    let result = execute(
        view("v", query(value)),
        "CREATE VIEW v AS SELECT 1/0 AS x",
        &catalog(),
        &Database::new(),
    );
    let definition = &result.catalog.as_ref().unwrap().views["public.v"];
    let mut encoded = serde_json::to_value(definition).unwrap();
    assert!(encoded.get("prepared").is_none());
    encoded["prepared"] = json!({});
    assert!(serde_json::from_value::<ViewDefinition>(encoded).is_err());
    assert!(definition.prepared.is_some());
}

/// Persisted constraints cannot be overridden by a contradictory execution declaration.
#[test]
fn stored_semantics_reject_conflicting_context() {
    let result = execute(
        table("t", false),
        "CREATE TABLE t(x int PRIMARY KEY)",
        &catalog(),
        &Database::new(),
    );
    let catalog = result.catalog.unwrap();
    let mut context = ExecutionContext::default();
    context
        .table_metadata
        .insert("public.t".into(), TableSemantics::default());
    assert_eq!(
        merged_catalog_context(&catalog, &context).unwrap_err().code,
        "invalid.catalog_semantics"
    );
}

/// Missing namespace evidence never establishes that a CREATE target is available.
#[test]
fn namespace_omission_fails_closed() {
    let mut catalog = catalog();
    catalog.namespace = None;
    assert_eq!(
        bind_statement_with_source(
            &report(table("t", false)),
            "CREATE TABLE t(x int)",
            &catalog,
            &Parameters::new()
        )
        .unwrap_err()
        .code,
        "unsupported.ddl_namespace"
    );
}

/// A same-named physical table is not treated as a view by DROP VIEW.
#[test]
fn drop_wrong_kind_returns_sqlstate() {
    let result = execute(
        table("t", false),
        "CREATE TABLE t(x int)",
        &catalog(),
        &Database::new(),
    );
    let catalog = result.catalog.unwrap();
    let ast = json!({"tag":"drop_view","names":[["t"]],"if_exists":true,"cascade":false});
    let plan = bind_statement_with_source(
        &report(ast),
        "DROP VIEW IF EXISTS t",
        &catalog,
        &Parameters::new(),
    )
    .unwrap();
    let result = evaluate_with_catalog(
        &plan,
        &catalog,
        &result.database,
        &ExecutionContext::default(),
    )
    .unwrap();
    assert!(
        matches!(&result.outcomes[0],StatementOutcome::SqlError(failure) if failure.sqlstate=="42809")
    );
}
