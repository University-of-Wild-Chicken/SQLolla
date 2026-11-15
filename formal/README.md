# SQLolla M0 Lean specification

This is a specification/proof workspace, not production SQL execution code.
The pinned compiler is Lean 4.34.0; no Mathlib or external Lean dependency is
required by this library. SQLeanParser is exercised in the separate M0 probe.

From PowerShell in this directory, run:

```powershell
& .\verify.ps1
```

The default uses the checksum-verified local toolchain downloaded for M0. To use
another pinned installation, supply `-LakePath` with its absolute Lake path.
The script writes [formal evidence](../m0/formal/report.json), build and trust logs.
For another OS, use Lean 4.34.0 and run `lake build` and
`lake env lean TrustAudit.lean`; the PowerShell source/inventory gate also needs
to run before making the full acceptance claim.

`SQLolla.lean` imports every specification module. `TrustAudit` is a default Lake
target and inspects every declaration owned by these modules, including private
and generated declarations. Transitive logical dependencies must be limited to
`propext`, `Classical.choice`, and `Quot.sound`. The gate also rejects direct custom
axioms and unsafe declarations. Source checking is conservative and can reject
forbidden words even in literal text; it must never be weakened to permit proof
shortcuts. The three listed Lean foundations are not custom axioms introduced by
this project.

The results are small proved contracts: duplicates are retained, UNKNOWN differs
from FALSE, rejected transactions stop later writes, rollback preserves exposed
events and abort status, frames preserve untouched keys, and bounded claims
require nonvacuity. Encoding results are conditional on explicit correspondence
and completeness hypotheses. They do not establish those hypotheses for a future
Rust compiler, Java bridge, PostgreSQL engine or SAT solver.

Instance presentation contracts distinguish examples and counterexamples and
require displayed facts and AI references to come from the decoded instance.
They do not prove that AI prose is true or that a graphical renderer is correct.

To add a module, give it purpose documentation, keep declarations small, add its
import to `SQLolla.lean`, state assumptions explicitly, discharge proofs, and run
`verify.ps1`. Never replace a failed proof with a new axiom or compiler-trust
shortcut. Independent reviews belong in `m0/` until a later production review
process is established.
