import SQLean.PostgresGrammar
import SQLean.AdvancedParser
import SQLollaParser.ExtendedStatement

/-! Exact-source access to the pinned PostgreSQL grammar certificate, before typed lowering. -/
open Lean (Json toJson)
open SQLean.PostgresGrammar
namespace SQLollaParser

/-- Find a pinned grammar symbol label without interpreting its SQL meaning. -/
def grammarName (symbol : Symbol) : String :=
  match symbol with
  | .terminal kind => (Generated.terminals.find? (·.symbol == kind)).map (·.name) |>.getD "unknown"
  | .nonterminal kind => (Generated.nonterminals.find? (·.2 == kind)).map (·.1) |>.getD "unknown"

/-- Identify a CST node by its certified production or terminal symbol. -/
def treeName : GrammarTree → String
  | .terminal token => grammarName (.terminal token.kind)
  | .rule production _ => match Generated.grammar.productions[production]? with
      | some definition => grammarName (.nonterminal definition.lhs)
      | none => "unknown"

/-- Preserve direct child order, avoiding recursive descendant matching that could lose scope. -/
def treeChildren : GrammarTree → List GrammarTree
  | .terminal _ => []
  | .rule _ children => children

/-- Collect exact terminal spans with a bounded traversal of grammar wrappers. -/
def treeTokens : Nat → GrammarTree → Except String (List Token)
  | 0, _ => .error "resource.cst_depth"
  | _ + 1, .terminal token => pure [token]
  | fuel + 1, .rule _ children => do pure ((← children.mapM (treeTokens fuel)).flatten)

/-- Slice original UTF-8 bytes across a subtree; no token substitution or SQL rewriting occurs. -/
def treeSource (source : String) (tree : GrammarTree) : Except String String := do
  let tokens ← treeTokens 512 tree
  match tokens.head?, tokens.getLast? with
  | some first, some last =>
      let some exact := String.fromUTF8? (source.toUTF8.extract first.startOffset last.endOffset)
        | throw "invariant.cst_utf8_span"
      pure exact
  | none, none => pure ""
  | _, _ => throw "invariant.cst_span"

/-- Locate exactly one direct child of a requested grammar family. -/
def treeChild (name : String) (tree : GrammarTree) : Except String GrammarTree :=
  match (treeChildren tree).filter (fun child => treeName child == name) with
  | [child] => pure child
  | _ => .error s!"unsupported.cst_child.{name}"

/-- Require an exact empty grammar production before treating an optional clause as absent. -/
def requireEmpty (tree : GrammarTree) : Except String Unit :=
  if (treeChildren tree).isEmpty then pure () else .error s!"unsupported.cst.{treeName tree}"

/-- Decode a selected exact identifier subtree using the upstream identifier rules. -/
def identifierParts (source : String) (tree : GrammarTree) : Except String (List String) := do
  let exact ← treeSource source tree
  match SQLean.Advanced.parseExpression exact with
  | .ok (.column qualifier name schema) => pure (schema.toList ++ qualifier.toList ++ [name])
  | _ => .error "unsupported.cst_identifier"

/-- Require a single decoded identifier where qualification would change syntax meaning. -/
def identifier (source : String) (tree : GrammarTree) : Except String String := do
  match ← identifierParts source tree with
  | [name] => pure name
  | _ => .error "unsupported.cst_qualified_identifier"

/-- Flatten only explicitly pinned list productions, never arbitrary descendant nodes. -/
def treeList (allowed : List Nat) : Nat → GrammarTree → Except String (List GrammarTree)
  | 0, _ => .error "resource.cst_list_depth"
  | fuel + 1, .rule production children => do
      unless allowed.contains production do throw s!"unsupported.cst_list.{production}"
      match children with
      | [item] => pure [item]
      | [prior, .terminal _, item] => pure ((← treeList allowed fuel prior) ++ [item])
      | [prior, item] => pure ((← treeList allowed fuel prior) ++ [item])
      | _ => throw s!"unsupported.cst_list_shape.{production}"
  | _, _ => .error "unsupported.cst_list_terminal"

/-- Build a complete undecorated query wrapper, with every modifier explicitly absent. -/
def plainQuery (body : Json) : Json :=
  node "query" [("body", body), ("ctes", toJson ([] : List Json)), ("recursive", toJson false),
    ("order_by", toJson ([] : List Json)), ("limit", Json.null), ("offset", Json.null),
    ("offset_first", toJson false), ("locking", toJson ([] : List Json))]

/-- Replace one known syntax field without appending duplicate JSON object keys. -/
def replaceField (value : Json) (key : String) (replacement : Json) : Except String Json := do
  let object ← value.getObj?.mapError (fun _ => "invariant.cst_json_object")
  unless (object.get? key).isSome do throw s!"invariant.cst_json_field.{key}"
  pure (Json.mkObj (object.toList.map fun (name, old) =>
    (name, if name == key then replacement else old)))

/-- Count only declared unsized array suffixes; fixed bounds require their own contract. -/
def arrayDimensions : Nat → GrammarTree → Except String Nat
  | 0, _ => .error "resource.cst_type_depth"
  | _ + 1, .rule 1885 [] => pure 0
  | fuel + 1, .rule 1883 [prior, _, _] => do pure ((← arrayDimensions fuel prior) + 1)
  | _, _ => .error "unsupported.cst_array_dimensions"

/-- Retain custom type identity and array wrappers when the upstream closed type list refuses it. -/
def namedTypeJson (source : String) : GrammarTree → Except String Json
  | .rule 1877 [.rule 1886 [.rule 1897 [name, modifiers]], dimensions] => do
      requireEmpty modifiers
      let name := node "named_type" [("name", toJson (← identifierParts source name))]
      let wrapped := (List.range (← arrayDimensions 64 dimensions)).foldl
        (fun element _ => node "array_type" [("element", element)]) name
      pure (Json.mkObj [("name", wrapped), ("modifiers", toJson ([] : List Nat))])
  | _ => .error "unsupported.cst_named_type"

/-- Preserve explicit BIT length as a natural modifier; computed typmods remain unsupported. -/
def bitLength (source : String) (tree : GrammarTree) : Except String (List Nat) := do
  let values ← treeList [2218, 2219] 512 tree
  values.mapM fun value => do
    match SQLean.Advanced.parseExpression (← treeSource source value) with
    | .ok (.literal (.int size)) => pure size
    | _ => throw "unsupported.cst_bit_modifier"

/-- Preserve fixed/varying BIT syntax and array wrappers as explicit scalar type metadata. -/
def bitTypeJson (source : String) : GrammarTree → Except String Json
  | .rule 1877 [.rule 1888 [bit], dimensions] => do
      let (varying, modifiers) ← match bit with
        | .rule 1914 [.rule 1918 [_, varying, _, length, _]] => pure (varying, ← bitLength source length)
        | .rule 1915 [.rule 1919 [_, varying]] => pure (varying, [])
        | _ => throw "unsupported.cst_bit_type"
      let type ← match varying with
        | .rule 1933 [] => pure "bit" | .rule 1932 [_] => pure "varbit"
        | _ => throw "unsupported.cst_bit_varying"
      let name := node "scalar_type" [("name", toJson type)]
      let wrapped := (List.range (← arrayDimensions 64 dimensions)).foldl
        (fun element _ => node "array_type" [("element", element)]) name
      pure (Json.mkObj [("name", wrapped), ("modifiers", toJson modifiers)])
  | tree => namedTypeJson source tree

/-- Reuse exact supported type subtrees before handling the explicitly pinned named-type extension. -/
def cstDataType (source : String) (tree : GrammarTree) : Except String Json := do
  match SQLean.Advanced.parseDataType (← treeSource source tree) with
  | .ok type => dataTypeJson type
  | .error _ => bitTypeJson source tree

end SQLollaParser
