//! Structural encoding gates independent of the PostgreSQL integration harness.
use sqlolla::alloy::{Inputs, Property, encode};
use sqlolla_core::{Cell, Column, Database, Plan, RelNode, SqlType};

/// Build a small valid typed constant bag without involving parser or evaluator.
fn values(count: usize) -> Plan {
    Plan {
        schema: vec![Column {
            name: "x".into(),
            ty: SqlType::Int32,
            nullable: false,
            qualifier: None,
            schema: None,
        }],
        node: RelNode::Values {
            rows: vec![vec![Cell::Int32(1)]; count],
        },
    }
}

/// Recompiling identical inputs must preserve byte-for-byte model artifacts.
#[test]
fn compilation_is_deterministic() {
    let plan = values(3);
    let database = Database::new();
    let first = encode(&plan, Inputs::Concrete(&database), None).unwrap();
    let second = encode(&plan, Inputs::Concrete(&database), None).unwrap();
    assert_eq!(first.model, second.model);
    assert_eq!(first.output.rows.len(), 3);
}

/// Complete occurrence capacity must refuse excess instead of reducing solver scope.
#[test]
fn oversized_bag_is_refused() {
    let result = encode(&values(257), Inputs::Concrete(&Database::new()), None);
    assert!(result.err().unwrap().contains("256 slots"));
}

/// Cardinality bounds above structural capacity must avoid overflowing Alloy integers.
#[test]
fn large_property_bound_is_encoded_as_impossible_violation() {
    let property = Property::MaxRows { max: usize::MAX };
    let result = encode(
        &values(2),
        Inputs::Concrete(&Database::new()),
        Some(&property),
    )
    .unwrap();
    assert!(result.model.contains("pred Violates { some none }"));
    assert!(!result.model.contains(&usize::MAX.to_string()));
}

/// Business rules must refer to an existing column with the exact resolved type.
#[test]
fn ill_typed_property_is_refused() {
    let property = Property::OnlyValue {
        column: 0,
        value: Cell::Bool(true),
    };
    assert!(
        encode(
            &values(1),
            Inputs::Concrete(&Database::new()),
            Some(&property)
        )
        .is_err()
    );
    let property = Property::OnlyValue {
        column: 2,
        value: Cell::Int32(1),
    };
    assert!(
        encode(
            &values(1),
            Inputs::Concrete(&Database::new()),
            Some(&property)
        )
        .is_err()
    );
}
