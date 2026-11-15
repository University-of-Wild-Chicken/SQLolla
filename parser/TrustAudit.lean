import SQLollaParser.Main
import Lean
import Lean.Util.CollectAxioms

/-! Compiled gate for every authored adapter declaration, including generated ones.
Logical foundations are distinct from trusted compiler/runtime execution behavior.
-/

open Lean Elab Command

/-- Permit only Lean's documented standard logical foundations. -/
def approvedFoundation (name : Name) : Bool :=
  [`propext, `Classical.choice, `Quot.sound].contains name

/-- Identify declarations by their source module, including private/generated names. -/
def adapterOwned (environment : Environment) (name : Name) : Bool :=
  match environment.getModuleIdxFor? name with
  | none => false
  | some index =>
      let owner := environment.header.moduleNames[index]!.toString
      owner == "Main" || owner == "SQLollaParser" || owner.startsWith "SQLollaParser."

run_cmd do
  let environment ← getEnv
  let entries := environment.constants.toList.filter fun entry => adapterOwned environment entry.1
  let mut used : Array Name := #[]
  for (name, info) in entries do
    if let .axiomInfo _ := info then throwError "Custom foundation rejected: {name}"
    if info.isUnsafe then throwError "Unsafe declaration rejected: {name}"
    for dependency in ← collectAxioms name do
      unless approvedFoundation dependency do
        throwError "Unapproved foundation in {name}: {dependency}"
      unless used.contains dependency do used := used.push dependency
  unless entries.length > 0 do throwError "Empty adapter audit rejected"
  logInfo m!"PARSER_TRUST_PASS declarations={entries.length} foundations={used}"
