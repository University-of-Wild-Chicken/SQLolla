import SQLollaParser.Query
import SQLollaParser.ExtendedStatement
import SQLollaParser.CstLower
import SQLollaParser.SourceBudget
import SQLean.AdvancedParser

/-! One bounded UTF-8 SQL input yields one versioned JSON report.
No source SQL or credentials are accepted as command-line arguments or logged.
-/

open Lean (Json toJson)

namespace SQLollaParser

/-- The wire version changes when node meaning, fields or acceptance rules change. -/
def protocolVersion : String := "sqlolla.parser.v1"

/-- The dependency revision checked by the build script and expected by Rust consumers. -/
def parserCommit : String := "ad15c585155ff7fc2c5fc4c9746905e4959ff5eb"

/-- Bound raw SQL transport before decoding or invoking the upstream parser. -/
def maxSourceBytes : Nat := 65536

/-- Envelope a complete protocol revision with its exact parser and syntax-only assurance. -/
def versionedEnvelope (version : String) (result : Json) : Json :=
  Json.mkObj [("protocol", toJson version), ("parser_commit", toJson parserCommit),
    ("assurance", toJson "syntax-only"), ("semantic_preservation", toJson false),
    ("result", result)]

/-- Preserve the original wire version for every previously admitted query and preflight error. -/
def envelope (result : Json) : Json := versionedEnvelope protocolVersion result

/-- Report transport refusal separately from SQL syntax or supported-AST decisions. -/
def inputFailure (code : String) : Json :=
  envelope (Json.mkObj [("parse_status", toJson "not_attempted"),
    ("export_status", toJson "unsupported"), ("diagnostic", toJson code)])

/-- Emit an AST only when every encountered node and modifier is represented. -/
def parsedReport (statement : SQLean.Advanced.Statement) : Json :=
  let fields := [("parse_status", toJson "parsed")]
  match statementJson statement with
  | .ok ast => Json.mkObj (fields ++ [("export_status", toJson "exported"), ("ast", ast)])
  | .error code => Json.mkObj (fields ++ [("export_status", toJson "unsupported"),
      ("diagnostic", toJson code)])

/-- Fall back to full typed syntax only under a new protocol; never relabel partial syntax v1. -/
def parsedEnvelope (statement : SQLean.Advanced.Statement) : Json :=
  match statementJson statement with
  | .ok _ => envelope (parsedReport statement)
  | .error _ => versionedEnvelope "sqlolla.parser.v2" (match statementV2 statement with
    | .ok ast => Json.mkObj [("parse_status", toJson "parsed"),
        ("export_status", toJson "exported"), ("ast", ast)]
    | .error code => Json.mkObj [("parse_status", toJson "parsed"),
        ("export_status", toJson "unsupported"), ("diagnostic", toJson code)])

/-- Every previously admitted typed statement retains its exact original envelope value. -/
theorem legacy_export_envelope_unchanged (statement : SQLean.Advanced.Statement) (ast : Json)
    (admitted : statementJson statement = .ok ast) :
    parsedEnvelope statement = envelope (parsedReport statement) := by
  simp [parsedEnvelope, admitted]

/-- Cap encoded responses without returning a partial AST or silently truncated JSON. -/
def encodeResponse (report : Json) : String :=
  let encoded := report.compress
  if encoded.toUTF8.size <= 1048576 then encoded
  else (envelope (Json.mkObj [("parse_status", toJson "parsed"),
    ("export_status", toJson "unsupported"), ("diagnostic", toJson "resource.response_bytes")])).compress

/-- Preserve upstream syntax locations when the complete PostgreSQL grammar also rejects the input. -/
def syntaxFailure (error : SQLean.ParseError) : Json :=
  envelope (Json.mkObj [("parse_status", toJson "rejected"),
    ("export_status", toJson "unsupported"), ("diagnostic", toJson "syntax.rejected"),
    ("line", toJson error.position.line), ("column", toJson error.position.column),
    ("offset", toJson error.position.offset)])

/-- Expose a new AST only after complete certified grammar parsing and explicit typed lowering. -/
def cstFallback (sql : String) (error : SQLean.ParseError) : Json :=
  match certifiedLower sql with
  | .rejected => syntaxFailure error
  | .inconclusive => inputFailure "resource.grammar_transitions"
  | .invalidContract => inputFailure "invariant.grammar_contract"
  | .parsed result => versionedEnvelope "sqlolla.parser.v2" (match result with
    | .ok ast => Json.mkObj [("parse_status", toJson "parsed"),
        ("export_status", toJson "exported"), ("ast", ast)]
    | .error code => Json.mkObj [("parse_status", toJson "parsed"),
        ("export_status", toJson "unsupported"), ("diagnostic", toJson code)])

/-- Parse a single SQL statement without claiming catalog validity or execution safety. -/
def analyzeSource (sql : String) : Json :=
  match checkSourceBudget sql with
  | .error code => inputFailure code
  | .ok _ => match SQLean.Advanced.parse sql with
    | .ok statement => parsedEnvelope statement
    | .error error => cstFallback sql error

/-- Validate encoding and reject NUL before the upstream parser sees the bytes. -/
def decodeSource (bytes : ByteArray) : Except String String := do
  if bytes.size > maxSourceBytes then throw "resource.source_bytes"
  let some sql := String.fromUTF8? bytes | throw "input.invalid_utf8"
  if sql.contains '\x00' then throw "input.nul"
  pure sql

/-- Consume at most one byte beyond the cap; each nonempty read consumes bounded fuel. -/
def readBounded : Nat → IO.FS.Stream → ByteArray → IO (Except String String)
  | 0, _, _ => pure (.error "resource.read_steps")
  | fuel + 1, stream, bytes => do
      let remaining := maxSourceBytes + 1 - bytes.size
      let chunk ← stream.read (USize.ofNat (min 4096 remaining))
      if chunk.isEmpty then return decodeSource bytes
      let accumulated := bytes ++ chunk
      if accumulated.size > maxSourceBytes then return .error "resource.source_bytes"
      readBounded fuel stream accumulated

/-- Run one request and convert I/O failures into a nonrevealing structured diagnostic. -/
def run : IO Json := do
  try
    match ← readBounded (maxSourceBytes + 2) (← IO.getStdin) .empty with
    | .ok sql => pure (analyzeSource sql)
    | .error code => pure (inputFailure code)
  catch _ => pure (inputFailure "input.io_failure")

end SQLollaParser
