// SQLolla example: {"sql": "SELECT tenant_id, COUNT(*) AS n FROM orders GROUP BY tenant_id HAVING COUNT(*) > 1 ORDER BY tenant_id", "property": {"kind": "max_rows", "max": 1}, "bounds": {"public.orders": 3}, "domain": [{"type": "null", "value": "int32"}, {"type": "int32", "value": 1}, {"type": "int32", "value": 2}], "profile": "sqlolla.explicit_outcomes.v2"}
// Explicit evaluator-backed transitions; payload SHA256 cd801286d4d1cce073a0c55070fd6239b95352b7150069b1a7d66fa919830a8f
abstract sig Value {}
abstract sig Col {}
abstract sig Row { cells: Col -> lone Value }
abstract sig Status {}
one sig Success, Failure extends Status {}
abstract sig Transition { output: set Row, status: one Status, prePayload: one Value, postPayload: one Value, eventPayload: one Value, detailPayload: one Value }
one sig Selected { choice: one Transition }
one sig V0 extends Value {}
one sig V1 extends Value {}
one sig V2 extends Value {}
one sig V3 extends Value {}
one sig V4 extends Value {}
one sig V5 extends Value {}
one sig V6 extends Value {}
one sig V7 extends Value {}
one sig V8 extends Value {}
one sig V9 extends Value {}
one sig V10 extends Value {}
one sig V11 extends Value {}
one sig V12 extends Value {}
one sig V13 extends Value {}
one sig V14 extends Value {}
one sig V15 extends Value {}
one sig V16 extends Value {}
one sig V17 extends Value {}
one sig V18 extends Value {}
one sig V19 extends Value {}
one sig V20 extends Value {}
one sig V21 extends Value {}
one sig V22 extends Value {}
one sig V23 extends Value {}
one sig V24 extends Value {}
one sig V25 extends Value {}
one sig V26 extends Value {}
one sig V27 extends Value {}
one sig V28 extends Value {}
one sig V29 extends Value {}
one sig V30 extends Value {}
one sig V31 extends Value {}
one sig V32 extends Value {}
one sig V33 extends Value {}
one sig V34 extends Value {}
one sig V35 extends Value {}
one sig V36 extends Value {}
one sig V37 extends Value {}
one sig V38 extends Value {}
one sig V39 extends Value {}
one sig V40 extends Value {}
one sig V41 extends Value {}
one sig V42 extends Value {}
one sig V43 extends Value {}
one sig V44 extends Value {}
one sig V45 extends Value {}
one sig K0 extends Col {}
one sig K1 extends Col {}
one sig R0 extends Row {}
one sig R1 extends Row {}
one sig R2 extends Row {}
one sig R3 extends Row {}
one sig R4 extends Row {}
one sig R5 extends Row {}
one sig R6 extends Row {}
one sig R7 extends Row {}
one sig R8 extends Row {}
one sig R9 extends Row {}
one sig R10 extends Row {}
one sig R11 extends Row {}
one sig T0 extends Transition {}
one sig T1 extends Transition {}
one sig T2 extends Transition {}
one sig T3 extends Transition {}
one sig T4 extends Transition {}
one sig T5 extends Transition {}
one sig T6 extends Transition {}
one sig T7 extends Transition {}
one sig T8 extends Transition {}
one sig T9 extends Transition {}
one sig T10 extends Transition {}
one sig T11 extends Transition {}
one sig T12 extends Transition {}
one sig T13 extends Transition {}
one sig T14 extends Transition {}
one sig T15 extends Transition {}
one sig T16 extends Transition {}
one sig T17 extends Transition {}
one sig T18 extends Transition {}
one sig T19 extends Transition {}
pred ExecutionTable {
T0.output = none
T0.status = Success
T1.output = none
T1.status = Success
T2.output = R0
T2.status = Success
T3.output = R1
T3.status = Success
T4.output = R2
T4.status = Success
T5.output = R3
T5.status = Success
T6.output = none
T6.status = Success
T7.output = R4
T7.status = Success
T8.output = none
T8.status = Success
T9.output = none
T9.status = Success
T10.output = R5
T10.status = Success
T11.output = none
T11.status = Success
T12.output = R6
T12.status = Success
T13.output = R7
T13.status = Success
T14.output = R8
T14.status = Success
T15.output = none
T15.status = Success
T16.output = R9
T16.status = Success
T17.output = none
T17.status = Success
T18.output = R10
T18.status = Success
T19.output = R11
T19.status = Success
R0.cells = K0->V0 + K1->V3
R1.cells = K0->V0 + K1->V4
R2.cells = K0->V0 + K1->V3
R3.cells = K0->V0 + K1->V3
R4.cells = K0->V1 + K1->V3
R5.cells = K0->V2 + K1->V3
R6.cells = K0->V1 + K1->V3
R7.cells = K0->V1 + K1->V4
R8.cells = K0->V1 + K1->V3
R9.cells = K0->V2 + K1->V3
R10.cells = K0->V2 + K1->V3
R11.cells = K0->V2 + K1->V4
T0.prePayload = V25
T0.postPayload = V25
T0.eventPayload = V5
T0.detailPayload = V45
T1.prePayload = V24
T1.postPayload = V24
T1.eventPayload = V5
T1.detailPayload = V44
T2.prePayload = V23
T2.postPayload = V23
T2.eventPayload = V5
T2.detailPayload = V43
T3.prePayload = V22
T3.postPayload = V22
T3.eventPayload = V5
T3.detailPayload = V42
T4.prePayload = V20
T4.postPayload = V20
T4.eventPayload = V5
T4.detailPayload = V40
T5.prePayload = V21
T5.postPayload = V21
T5.eventPayload = V5
T5.detailPayload = V41
T6.prePayload = V17
T6.postPayload = V17
T6.eventPayload = V5
T6.detailPayload = V37
T7.prePayload = V15
T7.postPayload = V15
T7.eventPayload = V5
T7.detailPayload = V35
T8.prePayload = V16
T8.postPayload = V16
T8.eventPayload = V5
T8.detailPayload = V36
T9.prePayload = V19
T9.postPayload = V19
T9.eventPayload = V5
T9.detailPayload = V39
T10.prePayload = V18
T10.postPayload = V18
T10.eventPayload = V5
T10.detailPayload = V38
T11.prePayload = V11
T11.postPayload = V11
T11.eventPayload = V5
T11.detailPayload = V31
T12.prePayload = V8
T12.postPayload = V8
T12.eventPayload = V5
T12.detailPayload = V28
T13.prePayload = V6
T13.postPayload = V6
T13.eventPayload = V5
T13.detailPayload = V26
T14.prePayload = V7
T14.postPayload = V7
T14.eventPayload = V5
T14.detailPayload = V27
T15.prePayload = V10
T15.postPayload = V10
T15.eventPayload = V5
T15.detailPayload = V30
T16.prePayload = V9
T16.postPayload = V9
T16.eventPayload = V5
T16.detailPayload = V29
T17.prePayload = V14
T17.postPayload = V14
T17.eventPayload = V5
T17.detailPayload = V34
T18.prePayload = V13
T18.postPayload = V13
T18.eventPayload = V5
T18.detailPayload = V33
T19.prePayload = V12
T19.postPayload = V12
T19.eventPayload = V5
T19.detailPayload = V32
}
run ExecutionTable for 0 but 5 Int, exactly 20 Transition, exactly 12 Row, exactly 2 Col, exactly 46 Value, exactly 2 Status
pred Violates { #Selected.choice.output > 1 }
pred ResultProperty { not Violates }
assert QueryProperty { ExecutionTable implies ResultProperty }
check QueryProperty for 0 but 5 Int, exactly 20 Transition, exactly 12 Row, exactly 2 Col, exactly 46 Value, exactly 2 Status
run { ExecutionTable and Selected.choice = T0 } for 0 but 5 Int, exactly 20 Transition, exactly 12 Row, exactly 2 Col, exactly 46 Value, exactly 2 Status
run { ExecutionTable and Selected.choice = T1 } for 0 but 5 Int, exactly 20 Transition, exactly 12 Row, exactly 2 Col, exactly 46 Value, exactly 2 Status
run { ExecutionTable and Selected.choice = T2 } for 0 but 5 Int, exactly 20 Transition, exactly 12 Row, exactly 2 Col, exactly 46 Value, exactly 2 Status
run { ExecutionTable and Selected.choice = T3 } for 0 but 5 Int, exactly 20 Transition, exactly 12 Row, exactly 2 Col, exactly 46 Value, exactly 2 Status
run { ExecutionTable and Selected.choice = T4 } for 0 but 5 Int, exactly 20 Transition, exactly 12 Row, exactly 2 Col, exactly 46 Value, exactly 2 Status
run { ExecutionTable and Selected.choice = T5 } for 0 but 5 Int, exactly 20 Transition, exactly 12 Row, exactly 2 Col, exactly 46 Value, exactly 2 Status
run { ExecutionTable and Selected.choice = T6 } for 0 but 5 Int, exactly 20 Transition, exactly 12 Row, exactly 2 Col, exactly 46 Value, exactly 2 Status
run { ExecutionTable and Selected.choice = T7 } for 0 but 5 Int, exactly 20 Transition, exactly 12 Row, exactly 2 Col, exactly 46 Value, exactly 2 Status
run { ExecutionTable and Selected.choice = T8 } for 0 but 5 Int, exactly 20 Transition, exactly 12 Row, exactly 2 Col, exactly 46 Value, exactly 2 Status
run { ExecutionTable and Selected.choice = T9 } for 0 but 5 Int, exactly 20 Transition, exactly 12 Row, exactly 2 Col, exactly 46 Value, exactly 2 Status
run { ExecutionTable and Selected.choice = T10 } for 0 but 5 Int, exactly 20 Transition, exactly 12 Row, exactly 2 Col, exactly 46 Value, exactly 2 Status
run { ExecutionTable and Selected.choice = T11 } for 0 but 5 Int, exactly 20 Transition, exactly 12 Row, exactly 2 Col, exactly 46 Value, exactly 2 Status
run { ExecutionTable and Selected.choice = T12 } for 0 but 5 Int, exactly 20 Transition, exactly 12 Row, exactly 2 Col, exactly 46 Value, exactly 2 Status
run { ExecutionTable and Selected.choice = T13 } for 0 but 5 Int, exactly 20 Transition, exactly 12 Row, exactly 2 Col, exactly 46 Value, exactly 2 Status
run { ExecutionTable and Selected.choice = T14 } for 0 but 5 Int, exactly 20 Transition, exactly 12 Row, exactly 2 Col, exactly 46 Value, exactly 2 Status
run { ExecutionTable and Selected.choice = T15 } for 0 but 5 Int, exactly 20 Transition, exactly 12 Row, exactly 2 Col, exactly 46 Value, exactly 2 Status
run { ExecutionTable and Selected.choice = T16 } for 0 but 5 Int, exactly 20 Transition, exactly 12 Row, exactly 2 Col, exactly 46 Value, exactly 2 Status
run { ExecutionTable and Selected.choice = T17 } for 0 but 5 Int, exactly 20 Transition, exactly 12 Row, exactly 2 Col, exactly 46 Value, exactly 2 Status
run { ExecutionTable and Selected.choice = T18 } for 0 but 5 Int, exactly 20 Transition, exactly 12 Row, exactly 2 Col, exactly 46 Value, exactly 2 Status
run { ExecutionTable and Selected.choice = T19 } for 0 but 5 Int, exactly 20 Transition, exactly 12 Row, exactly 2 Col, exactly 46 Value, exactly 2 Status
