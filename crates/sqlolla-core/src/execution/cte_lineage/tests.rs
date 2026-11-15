//! Conditional certificates distinguish occurrence coverage from equal visible values.
use super::*;

/// A single integral column keeps hidden lineage separate from ordinary row contents.
fn scan(table: &str) -> Plan {
    Plan {
        schema: vec![Column {
            name: "k".into(),
            ty: SqlType::Int64,
            nullable: false,
            qualifier: None,
            schema: None,
        }],
        node: RelNode::Scan {
            table: table.into(),
        },
    }
}

/// The producer emits its observable event in the final projection of each input occurrence.
fn plan() -> WithPlan {
    let input = scan("source");
    let expression = BoundExpr {
        ty: SqlType::Int64,
        nullable: false,
        node: ExprNode::Call {
            name: "nextval".into(),
            args: vec![],
        },
    };
    let producer = Plan {
        schema: input.schema.clone(),
        node: RelNode::Project {
            input: Box::new(input),
            expressions: vec![expression],
        },
    };
    WithPlan {
        bindings: vec![CteStatement {
            name: "q".into(),
            table_id: "q".into(),
            statement: Box::new(StatementPlan::Query(producer)),
        }],
        body: Box::new(StatementPlan::Query(scan("q"))),
    }
}

/// A visible duplicate bag retains distinct positions even though every returned cell is equal.
fn evidence(count: usize, covered: Vec<usize>) -> Evidence {
    let producer = Observation {
        rows: vec![vec![Cell::Int64(7)]; count],
        affected: count as u64,
        effects: vec![EffectEvent::SequenceValue {
            sequence: "s".into(),
            value: 7,
        }],
        prefix: vec![],
        trace: None,
    };
    let body = Observation {
        rows: vec![],
        affected: covered.len() as u64,
        effects: vec![],
        prefix: producer.effects.clone(),
        trace: Some(Trace {
            origins: vec!["q".into()],
            rows: covered.into_iter().map(|id| vec![id]).collect(),
        }),
    };
    Evidence {
        required: BTreeSet::from(["q".into()]),
        proven: BTreeSet::new(),
        observations: BTreeMap::from([(0, producer), (1, body)]),
    }
}

/// Complete positional coverage permits the already-evaluated producer observation.
#[test]
fn every_occurrence_is_required() {
    let context = ExecutionContext::default();
    evidence(2, vec![0, 1])
        .verify(&plan(), &Database::new(), &Runtime::new(&context))
        .unwrap();
}

/// Repeating one duplicate-valued occurrence never certifies its equal-valued sibling.
#[test]
fn equal_values_do_not_collapse_occurrences() {
    let context = ExecutionContext::default();
    let failure = evidence(2, vec![0, 0])
        .verify(&plan(), &Database::new(), &Runtime::new(&context))
        .unwrap_err();
    assert_eq!(failure.code, "unsupported.cte_consumer_demand");
}

/// An event emitted behind an empty output cannot pass a vacuous all-occurrences test.
#[test]
fn empty_eventful_producer_is_not_vacuously_demanded() {
    let context = ExecutionContext::default();
    assert!(evidence(0, vec![])
        .verify(&plan(), &Database::new(), &Runtime::new(&context))
        .is_err());
}

/// The certificate never uses an error branch as if it had exhausted its input.
#[test]
fn error_branch_requires_a_separate_prefix_proof() {
    assert_eq!(
        evidence(1, vec![0]).error_admission().unwrap_err().code,
        "unsupported.cte_consumer_error"
    );
}

/// A reducing producer may leave an unobserved event tail despite complete visible coverage.
#[test]
fn reducing_stage_does_not_close_event_sites() {
    let mut plan = plan();
    let StatementPlan::Query(query) = plan.bindings[0].statement.as_ref() else {
        unreachable!()
    };
    let reduced = Plan {
        schema: query.schema.clone(),
        node: RelNode::Distinct {
            input: Box::new(query.clone()),
        },
    };
    *plan.bindings[0].statement = StatementPlan::Query(reduced);
    let context = ExecutionContext::default();
    assert!(evidence(1, vec![0])
        .verify(&plan, &Database::new(), &Runtime::new(&context))
        .is_err());
}

/// Only ownership established before the WITH invocation can disappear as an idempotent delta.
#[test]
fn initialization_is_not_inferred_from_the_producers_own_events() {
    let event = EffectEvent::TableLock {
        table: "source".into(),
        mode: "row_share".into(),
    };
    let mut evidence = evidence(0, vec![]);
    evidence.observations.get_mut(&0).unwrap().effects = vec![event.clone()];
    let context = ExecutionContext::default();
    let runtime = Runtime::new(&context);
    assert!(evidence
        .verify(&plan(), &Database::new(), &runtime)
        .is_err());
    evidence
        .verify(&plan(), &Database::new(), &runtime.fork(&[event]))
        .unwrap();
}

/// A direct positive membership predicate compares the target position with one virtual output.
fn membership() -> BoundExpr {
    let mut query = scan("q");
    query.schema[0].nullable = true;
    BoundExpr {
        ty: SqlType::Bool,
        nullable: true,
        node: ExprNode::Subquery {
            kind: SubqueryKind::In,
            query: Box::new(query),
            left: Some(Box::new(BoundExpr {
                ty: SqlType::Int64,
                nullable: false,
                node: ExprNode::Column(0),
            })),
            op: Some(BinaryOp::Eq),
        },
    }
}

/// Pure replay receives the original target and producer bags and an independently supplied affected count.
fn member_coverage(
    predicate: &BoundExpr,
    rows: Bag,
    affected: u64,
) -> BTreeMap<String, BTreeSet<usize>> {
    let target = MutationTarget {
        table: "t".into(),
        schema: scan("t").schema,
    };
    let db = Database::from([("t".into(), vec![vec![Cell::Int64(1)]]), ("q".into(), rows)]);
    let observed = Observation {
        rows: vec![],
        affected,
        effects: vec![],
        prefix: vec![],
        trace: None,
    };
    let context = ExecutionContext::default();
    super::membership::coverage(&target, predicate, &observed, &db, &Runtime::new(&context))
        .unwrap()
}

/// One TRUE source identity is a witness; equal-valued duplicate occurrences are ambiguous.
#[test]
fn positive_membership_requires_unique_identity() {
    let row = vec![Cell::Int64(1)];
    assert_eq!(
        member_coverage(&membership(), vec![row.clone()], 1)["q"],
        BTreeSet::from([0])
    );
    assert!(member_coverage(&membership(), vec![row.clone(), row], 1).is_empty());
}

/// UNKNOWN comparisons cannot witness selected target demand.
#[test]
fn null_membership_never_supplies_a_positive_witness() {
    assert!(member_coverage(&membership(), vec![vec![Cell::Null(SqlType::Int64)]], 0).is_empty());
}

/// A necessary AND preserves the witness; an OR alternative and negation do not.
#[test]
fn only_necessary_positive_membership_is_certified() {
    let combine = |op| BoundExpr {
        ty: SqlType::Bool,
        nullable: true,
        node: ExprNode::Binary {
            op,
            left: Box::new(membership()),
            right: Box::new(crate::expression::literal(Cell::Bool(true))),
        },
    };
    let rows = vec![vec![Cell::Int64(1)]];
    assert!(!member_coverage(&combine(BinaryOp::And), rows.clone(), 1).is_empty());
    assert!(member_coverage(&combine(BinaryOp::Or), rows.clone(), 1).is_empty());
    let negated = BoundExpr {
        ty: SqlType::Bool,
        nullable: true,
        node: ExprNode::Not(Box::new(membership())),
    };
    assert!(member_coverage(&negated, rows, 0).is_empty());
}
