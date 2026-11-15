import SQLolla.Relation

/-!
# Typed relational IR design

These constructors specify representable shape, not implemented PostgreSQL
semantics. The executable helpers cover projection, selection, product and
UNION ALL at the abstract bag level in Relation; there is no RelExpr evaluator. Binding,
nullability validation, casts, errors, aggregates and Alloy lowering are open
obligations. A constructor never implies a supported verification capability.
-/

namespace SQLolla

/-- Typed projections retain output position and may repeat an input column. -/
inductive Projection (input : Schema) : Schema → Type where
  | nil : Projection input []
  | cons (expression : ScalarExpr input ty) (rest : Projection input output) :
      Projection input (ty :: output)

/-- Evaluate the admitted total projection fragment without removing occurrences. -/
def Projection.eval : Projection input output → Row input → Row output
  | .nil, _ => .nil
  | .cons expression rest, row => .cons (expression.eval row) (rest.eval row)

/-- Predicates retain scalar types and SQL's three-valued logical operators. -/
inductive Predicate (schema : Schema) where
  | literal (value : Truth)
  | equalInteger (left right : ScalarExpr schema .integer)
  | equalBoolean (left right : ScalarExpr schema .boolean)
  | isNull (expression : ScalarExpr schema ty)
  | negate (predicate : Predicate schema)
  | and (left right : Predicate schema)
  | or (left right : Predicate schema)

/-- Evaluate only this explicit, total predicate fragment; text equality is absent. -/
def Predicate.eval : Predicate schema → Row schema → Truth
  | .literal value, _ => value
  | .equalInteger left right, row => sqlEq (left.eval row) (right.eval row)
  | .equalBoolean left right, row => sqlEq (left.eval row) (right.eval row)
  | .isNull expression, row => if (expression.eval row).isNone then .yes else .no
  | .negate predicate, row => (predicate.eval row).negate
  | .and left right, row => (left.eval row).and (right.eval row)
  | .or left right, row => (left.eval row).or (right.eval row)

/-- A resolved table reference is tied to a schema, with binding supplied externally. -/
structure TableRef (schema : Schema) where
  catalogId : String
  relationId : String

/-- ALL retains multiplicity; DISTINCT requires admitted duplicate equivalence. -/
inductive Multiplicity where
  | all
  | distinct
  deriving DecidableEq, Repr

/-- Join kind is explicit because null extension changes outer-join semantics. -/
inductive JoinKind where
  | inner
  | left
  | right
  | full
  deriving DecidableEq, Repr

/-- Typed algebra syntax. Its full denotation is a later milestone obligation. -/
inductive RelExpr : Schema → Type where
  | scan (table : TableRef schema) : RelExpr schema
  | values (rows : Bag (Row schema)) : RelExpr schema
  | select (predicate : Predicate schema) (source : RelExpr schema) : RelExpr schema
  | project (columns : Projection input output) (source : RelExpr input) : RelExpr output
  | product (left : RelExpr a) (right : RelExpr b) : RelExpr (a ++ b)
  | join (kind : JoinKind) (predicate : Predicate (a ++ b))
      (left : RelExpr a) (right : RelExpr b) : RelExpr (a ++ b)
  | semiJoin (predicate : Predicate (a ++ b))
      (left : RelExpr a) (right : RelExpr b) : RelExpr a
  | antiJoin (predicate : Predicate (a ++ b))
      (left : RelExpr a) (right : RelExpr b) : RelExpr a
  | union (mode : Multiplicity) (left right : RelExpr schema) : RelExpr schema
  | intersect (mode : Multiplicity) (left right : RelExpr schema) : RelExpr schema
  | except (mode : Multiplicity) (left right : RelExpr schema) : RelExpr schema
  | distinct (source : RelExpr schema) : RelExpr schema

/-- A schema-preserving assignment evaluates every expression against the old row. -/
structure Assignment (schema : Schema) where
  values : Projection schema schema

/-- An effect is distinct from a query; its error and frame semantics remain explicit. -/
inductive Effect where
  | insert (table : TableRef schema) (source : RelExpr schema)
  | update (table : TableRef schema) (predicate : Predicate schema)
      (assignment : Assignment schema)
  | delete (table : TableRef schema) (predicate : Predicate schema)

/-- MVP transactions assume no concurrent database transactions. -/
inductive ExecutionProfile where
  | sequentialNoConcurrency
  deriving DecidableEq, Repr

/-- Structured boundaries avoid pretending the parser accepts BEGIN or COMMIT. -/
structure TransactionIR where
  profile : ExecutionProfile
  statements : List Effect

/-- A capability decision is distinct from whether syntax can be represented. -/
inductive Capability where
  | supported
  | unsupported (reason : String)
  | incompleteContext (reason : String)
  deriving DecidableEq, Repr

/-- Shape checks and semantics checks must pass independently before analysis. -/
structure Admission where
  parser : Capability
  binding : Capability
  execution : Capability
  encoding : Capability

end SQLolla
