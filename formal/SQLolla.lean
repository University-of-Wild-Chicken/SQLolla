import SQLolla.Truth
import SQLolla.Relation
import SQLolla.IR
import SQLolla.Transaction
import SQLolla.Analysis
import SQLolla.Encoding
import SQLolla.Examples
import SQLolla.Instance

/-!
# SQLolla M0 specification

This library specifies small semantic contracts and their proof obligations.
It is not a PostgreSQL implementation or a verified SQL-to-Alloy compiler.
Its database execution relation and encoding correspondence must be supplied
and validated by later milestones. No parser or SAT solver is trusted here
to manufacture Lean proofs.
-/
