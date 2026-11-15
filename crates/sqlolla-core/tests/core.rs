//! Adversarial examples for bags, NULL, contextual types and lexical scope.
use serde_json::{json, Value};
use sqlolla_core::*;
use std::collections::BTreeMap;

/// Wrap an AST in the pinned, syntax-only adapter protocol.
fn report(ast: Value) -> Value {
    json!({"protocol":"sqlolla.parser.v1","parser_commit":"ad15c585155ff7fc2c5fc4c9746905e4959ff5eb",
        "result":{"parse_status":"parsed","export_status":"exported","ast":ast}})
}

/// Supply one-column nullable catalog tables under an explicit search path.
fn catalog() -> Catalog {
    let column = nullable_id();
    let tables = ["a", "b"]
        .into_iter()
        .map(|name| {
            (
                name.into(),
                Table {
                    schema: "public".into(),
                    name: name.into(),
                    columns: vec![column.clone()],
                },
            )
        })
        .collect();
    Catalog {
        creation_schema: None,
        relation_lookup: None,
        functions: None,
        namespace: None,
        views: Default::default(),
        table_semantics: Default::default(),
        search_path: vec!["public".into()],
        tables,
    }
}

/// A nullable integer position is shared by both catalog input relations.
fn nullable_id() -> Column {
    Column {
        name: "id".into(),
        ty: SqlType::Int32,
        nullable: true,
        qualifier: None,
        schema: None,
    }
}

/// Encode a direct table reference with the adapter's explicit optional fields.
fn table(name: &str, alias: Option<&str>) -> Value {
    json!({"tag":"table","name":name,"alias":alias,"columns":[],"schema":null})
}

/// Encode a qualified or unqualified column reference.
fn column(qualifier: Option<&str>, name: &str) -> Value {
    json!({"tag":"column","name":name,"qualifier":qualifier,"schema":null})
}

/// Encode an unsigned small integer literal.
fn integer(value: u32) -> Value {
    json!({"tag":"integer","magnitude":value.to_string()})
}

/// Encode one scalar SELECT without FROM.
fn scalar(value: Value) -> Value {
    json!({"tag":"select","items":[{"tag":"expression","value":value,"alias":null}],
        "source":null,"where":null,"distinct":false})
}

/// Encode SELECT * over a supplied relation source.
fn select(source: Value) -> Value {
    json!({"tag":"select","items":[{"tag":"wildcard","qualifier":null,"schema":null}],
        "source":source,"where":null,"distinct":false})
}

/// Encode a binary scalar operation.
fn binary(op: &str, left: Value, right: Value) -> Value {
    json!({"tag":"binary","op":op,"left":left,"right":right})
}

/// Encode a set operation without assuming its duplicate semantics.
fn set(op: &str, all: bool, left: Value, right: Value) -> Value {
    json!({"tag":"set_operation","op":op,"all":all,"left":left,"right":right})
}

/// Evaluate a complete bound query against an explicit finite database.
fn run(ast: Value, db: &Database) -> Result<Bag> {
    eval(&bind(&report(ast), &catalog(), &Parameters::new())?, db)
}

/// Produce one-column input rows, retaining NULL and duplicate occurrences.
fn rows(values: &[Option<i32>]) -> Bag {
    values
        .iter()
        .map(|value| vec![value.map(Cell::Int32).unwrap_or(Cell::Null(SqlType::Int32))])
        .collect()
}

/// Compare unordered occurrence collections, never relying on output order.
fn sorted(mut bag: Bag) -> Bag {
    bag.sort();
    bag
}

/// A projection collision must preserve both source row occurrences.
#[test]
fn projection_preserves_duplicates() {
    let mut ast = scalar(integer(7));
    ast["source"] = table("a", None);
    let db = BTreeMap::from([("a".into(), rows(&[Some(1), Some(2)]))]);
    assert_eq!(run(ast, &db).unwrap(), rows(&[Some(7), Some(7)]));
}

/// Every outer join preserves unmatched NULLs and matched duplicate products.
#[test]
fn outer_join_occurrences() {
    let db = BTreeMap::from([
        ("a".into(), rows(&[Some(1), Some(1), None])),
        ("b".into(), rows(&[Some(1), Some(2), None])),
    ]);
    for (kind, count) in [("inner", 2), ("left", 3), ("right", 4), ("full", 5)] {
        let source = json!({"tag":"join","kind":kind,"left":table("a",None),"right":table("b",None),
            "on":binary("eq",column(Some("a"),"id"),column(Some("b"),"id"))});
        let output = run(select(source), &db).unwrap();
        assert_eq!(output.len(), count, "{kind}");
        assert_eq!(
            output
                .iter()
                .filter(|row| **row == vec![Cell::Int32(1), Cell::Int32(1)])
                .count(),
            2
        );
    }
}

/// Empty outer-join inputs still produce correctly typed null extensions.
#[test]
fn empty_outer_side() {
    let db = BTreeMap::from([("a".into(), vec![]), ("b".into(), rows(&[Some(2)]))]);
    let source = json!({"tag":"join","kind":"full","left":table("a",None),"right":table("b",None),
        "on":{"tag":"boolean","value":true}});
    assert_eq!(
        run(select(source), &db).unwrap(),
        vec![vec![Cell::Null(SqlType::Int32), Cell::Int32(2)]]
    );
}

/// ALL uses sum/min/subtraction while DISTINCT uses SQL duplicate equivalence.
#[test]
fn set_multiplicities() {
    let db = BTreeMap::from([
        ("a".into(), rows(&[Some(1), Some(1), None, None])),
        ("b".into(), rows(&[Some(1), None, Some(2)])),
    ]);
    for (op, all, expected) in [
        ("union", true, 7),
        ("union", false, 3),
        ("intersect", true, 2),
        ("intersect", false, 2),
        ("except", true, 2),
        ("except", false, 0),
    ] {
        let ast = set(op, all, select(table("a", None)), select(table("b", None)));
        assert_eq!(run(ast, &db).unwrap().len(), expected, "{op} ALL={all}");
    }
}

/// NULL comparisons are UNKNOWN; FALSE AND UNKNOWN remains FALSE.
#[test]
fn sql_three_valued_logic() {
    let unknown = binary("eq", json!({"tag":"sql_null"}), integer(1));
    let truth = json!({"tag":"boolean","value":true});
    let falsity = json!({"tag":"boolean","value":false});
    assert_eq!(
        run(scalar(unknown.clone()), &Database::new()).unwrap(),
        vec![vec![Cell::Null(SqlType::Bool)]]
    );
    assert_eq!(
        run(
            scalar(binary("and", falsity, unknown.clone())),
            &Database::new()
        )
        .unwrap(),
        vec![vec![Cell::Bool(false)]]
    );
    assert_eq!(
        run(scalar(binary("or", truth, unknown)), &Database::new()).unwrap(),
        vec![vec![Cell::Bool(true)]]
    );
}

/// WHERE accepts TRUE only and must discard NULL predicates.
#[test]
fn where_unknown_discarded() {
    let mut ast = select(table("a", None));
    ast["where"] = json!({"tag":"sql_null"});
    let db = BTreeMap::from([("a".into(), rows(&[Some(1), None]))]);
    assert!(run(ast, &db).unwrap().is_empty());
}

/// SELECT unknown literals resolve using comparison or set-operand context.
#[test]
fn contextual_unknown_literals() {
    let text = json!({"tag":"text","value":" 2 "});
    assert_eq!(
        run(scalar(binary("eq", text, integer(2))), &Database::new()).unwrap(),
        vec![vec![Cell::Bool(true)]]
    );
    let ast = set(
        "union",
        true,
        scalar(json!({"tag":"sql_null"})),
        scalar(integer(1)),
    );
    assert_eq!(
        sorted(run(ast, &Database::new()).unwrap()),
        sorted(rows(&[None, Some(1)]))
    );
}

/// A nested all-unknown UNION resolves to text before its integer parent.
#[test]
fn nested_unknown_resolution_is_not_flattened() {
    let null = scalar(json!({"tag":"sql_null"}));
    let ast = set(
        "union",
        true,
        set("union", true, null.clone(), null),
        scalar(integer(1)),
    );
    assert_eq!(run(ast, &Database::new()).unwrap_err().code, "invalid.type");
}

/// Signed minimum fits Int32 even though its positive magnitude does not.
#[test]
fn signed_minimum_and_overflow() {
    let magnitude = json!({"tag":"integer","magnitude":"2147483648"});
    let negative = json!({"tag":"unary","op":"negative","arg":magnitude});
    assert_eq!(
        run(scalar(negative), &Database::new()).unwrap(),
        rows(&[Some(i32::MIN)])
    );
    assert_eq!(
        run(scalar(magnitude), &Database::new()).unwrap_err().code,
        "unsupported.integer_range"
    );
}

/// Aliases hide base table names; duplicate and ambiguous scopes fail closed.
#[test]
fn alias_and_ambiguity_errors() {
    let mut ast = scalar(column(Some("a"), "id"));
    ast["source"] = table("a", Some("x"));
    assert_eq!(
        bind(&report(ast), &catalog(), &Parameters::new())
            .unwrap_err()
            .code,
        "invalid.column"
    );
    let product = json!({"tag":"product","left":table("a",None),"right":table("b",None)});
    let mut ambiguous = scalar(column(None, "id"));
    ambiguous["source"] = product;
    assert_eq!(
        bind(&report(ambiguous), &catalog(), &Parameters::new())
            .unwrap_err()
            .code,
        "invalid.ambiguous_column"
    );
    let duplicate =
        json!({"tag":"product","left":table("a",Some("x")),"right":table("b",Some("x"))});
    assert_eq!(
        bind(&report(select(duplicate)), &catalog(), &Parameters::new())
            .unwrap_err()
            .code,
        "invalid.duplicate_alias"
    );
}

/// Parameters need explicit types even when their actual value is NULL.
#[test]
fn parameters_are_declared_and_typed() {
    let ast = report(scalar(json!({"tag":"numbered_parameter","index":1})));
    assert_eq!(
        bind(&ast, &catalog(), &Parameters::new()).unwrap_err().code,
        "invalid.parameter"
    );
    let params = BTreeMap::from([(1, Cell::Null(SqlType::Int32))]);
    assert_eq!(
        eval(&bind(&ast, &catalog(), &params).unwrap(), &Database::new()).unwrap(),
        rows(&[None])
    );
}

/// CTEs are lexical relations and their column aliases are visible downstream.
#[test]
fn nonrecursive_cte_and_derived() {
    let cte = json!({"name":"q","columns":["n"],"query":scalar(integer(7))});
    let mut body = scalar(column(Some("q"), "n"));
    body["source"] = table("q", None);
    let ast = json!({"tag":"with","ctes":[cte],"body":body});
    assert_eq!(run(ast, &Database::new()).unwrap(), rows(&[Some(7)]));
    let source = json!({"tag":"derived","alias":"d","columns":["n"],"query":scalar(integer(3))});
    assert_eq!(
        run(select(source), &Database::new()).unwrap(),
        rows(&[Some(3)])
    );
}

/// Unexpected modifiers, omitted fields and unsupported operators are blockers.
#[test]
fn malformed_and_unimplemented_are_blocked() {
    let mut ast = scalar(integer(1));
    ast["order_by"] = json!([]);
    assert_eq!(
        run(ast, &Database::new()).unwrap_err().code,
        "unsupported.ast_field"
    );
    let mut missing = scalar(integer(1));
    missing.as_object_mut().unwrap().remove("source");
    assert_eq!(
        run(missing, &Database::new()).unwrap_err().code,
        "invalid.ast"
    );
    assert_eq!(
        run(
            scalar(binary("add", integer(1), integer(2))),
            &Database::new()
        )
        .unwrap_err()
        .code,
        "unsupported.operator"
    );
}

/// Database rows cannot bypass declared types, nullability or positional arity.
#[test]
fn invalid_database_is_rejected() {
    let db = BTreeMap::from([("a".into(), vec![vec![Cell::Text("1".into())]])]);
    assert_eq!(
        run(select(table("a", None)), &db).unwrap_err().code,
        "invalid.row_type"
    );
    let db = BTreeMap::from([("a".into(), vec![vec![]])]);
    assert_eq!(
        run(select(table("a", None)), &db).unwrap_err().code,
        "invalid.row_arity"
    );
}

/// SELECT aliases are not in scope for the same SELECT's WHERE clause.
#[test]
fn projection_alias_not_visible_to_where() {
    let mut ast = scalar(column(None, "id"));
    ast["source"] = table("a", None);
    ast["items"][0]["alias"] = json!("renamed");
    ast["where"] = binary("eq", column(None, "renamed"), integer(1));
    let result = bind(&report(ast), &catalog(), &Parameters::new());
    assert_eq!(result.unwrap_err().code, "invalid.column");
}

/// Nonrecursive CTEs cannot see later CTEs, and duplicate CTE names are rejected.
#[test]
fn cte_scope_is_lexical() {
    let first = json!({"name":"q","columns":[],"query":select(table("later",None))});
    let later = json!({"name":"later","columns":[],"query":scalar(integer(1))});
    let ast = json!({"tag":"with","ctes":[first,later],"body":select(table("q",None))});
    assert_eq!(
        run(ast, &Database::new()).unwrap_err().code,
        "invalid.table"
    );
    let cte = json!({"name":"q","columns":[],"query":scalar(integer(1))});
    let duplicate = json!({"tag":"with","ctes":[cte,cte],"body":select(table("q",None))});
    assert_eq!(
        run(duplicate, &Database::new()).unwrap_err().code,
        "invalid.cte"
    );
}

/// A derived relation has no access to the sibling FROM relation's columns.
#[test]
fn derived_correlation_is_not_inferred() {
    let derived =
        json!({"tag":"derived","alias":"d","columns":[],"query":scalar(column(Some("a"),"id"))});
    let source = json!({"tag":"product","left":table("a",None),"right":derived});
    assert_eq!(
        run(select(source), &Database::new()).unwrap_err().code,
        "invalid.column"
    );
}

/// Required field validation also protects literal nodes and parser identity.
#[test]
fn protocol_and_literal_fields_fail_closed() {
    let mut envelope = report(scalar(integer(1)));
    envelope["protocol"] = json!("sqlolla.m0.parser.specimen.v0");
    assert_eq!(
        bind(&envelope, &catalog(), &Parameters::new())
            .unwrap_err()
            .code,
        "unsupported.protocol"
    );
    let literal = json!({"tag":"integer","magnitude":"1","cast":"text"});
    assert_eq!(
        run(scalar(literal), &Database::new()).unwrap_err().code,
        "unsupported.ast_field"
    );
}

/// Unused supplied tables are still subject to the closed catalog contract.
#[test]
fn full_database_validation_checks_unused_data() {
    let unknown = BTreeMap::from([("missing".into(), rows(&[Some(1)]))]);
    assert_eq!(
        validate_database(&catalog(), &unknown).unwrap_err().code,
        "invalid.database"
    );
    let bad = BTreeMap::from([("b".into(), vec![vec![Cell::Text("wrong".into())]])]);
    assert_eq!(
        validate_database(&catalog(), &bad).unwrap_err().code,
        "invalid.row_type"
    );
    assert!(validate_database(&catalog(), &Database::new()).is_ok());
    let mut strict = catalog();
    strict.tables.get_mut("b").unwrap().columns[0].nullable = false;
    let null = BTreeMap::from([("b".into(), rows(&[None]))]);
    assert_eq!(
        validate_database(&strict, &null).unwrap_err().code,
        "invalid.row_type"
    );
}

/// Unmodeled catalog constraints and extra tagged-cell fields cannot disappear silently.
#[test]
fn serde_rejects_unknown_metadata() {
    let mut value = serde_json::to_value(catalog()).unwrap();
    value["tables"]["a"]["triggers"] = json!(["audit"]);
    assert!(serde_json::from_value::<Catalog>(value).is_err());
    let mut value = serde_json::to_value(catalog()).unwrap();
    value["tables"]["a"]["columns"][0]["unique"] = json!(true);
    assert!(serde_json::from_value::<Catalog>(value).is_err());
    assert!(
        serde_json::from_value::<Cell>(json!({"type":"int32","value":1,"cast":"text"})).is_err()
    );
}

/// PostgreSQL rejects NBSP-wrapped integer and Boolean unknown literals.
#[test]
fn unicode_whitespace_is_not_input_whitespace() {
    for (raw, right) in [
        ("\u{00a0}1\u{00a0}", integer(1)),
        (
            "\u{00a0}true\u{00a0}",
            json!({"tag":"boolean","value":true}),
        ),
    ] {
        let left = json!({"tag":"text","value":raw});
        assert_eq!(
            run(scalar(binary("eq", left, right)), &Database::new())
                .unwrap_err()
                .code,
            "invalid.literal_coercion"
        );
    }
}

/// Repeated CTE references must hit the expanded-plan cap before exponential growth.
#[test]
fn doubling_ctes_are_bounded_before_encoding() {
    let mut ctes = vec![json!({"name":"q0","columns":[],"query":scalar(integer(1))})];
    for index in 1..20 {
        let prior = select(table(&format!("q{}", index - 1), None));
        let query = set("union", true, prior.clone(), prior);
        ctes.push(json!({"name":format!("q{index}"),"columns":[],"query":query}));
    }
    let ast = json!({"tag":"with","ctes":ctes,"body":select(table("q19",None))});
    assert_eq!(
        run(ast, &Database::new()).unwrap_err().code,
        "resource.plan_nodes"
    );
}

/// Excessive output width is a coverage refusal rather than truncated evidence.
#[test]
fn projection_width_is_bounded() {
    let mut ast = scalar(integer(1));
    ast["items"] = json!((0..65)
        .map(|_| json!({"tag":"expression","value":integer(1),"alias":null}))
        .collect::<Vec<_>>());
    assert_eq!(
        run(ast, &Database::new()).unwrap_err().code,
        "resource.plan_width"
    );
}
