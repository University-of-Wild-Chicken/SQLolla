//! Complete expansion regressions preserve NULL padding and storage-order pagination.
use serde_json::{json, Value};
use sqlolla_core::*;

/// Build a pinned parser envelope with two independently supplied array parameters.
fn report(limit: Value) -> Value {
    let items = (1..=2)
        .map(|index| {
            json!({"tag":"expression","alias":null,"value":{
        "tag":"call","name":["unnest"],"args":[{"tag":"numbered_parameter","index":index}]}})
        })
        .collect::<Vec<_>>();
    json!({"protocol":"sqlolla.parser.v2","parser_commit":"ad15c585155ff7fc2c5fc4c9746905e4959ff5eb",
        "assurance":"syntax-only","semantic_preservation":false,"result":{"parse_status":"parsed",
        "export_status":"exported","ast":{"tag":"query","ctes":[],"recursive":false,"order_by":[],
        "limit":limit,"offset":null,"offset_first":false,"locking":[],"body":{"tag":"select","items":items,
        "source":null,"where":null,"distinct":false,"group_by":[],"having":null,"distinct_on":[]}}}})
}

/// Bind and evaluate target-list arrays through the full statement interface.
fn run(a: &str, b: &str, limit: Value) -> Bag {
    let ty = SqlType::Array(Box::new(SqlType::Int32));
    let params = Parameters::from([
        (1, scalar::input(&ty, a).unwrap()),
        (2, scalar::input(&ty, b).unwrap()),
    ]);
    let catalog = serde_json::from_value(json!({"search_path":["public"],"tables":{}})).unwrap();
    let plan = bind_statement(&report(limit), &catalog, &params).unwrap();
    let result = evaluate_statement(&plan, &Database::new(), &ExecutionContext::default()).unwrap();
    assert_eq!(result.outcomes.len(), 1);
    let StatementOutcome::Succeeded(value) = &result.outcomes[0] else {
        panic!("Unexpected SQL error")
    };
    assert_eq!(value.ordering, ResultOrdering::Bag);
    value.rows.clone()
}

/// Sibling arrays align by ordinal, including typed NULL padding and repeated values.
#[test]
fn aligned_arrays_keep_duplicates_and_padding() {
    assert_eq!(
        run("{1,1}", "{2}", Value::Null),
        vec![
            vec![Cell::Int32(1), Cell::Null(SqlType::Int32)],
            vec![Cell::Int32(1), Cell::Int32(2)]
        ]
    );
}

/// LIMIT consumes storage-order expansion before the SQL presentation boundary becomes a bag.
#[test]
fn page_keeps_first_array_element() {
    assert_eq!(
        run("{2,1}", "{4,3}", json!({"tag":"integer","magnitude":"1"})),
        vec![vec![Cell::Int32(2), Cell::Int32(4)]]
    );
}

/// All-empty sibling sets remove the otherwise singleton SELECT-without-FROM source row.
#[test]
fn empty_arrays_emit_no_row() {
    assert!(run("{}", "{}", Value::Null).is_empty());
}
