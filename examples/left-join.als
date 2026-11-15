// SQLolla example: {"sql": "SELECT l.x AS left_x, r.x AS right_x FROM ledger AS l LEFT JOIN rates AS r ON l.x = r.x", "property": {"kind": "no_value", "column": 1, "value": {"type": "null", "value": "int32"}}, "bounds": {"public.ledger": 2, "public.rates": 2}, "domain": [{"type": "null", "value": "int32"}, {"type": "int32", "value": 1}, {"type": "int32", "value": 2}], "profile": "sqlolla.m1.relational.v1"}
abstract sig V {}
one sig Null extends V {}
one sig V0 extends V {}
one sig V1 extends V {}
one sig V2 extends V {}
one sig V3 extends V {}
one sig Ordering { lt: V -> V }
fact { Ordering.lt = V0->V1 }
fun eqv[a,b:V]:one V { (a=Null or b=Null) => Null else (a=b => V3 else V2) }
fun ltv[a,b:V]:one V { (a=Null or b=Null) => Null else (a->b in Ordering.lt => V3 else V2) }
fun notv[a:V]:one V { a=Null => Null else (a=V3 => V2 else V3) }
fun andv[a,b:V]:one V { (a=V2 or b=V2) => V2 else ((a=Null or b=Null) => Null else V3) }
fun orv[a,b:V]:one V { (a=V3 or b=V3) => V3 else ((a=Null or b=Null) => Null else V2) }
fun nullv[a:V]:one V { a=Null => V3 else V2 }
abstract sig N0 { alive: one V, c0: one V }
one sig N0R0 extends N0 {}
one sig N0R1 extends N0 {}
abstract sig N1 { alive: one V, c0: one V }
one sig N1R0 extends N1 {}
one sig N1R1 extends N1 {}
abstract sig N2 { alive: one V, c0: one V, c1: one V }
one sig N2R0 extends N2 {}
one sig N2R1 extends N2 {}
one sig N2R2 extends N2 {}
one sig N2R3 extends N2 {}
one sig N2R4 extends N2 {}
one sig N2R5 extends N2 {}
abstract sig N3 { alive: one V, c0: one V, c1: one V }
one sig N3R0 extends N3 {}
one sig N3R1 extends N3 {}
one sig N3R2 extends N3 {}
one sig N3R3 extends N3 {}
one sig N3R4 extends N3 {}
one sig N3R5 extends N3 {}
pred QuerySemantics {
all r:N0 | r.alive in V3+V2
all r:N0 | r.alive=V3 implies r.c0 in (Null+V0+V1)
all r:N0 | r.alive=V2 implies r.c0=Null
all r:N1 | r.alive in V3+V2
all r:N1 | r.alive=V3 implies r.c0 in (Null+V0+V1)
all r:N1 | r.alive=V2 implies r.c0=Null
N2R0.alive = ((N0R0.alive = V3 and N1R0.alive = V3 and eqv[N0R0.c0,N1R0.c0]=V3) => V3 else V2)
N2R0.c0 = N0R0.c0
N2R0.c1 = N1R0.c0
N2R1.alive = ((N0R0.alive = V3 and N1R1.alive = V3 and eqv[N0R0.c0,N1R1.c0]=V3) => V3 else V2)
N2R1.c0 = N0R0.c0
N2R1.c1 = N1R1.c0
N2R2.alive = ((N0R1.alive = V3 and N1R0.alive = V3 and eqv[N0R1.c0,N1R0.c0]=V3) => V3 else V2)
N2R2.c0 = N0R1.c0
N2R2.c1 = N1R0.c0
N2R3.alive = ((N0R1.alive = V3 and N1R1.alive = V3 and eqv[N0R1.c0,N1R1.c0]=V3) => V3 else V2)
N2R3.c0 = N0R1.c0
N2R3.c1 = N1R1.c0
N2R4.alive = ((N0R0.alive = V3 and no p:N1 | N0R0.alive = V3 and p.alive = V3 and eqv[N0R0.c0,p.c0]=V3) => V3 else V2)
N2R4.c0 = N0R0.c0
N2R4.c1 = Null
N2R5.alive = ((N0R1.alive = V3 and no p:N1 | N0R1.alive = V3 and p.alive = V3 and eqv[N0R1.c0,p.c0]=V3) => V3 else V2)
N2R5.c0 = N0R1.c0
N2R5.c1 = Null
N3R0.alive = ((N2R0.alive = V3) => V3 else V2)
N3R0.c0 = N2R0.c0
N3R0.c1 = N2R0.c1
N3R1.alive = ((N2R1.alive = V3) => V3 else V2)
N3R1.c0 = N2R1.c0
N3R1.c1 = N2R1.c1
N3R2.alive = ((N2R2.alive = V3) => V3 else V2)
N3R2.c0 = N2R2.c0
N3R2.c1 = N2R2.c1
N3R3.alive = ((N2R3.alive = V3) => V3 else V2)
N3R3.c0 = N2R3.c0
N3R3.c1 = N2R3.c1
N3R4.alive = ((N2R4.alive = V3) => V3 else V2)
N3R4.c0 = N2R4.c0
N3R4.c1 = N2R4.c1
N3R5.alive = ((N2R5.alive = V3) => V3 else V2)
N3R5.c0 = N2R5.c0
N3R5.c1 = N2R5.c1
}
run QuerySemantics for 0 but 6 Int, exactly 5 V, exactly 2 N0, exactly 2 N1, exactly 6 N2, exactly 6 N3
pred Violates { some r:N3 | r.alive = V3 and r.c1 = Null }
pred ResultProperty { not Violates }
assert QueryProperty { QuerySemantics implies ResultProperty }
check QueryProperty for 0 but 6 Int, exactly 5 V, exactly 2 N0, exactly 2 N1, exactly 6 N2, exactly 6 N3
