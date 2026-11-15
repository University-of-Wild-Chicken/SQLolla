import SQLollaParser

/-! Native one-shot parser transport. The supervising Rust process owns deadlines. -/

/-- Refuse command arguments so SQL remains on the bounded private stdin transport. -/
def main (args : List String) : IO UInt32 := do
  if !args.isEmpty then
    IO.println (SQLollaParser.encodeResponse (SQLollaParser.inputFailure "input.arguments_forbidden"))
    return 2
  IO.println (SQLollaParser.encodeResponse (← SQLollaParser.run))
  pure 0
