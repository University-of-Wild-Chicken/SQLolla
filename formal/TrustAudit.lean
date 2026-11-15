import SQLolla
import Lean
import Lean.Util.CollectAxioms

/-!
# Compiled trust gate

Inspect every compiled SQLolla declaration, including generated declarations.
Only Lean's three documented foundational dependencies are permitted. A new
declaration of an axiom in SQLolla or another transitive dependency is rejected.
The audit is a build-time inspection tool, not an extra logical assumption.
-/

open Lean Elab Command

/-- The explicit foundational allowlist does not permit compiler-trust shortcuts. -/
def approvedFoundation (name : Name) : Bool :=
  [`propext, `Classical.choice, `Quot.sound].contains name

/-- Module ownership also captures private declarations and names outside a namespace. -/
def ownedDeclaration (environment : Environment) (name : Name) : Bool :=
  match environment.getModuleIdxFor? name with
  | none => false
  | some index =>
    let owner := environment.header.moduleNames[index]!.toString
    owner == "SQLolla" || owner.startsWith "SQLolla."

run_cmd do
  let environment ← getEnv
  let entries := environment.constants.toList.filter fun entry =>
    ownedDeclaration environment entry.1
  let mut theoremCount : Nat := 0
  let mut used : Array Name := #[]
  for (name, info) in entries do
    if let .axiomInfo _ := info then
      throwError "Custom axiom declaration rejected: {name}"
    if info.isUnsafe then
      throwError "Unsafe declaration rejected: {name}"
    let dependencies ← collectAxioms name
    for dependency in dependencies do
      unless approvedFoundation dependency do
        throwError "Unapproved foundation in {name}: {dependency}"
      unless used.contains dependency do used := used.push dependency
    if let .thmInfo _ := info then
      theoremCount := theoremCount + 1
      logInfo m!"AUDITED_THEOREM {name}: {dependencies}"
  unless entries.length > 0 do throwError "Empty audit scope rejected"
  logInfo m!"TRUST_PASS declarations={entries.length} theorems={theoremCount} foundations={used}"
