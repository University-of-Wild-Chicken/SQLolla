//! Fixed-snapshot lock observations are distinct from row results and SQL failures.
use sqlolla_core::*;

/// One physical integer column gives every input occurrence a stable lock ordinal.
fn column() -> Column {
    Column {
        name: "x".into(),
        ty: SqlType::Int32,
        nullable: false,
        qualifier: None,
        schema: None,
    }
}

/// Ascending output order makes OFFSET's consumed lock prefix directly observable.
fn locked(wait_policy: RowWaitPolicy, offset: usize, limit: usize) -> StatementPlan {
    let schema = vec![column()];
    let source = ordered_source(&schema);
    StatementPlan::LockedQuery(LockedQueryPlan {
        source,
        table: "t".into(),
        joined_tables: Vec::new(),
        clause: RowLockClause {
            strength: RowLockMode::Update,
            wait_policy,
        },
        offset,
        limit: Some(limit),
        schema,
    })
}

/// Build the independently ordered source before adding locking semantics.
fn ordered_source(schema: &[Column]) -> Plan {
    let source = Plan {
        schema: schema.to_vec(),
        node: RelNode::Scan { table: "t".into() },
    };
    let key = BoundExpr {
        ty: SqlType::Int32,
        nullable: false,
        node: ExprNode::Column(0),
    };
    Plan {
        schema: schema.to_vec(),
        node: RelNode::Order {
            input: Box::new(source),
            keys: vec![OrderKey {
                expression: key,
                direction: SortDirection::Ascending,
                nulls_first: false,
            }],
        },
    }
}

/// The first occurrence is locked by another transaction; every other row is available.
fn context() -> ExecutionContext {
    ExecutionContext {
        locks: Some(LockContext {
            complete: true,
            transaction: "self".into(),
            held_rows: vec![HeldRowLock {
                table: "t".into(),
                occurrence: 0,
                owner: "other".into(),
                strength: RowLockMode::Update,
            }],
            row_share_conflicts: Default::default(),
        }),
        ..Default::default()
    }
}

/// Stable occurrence indexes refer to this explicit physical bag rather than output positions.
fn database() -> Database {
    Database::from([(
        "t".into(),
        vec![
            vec![Cell::Int32(10)],
            vec![Cell::Int32(20)],
            vec![Cell::Int32(30)],
        ],
    )])
}

/// SKIP LOCKED removes conflicts before OFFSET; skipped output rows still acquire row locks.
#[test]
fn skipped_conflict_precedes_locked_offset_prefix() {
    let outcomes = evaluate_statement(
        &locked(RowWaitPolicy::SkipLocked, 1, 1),
        &database(),
        &context(),
    )
    .unwrap();
    let [StatementOutcome::Succeeded(value)] = outcomes.outcomes.as_slice() else {
        panic!("Expected one lock result");
    };
    assert_eq!(value.rows, vec![vec![Cell::Int32(30)]]);
    let held = value
        .effects
        .iter()
        .filter_map(|effect| match effect {
            EffectEvent::RowLock { occurrence, .. } => Some(*occurrence),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(held, vec![1, 2]);
    assert!(value
        .effects
        .iter()
        .any(|effect| matches!(effect,EffectEvent::TableLock{mode,..} if mode=="row_share")));
}

/// NOWAIT is a modeled SQL error, while an unresolved blocking wait remains an outer refusal.
#[test]
fn nowait_and_unresolved_wait_are_distinct() {
    let outcomes = evaluate_statement(
        &locked(RowWaitPolicy::Nowait, 0, 1),
        &database(),
        &context(),
    )
    .unwrap();
    let StatementOutcome::SqlError(failure) = &outcomes.outcomes[0] else {
        panic!("Expected lock error");
    };
    assert_eq!(failure.sqlstate, "55P03");
    assert!(failure.effects.is_empty());
    let error = evaluate_statement(&locked(RowWaitPolicy::Block, 0, 1), &database(), &context())
        .unwrap_err();
    assert!(error.code.starts_with("unsupported."));
}

/// LIMIT zero acquires ROW SHARE without attempting conflicting row locks.
#[test]
fn zero_limit_has_relation_lock_but_no_row_locks() {
    let outcomes = evaluate_statement(
        &locked(RowWaitPolicy::Nowait, 2, 0),
        &database(),
        &context(),
    )
    .unwrap();
    let StatementOutcome::Succeeded(value) = &outcomes.outcomes[0] else {
        panic!("Expected empty lock result");
    };
    assert!(value.rows.is_empty());
    assert_eq!(value.effects.len(), 1);
    assert!(matches!(&value.effects[0],EffectEvent::TableLock{mode,..} if mode=="row_share"));
}

/// Unordered OFFSET alone can exclude any occurrence even though all rows remain locked.
#[test]
fn unordered_offset_without_limit_preserves_every_result_bag() {
    let StatementPlan::LockedQuery(mut plan) = locked(RowWaitPolicy::Block, 1, 1) else {
        unreachable!()
    };
    let RelNode::Order { input, .. } = plan.source.node else {
        unreachable!()
    };
    plan.source = *input;
    plan.limit = None;
    let mut context = context();
    context.locks.as_mut().unwrap().held_rows.clear();
    let outcomes =
        evaluate_statement(&StatementPlan::LockedQuery(plan), &database(), &context).unwrap();
    assert_eq!(outcomes.outcomes.len(), 3);
    let mut bags = Vec::new();
    for outcome in outcomes.outcomes {
        let StatementOutcome::Succeeded(value) = outcome else {
            panic!("Expected successful ownership");
        };
        assert_eq!(value.effects.len(), 4);
        bags.push(value.rows);
    }
    assert!(bags.contains(&vec![vec![Cell::Int32(20)], vec![Cell::Int32(30)]]));
    assert!(bags.contains(&vec![vec![Cell::Int32(10)], vec![Cell::Int32(30)]]));
    assert!(bags.contains(&vec![vec![Cell::Int32(10)], vec![Cell::Int32(20)]]));
}
