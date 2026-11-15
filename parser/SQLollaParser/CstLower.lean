import SQLollaParser.CstStatement
import SQLollaParser.CstContract

/-! Total bounded dispatch from a certified CST to explicit typed-syntax JSON. -/
open Lean (Json toJson)
open SQLean.PostgresGrammar
namespace SQLollaParser

/-- Dispatch only mapped statement productions and verified single-statement wrappers. -/
def cstStatementShape (source : String) (expression query fromExport statement : GrammarTree → Except String Json)
    (tree : GrammarTree) : Except String Json := do
  match tree with
  | .rule 7 [prior, _, .rule 9 [.rule 135 []]] => statement prior
  | .rule 1664 [_] | .rule 1665 [_] => query tree
  | .rule id [child] =>
      if [1, 8, 9, 86, 93, 107, 125, 129, 134, 1573, 1574, 1575, 1576].contains id then statement child
      else throw s!"unsupported.cst_statement_wrapper.{id}"
  | _ => match treeName tree with
      | "SelectStmt" | "select_no_parens" => query tree
      | "InsertStmt" => cstInsert source expression query statement tree
      | "UpdateStmt" => cstUpdate source expression fromExport statement tree
      | "DeleteStmt" => cstDelete source expression fromExport statement tree
      | "ViewStmt" => cstCreateView source query tree
      | "DropStmt" => cstDropView source tree
      | _ => throw s!"unsupported.cst_statement.{treeName tree}"

mutual
  /-- Reuse only exact complete supported expression subtrees before explicit extension lowering. -/
  def lowerExpression (source : String) : Nat → GrammarTree → Except String Json
    | 0, _ => .error "resource.cst_lowering_depth"
    | fuel + 1, tree => do
        match SQLean.Advanced.parseExpression (← treeSource source tree) with
        | .ok expression => expressionV2 64 expression
        | .error _ => cstExpressionShape source (lowerExpression source fuel) (lowerQuery source fuel) tree

  /-- Reuse exact query spans, preserving all numbered parameter identities and SQL bytes. -/
  def lowerQuery (source : String) : Nat → GrammarTree → Except String Json
    | 0, _ => .error "resource.cst_lowering_depth"
    | fuel + 1, tree => do
        match SQLean.Advanced.parseQuery (← treeSource source tree) with
        | .ok query => queryV2 64 query
        | .error _ =>
            cstQueryShape source (lowerExpression source fuel) (lowerQuery source fuel)
              (lowerSource source fuel) (lowerStatement source fuel) tree

  /-- Preserve source scope through direct child recursion instead of descendant extraction. -/
  def lowerSource (source : String) : Nat → GrammarTree → Except String Json
    | 0, _ => .error "resource.cst_lowering_depth"
    | fuel + 1, tree => cstSourceShape source (lowerExpression source fuel)
        (lowerQuery source fuel) (lowerSource source fuel) tree

  /-- Prefer the existing typed parser on exact statements; new syntax requires whole-production lowering. -/
  def lowerStatement (source : String) : Nat → GrammarTree → Except String Json
    | 0, _ => .error "resource.cst_lowering_depth"
    | fuel + 1, tree => do
        match SQLean.Advanced.parse (← treeSource source tree) with
        | .ok statement => statementV2 statement
        | .error _ =>
            cstStatementShape source (lowerExpression source fuel) (lowerQuery source fuel)
              (lowerSource source fuel) (lowerStatement source fuel) tree
end

/-- Keep grammar failure, resource exhaustion and typed structural lowering outcomes distinct. -/
inductive CstOutcome where
  | rejected
  | inconclusive
  | invalidContract
  | parsed (exported : Except String Json)

/-- Count only complete top-level statements, excluding writable CTE bodies and empty separators. -/
def topStatementCount : Nat → GrammarTree → Nat
  | 0, _ => 2
  | fuel + 1, .rule 1 [child] | fuel + 1, .rule 8 [child] => topStatementCount fuel child
  | fuel + 1, .rule 7 [prior, _, last] => topStatementCount fuel prior + topStatementCount fuel last
  | _ + 1, .rule 9 [.rule 135 []] => 0
  | _ + 1, .rule 9 [.rule _ [_]] | _ + 1, .rule 10 [_] => 1
  | _, _ => 2

/-- Require a certified complete grammar parse before attempting any repository-owned extension. -/
def certifiedLower (source : String) : CstOutcome :=
  if !productionContractValid then .invalidContract else
  match SQLean.PostgresGrammar.parse source 200000 with
  | .ok parsed =>
      if topStatementCount 512 parsed.grammatical.tree == 1 then
        .parsed (lowerStatement source 512 parsed.grammatical.tree)
      else .rejected
  | .error (.grammatical .inconclusive) => .inconclusive
  | .error _ => .rejected

end SQLollaParser
