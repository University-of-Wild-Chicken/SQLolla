import SQLollaParser.Cst

/-! Selected certified grammar productions lower to explicit expressions, without source rewriting. -/
open Lean (Json toJson)
open SQLean.PostgresGrammar
namespace SQLollaParser

/-- Export a pinned expression list in lexical order. -/
def cstExpressionList (emit : GrammarTree → Except String Json) (tree : GrammarTree) : Except String (List Json) := do
  (← treeList [2218, 2219] 512 tree).mapM emit

/-- Export one ORDER BY item and reject operator-based sorting until represented. -/
def cstOrder (emit : GrammarTree → Except String Json) : GrammarTree → Except String Json
  | .rule 1730 [expression, direction, nulls] => do
      let direction ← match direction with
        | .rule 1092 [_] | .rule 1094 [] => pure "asc"
        | .rule 1093 [_] => pure "desc" | _ => .error "unsupported.cst_sort_direction"
      let nulls ← match nulls with
        | .rule 1095 [_, _] => pure (toJson "first")
        | .rule 1096 [_, _] => pure (toJson "last")
        | .rule 1097 [] => pure Json.null | _ => .error "unsupported.cst_sort_nulls"
      pure (Json.mkObj [("expression", ← emit expression), ("direction", toJson direction), ("nulls", nulls)])
  | _ => .error "unsupported.cst_sort"

/-- Retain all sort items while distinguishing an absent ordering clause. -/
def cstOrdering (emit : GrammarTree → Except String Json) : GrammarTree → Except String (List Json)
  | .rule 1725 [] => pure []
  | .rule 1724 [.rule 1726 [_, _, items]] | .rule 1726 [_, _, items] => do
      (← treeList [1727, 1728] 512 items).mapM (cstOrder emit)
  | _ => .error "unsupported.cst_order_clause"

/-- Preserve window name, partition and order; explicit frames require another admitted mapping. -/
def cstWindow (source : String) (emit : GrammarTree → Except String Json) : GrammarTree → Except String Json
  | .rule 2164 [_, name, partition, order, frame, _] => do
      requireEmpty frame
      let name ← match name with
        | .rule 2166 [] => pure Json.null
        | .rule 2165 [name] => toJson <$> identifier source name
        | _ => .error "unsupported.cst_window_name"
      let partition ← match partition with
        | .rule 2168 [] => pure []
        | .rule 2167 [_, _, items] => cstExpressionList emit items
        | _ => .error "unsupported.cst_window_partition"
      pure (Json.mkObj [("name", name), ("partition_by", toJson partition),
        ("order_by", toJson (← cstOrdering emit order)), ("frame", Json.null)])
  | _ => .error "unsupported.cst_window"

/-- Preserve argument order for ordinary calls; named/variadic forms fail closed on this path. -/
def cstCallArguments (emit : GrammarTree → Except String Json) (tree : GrammarTree) : Except String (List Json) := do
  let items ← treeList [2220, 2221] 512 tree
  items.mapM fun item => match item with
    | .rule 2222 [expression] => emit expression
    | _ => .error "unsupported.cst_call_argument"

/-- Lower complete simple call productions, rejecting any hidden in-call sorting. -/
def cstCall (source : String) (emit : GrammarTree → Except String Json) : GrammarTree → Except String Json
  | .rule 2061 [name, _, _] => do
      pure (node "call" [("name", toJson (← identifierParts source name)), ("args", toJson ([] : List Json))])
  | .rule 2062 [name, _, arguments, sorting, _] => do
      requireEmpty sorting
      pure (node "call" [("name", toJson (← identifierParts source name)),
        ("args", toJson (← cstCallArguments emit arguments))])
  | _ => .error "unsupported.cst_call"

/-- Add a window only after accounting for every function decoration. -/
def cstFunction (source : String) (emit : GrammarTree → Except String Json) : GrammarTree → Except String Json
  | .rule 2068 [application, withinGroup, filter, over] => do
      requireEmpty withinGroup
      requireEmpty filter
      let function ← cstCall source emit application
      match over with
      | .rule 2163 [] => pure function
      | .rule 2161 [_, window] => pure (node "window_call"
          [("function", function), ("window", ← cstWindow source emit window)])
      | _ => .error "unsupported.cst_over"
  | .rule 2106 [_, _, arguments, _] => do
      pure (node "coalesce" [("args", toJson (← cstExpressionList emit arguments))])
  | .rule 2105 [_, _, left, _, right, _] => do
      pure (node "null_if" [("left", ← emit left), ("right", ← emit right)])
  | _ => .error "unsupported.cst_function"

/-- Retain searched/simple CASE branches and the explicit absence of ELSE. -/
def cstCase (emit : GrammarTree → Except String Json) : GrammarTree → Except String Json
  | .rule 2260 [_, operand, branches, otherwise, _] => do
      let operand ← match operand with
        | .rule 2267 [] => pure Json.null | .rule 2266 [value] => emit value
        | _ => .error "unsupported.cst_case_operand"
      let branches ← (← treeList [2261, 2262] 512 branches).mapM fun branch => do
        match branch with
        | .rule 2263 [_, condition, _, result] =>
            pure (Json.mkObj [("when", ← emit condition), ("then", ← emit result)])
        | _ => throw "unsupported.cst_case_branch"
      let otherwise ← match otherwise with
        | .rule 2265 [] => pure Json.null | .rule 2264 [_, value] => emit value
        | _ => .error "unsupported.cst_case_else"
      pure (node "case" [("operand", operand), ("branches", toJson branches), ("else", otherwise)])
  | _ => .error "unsupported.cst_case"

/-- Interpret only explicitly listed scalar operator spellings from certified terminal spans. -/
def cstOperator (source : String) (tree : GrammarTree) : Except String String := do
  match (← treeSource source tree).toLower with
  | "=" => pure "eq" | "<>" | "!=" => pure "ne" | "<" => pure "lt" | "<=" => pure "le"
  | ">" => pure "gt" | ">=" => pure "ge" | "+" => pure "add" | "-" => pure "subtract"
  | "*" => pure "multiply" | "/" => pure "divide" | "and" => pure "and" | "or" => pure "or"
  | "||" => pure "concat" | _ => .error "unsupported.cst_operator"

/-- Preserve query and list membership, including NOT as the original unary operator. -/
def cstIn (emit query : GrammarTree → Except String Json) (arg input : GrammarTree)
    (negated : Bool) : Except String Json := do
  let value ← match input with
    | .rule 2258 [body] => pure (node "in_query" [("arg", ← emit arg), ("query", ← query body)])
    | .rule 2259 [_, values, _] => pure (node "in_list"
        [("arg", ← emit arg), ("values", toJson (← cstExpressionList emit values))])
    | _ => throw "unsupported.cst_in"
  pure (if negated then node "unary" [("op", toJson "not"), ("arg", value)] else value)

/-- Preserve ARRAY literal nesting when its elements contain extended named-type syntax. -/
def cstArray (emit : GrammarTree → Except String Json) : Nat → GrammarTree → Except String Json
  | 0, _ => .error "resource.cst_array_depth"
  | _ + 1, .rule 2229 [_, values, _] => do
      pure (node "array" [("elements", toJson (← cstExpressionList emit values))])
  | fuel + 1, .rule 2230 [_, rows, _] => do
      let rows ← (← treeList [2232, 2233] 512 rows).mapM (cstArray emit fuel)
      pure (node "array" [("elements", toJson rows)])
  | _ + 1, .rule 2231 [_, _] => pure (node "array" [("elements", toJson ([] : List Json))])
  | _, _ => .error "unsupported.cst_array_literal"

/-- Lower array-query and predicate extensions while preserving every operand. -/
def cstPredicate (source : String) (emit query : GrammarTree → Except String Json) : GrammarTree → Except String Json
  | .rule 2055 [_, body] => do pure (node "exists" [("query", ← query body)])
  | .rule 2056 [_, body] => do pure (node "array_query" [("query", ← query body)])
  | .rule 2057 [_, values] => cstArray emit 512 values
  | .rule 2011 [arg, _, input] => cstIn emit query arg input false
  | .rule 2012 [arg, _, _, input] => cstIn emit query arg input true
  | .rule 1994 [arg, _, _] => do pure (node "is_null" [("arg", ← emit arg), ("negated", toJson false)])
  | .rule 1996 [arg, _, _, _] => do pure (node "is_null" [("arg", ← emit arg), ("negated", toJson true)])
  | .rule 2014 [left, op, quantifier, _, right, _] => do
      let quantifier ← treeSource source quantifier
      unless ["any", "some", "all"].contains quantifier.toLower do throw "unsupported.cst_quantifier"
      pure (node "quantified_comparison" [("op", toJson (← cstOperator source op)),
        ("quantifier", toJson quantifier.toLower), ("left", ← emit left), ("right", ← emit right)])
  | _ => .error "unsupported.cst_predicate"

/-- Route nontrivial expression families without accepting an arbitrary grammar wrapper. -/
def cstExpressionOther (source : String) (emit query : GrammarTree → Except String Json)
    (tree : GrammarTree) : Except String Json :=
  match treeName tree with
  | "case_expr" => cstCase emit tree
  | "func_expr" | "func_expr_common_subexpr" => cstFunction source emit tree
  | _ => cstPredicate source emit query tree

/-- Dispatch selected non-upstream expression forms by pinned complete production shape. -/
def cstExpressionShape (source : String) (emit query : GrammarTree → Except String Json)
    (tree : GrammarTree) : Except String Json := do
  match tree with
  | .rule 2078 [_] => pure (node "context_value" [("value", toJson "current_timestamp")])
  | .rule 2024 [_] => pure (node "default_value")
  | .rule 2053 [body] => do pure (node "scalar_subquery" [("query", ← query body)])
  | .rule 1959 [arg, _, type] => pure (node "cast" [("arg", ← emit arg), ("type", ← cstDataType source type)])
  | .rule 1980 [_, arg] => pure (node "unary" [("op", toJson "not"), ("arg", ← emit arg)])
  | .rule 2050 [_, arg, _, indirection] => do requireEmpty indirection; emit arg
  | .rule id [arg] =>
      if [1958, 2051, 2052, 2070, 2222].contains id then emit arg
      else throw s!"unsupported.cst_expression.{id}"
  | .rule id [left, op, right] =>
      if [1964, 1965, 1966, 1967, 1970, 1971, 1972, 1973, 1974, 1975, 1976, 1978, 1979].contains id then
        pure (node "binary" [("op", toJson (← cstOperator source op)), ("left", ← emit left), ("right", ← emit right)])
      else cstExpressionOther source emit query tree
  | _ => cstExpressionOther source emit query tree


end SQLollaParser
