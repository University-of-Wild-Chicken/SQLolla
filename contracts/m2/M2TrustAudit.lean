import SQLollaM2
import Lean
import Lean.Util.CollectAxioms

/-! Module ownership and transitive foundations are checked for the inherited and draft libraries. -/

open Lean Elab Command

/-- Restrict logical dependencies to the inherited documented foundations. -/
def approvedFoundation (name : Name) : Bool :=
  [`propext, `Classical.choice, `Quot.sound].contains name

/-- Recover declaration ownership independently of declaration namespaces. -/
def ownerOf (environment : Environment) (name : Name) : String :=
  match environment.getModuleIdxFor? name with
  | none => ""
  | some index => environment.header.moduleNames[index]!.toString

/-- Include all draft and inherited library declarations, including generated/private ones. -/
def auditedOwner (owner : String) : Bool :=
  owner == "SQLollaM2" || owner.startsWith "SQLollaM2." ||
  owner == "SQLolla" || owner.startsWith "SQLolla."

/-- Reject unapproved logical assumptions and privileged compiled declarations. -/
def auditOne (name : Name) (info : ConstantInfo) : CommandElabM (Array Name) := do
  if let .axiomInfo _ := info then throwError "Custom assumption declaration: {name}"
  if info.isUnsafe then throwError "Privileged declaration: {name}"
  let dependencies ← collectAxioms name
  for dependency in dependencies do
    unless approvedFoundation dependency do throwError "Unapproved foundation: {name}: {dependency}"
  return dependencies

run_cmd do
  let environment ← getEnv
  let mut draftCount : Nat := 0
  let mut draftTheorems : Nat := 0
  let mut inheritedCount : Nat := 0
  let mut foundations : Array Name := #[]
  for (name, info) in environment.constants.toList do
    let owner := ownerOf environment name
    if auditedOwner owner then
      let dependencies ← auditOne name info
      for dependency in dependencies do
        unless foundations.contains dependency do foundations := foundations.push dependency
      if owner == "SQLollaM2" || owner.startsWith "SQLollaM2." then
        draftCount := draftCount + 1
        if let .thmInfo _ := info then draftTheorems := draftTheorems + 1
      else inheritedCount := inheritedCount + 1
  unless draftCount > 0 do throwError "Empty draft audit scope"
  unless inheritedCount > 0 do throwError "Empty inherited audit scope"
  logInfo m!"M2_TRUST_PASS declarations={draftCount} theorems={draftTheorems} inherited={inheritedCount} foundations={foundations}"
