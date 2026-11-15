import Std

/-! A conservative lexical workload guard, not a second SQL parser.
Quoted text and comments do not consume syntax-depth or word budgets internally.
The upstream parser remains responsible for lexical and grammatical validity.
-/

namespace SQLollaParser

/-- Track only regions needed to exclude quoted text and comments from workload counts. -/
inductive ScanMode where
  | normal | singleQuoted | doubleQuoted | lineComment | blockComment
  deriving BEq

/-- Bound syntax nesting, nested comments and token-like workload before parsing. -/
structure ScanBudget where
  mode : ScanMode := .normal
  nesting : Nat := 0
  commentNesting : Nat := 0
  units : Nat := 0
  inWord : Bool := false

/-- Treat ordinary identifier/numeric runs as one workload unit without assigning SQL meaning. -/
def countCharacter (state : ScanBudget) (char : Char) : ScanBudget :=
  let word := char.isAlphanum || char == '_' || char == '$'
  let increment := if char.isWhitespace || (word && state.inWord) then 0 else 1
  { state with units := state.units + increment, inWord := word }

/-- Consume normal text, entering quoted/comment regions and counting visible delimiters. -/
def normalStep (state : ScanBudget) : List Char → ScanBudget × List Char
  | '-' :: '-' :: rest => ({state with mode := .lineComment, inWord := false}, rest)
  | '/' :: '*' :: rest => ({state with mode := .blockComment, commentNesting := 1, inWord := false}, rest)
  | '\'' :: rest => ({state with mode := .singleQuoted, units := state.units + 1, inWord := false}, rest)
  | '"' :: rest => ({state with mode := .doubleQuoted, units := state.units + 1, inWord := false}, rest)
  | '(' :: rest | '[' :: rest => ({state with nesting := state.nesting + 1, units := state.units + 1, inWord := false}, rest)
  | ')' :: rest | ']' :: rest => ({state with nesting := state.nesting - 1, units := state.units + 1, inWord := false}, rest)
  | char :: rest => (countCharacter state char, rest)
  | [] => (state, [])

/-- Respect doubled quote characters while excluding literal contents from syntax counts. -/
def quoteStep (state : ScanBudget) : List Char → ScanBudget × List Char
  | '\'' :: '\'' :: rest =>
      if state.mode == .singleQuoted then (state, rest) else (state, '\'' :: rest)
  | '"' :: '"' :: rest =>
      if state.mode == .doubleQuoted then (state, rest) else (state, '"' :: rest)
  | '\'' :: rest =>
      if state.mode == .singleQuoted then ({state with mode := .normal}, rest) else (state, rest)
  | '"' :: rest =>
      if state.mode == .doubleQuoted then ({state with mode := .normal}, rest) else (state, rest)
  | _ :: rest => (state, rest)
  | [] => (state, [])

/-- Skip line comments and account for nested block-comment delimiters. -/
def commentStep (state : ScanBudget) : List Char → ScanBudget × List Char
  | '\n' :: rest | '\r' :: rest =>
      if state.mode == .lineComment then ({state with mode := .normal}, rest) else (state, rest)
  | '/' :: '*' :: rest =>
      if state.mode == .blockComment then ({state with commentNesting := state.commentNesting + 1}, rest)
      else (state, rest)
  | '*' :: '/' :: rest =>
      if state.mode != .blockComment then (state, rest)
      else ({state with commentNesting := state.commentNesting - 1, mode := if state.commentNesting == 1 then .normal else .blockComment}, rest)
  | _ :: rest => (state, rest)
  | [] => (state, [])

/-- Refuse excessive lexical work before calling the upstream recursive-descent parser. -/
def scanSourceBudget : Nat → List Char → ScanBudget → Except String Unit
  | 0, [], _ => pure ()
  | 0, _, _ => .error "resource.source_steps"
  | _ + 1, [], _ => pure ()
  | fuel + 1, chars, state => do
      let (next, rest) := match state.mode with
        | .normal => normalStep state chars
        | .singleQuoted | .doubleQuoted => quoteStep state chars
        | .lineComment | .blockComment => commentStep state chars
      if next.nesting > 64 || next.commentNesting > 64 then throw "resource.source_depth"
      if next.units > 1024 then throw "resource.source_units"
      scanSourceBudget fuel rest next

/-- Apply the lexical workload guard while leaving malformed SQL diagnosis to SQLeanParser. -/
def checkSourceBudget (sql : String) : Except String Unit :=
  let chars := sql.toList
  scanSourceBudget (chars.length + 1) chars {}

end SQLollaParser
