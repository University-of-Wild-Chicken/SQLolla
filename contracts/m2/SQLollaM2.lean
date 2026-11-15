import SQLolla
import SQLollaM2.Outcome
import SQLollaM2.Scalar
import SQLollaM2.Crud
import SQLollaM2.Transaction
import SQLollaM2.Coverage
import SQLollaM2.Extensions
import SQLollaM2.Witnesses
import SQLollaM2.FiniteSearch
import SQLollaM2.Values
import SQLollaM2.Subquery
import SQLollaM2.Aggregates
import SQLollaM2.Locks
import SQLollaM2.Predicates
import SQLollaM2.Intervals
import SQLollaM2.Conflict
import SQLollaM2.WritableCte
import SQLollaM2.ScalarPropagation
import SQLollaM2.Ddl
import SQLollaM2.SessionContext
import SQLollaM2.Defaults
import SQLollaM2.MutationSource
import SQLollaM2.Enumeration
import SQLollaM2.SetReturning
import SQLollaM2.InstanceReview
import SQLollaM2.UniqueIndex
import SQLollaM2.SqlFunction
import SQLollaM2.Windows
import SQLollaM2.RelationalEffects
import SQLollaM2.Collections
import SQLollaM2.SessionEffects
import SQLollaM2.StringCollections
import SQLollaM2.SequenceEffects
import SQLollaM2.Comparability
import SQLollaM2.BranchEffects
import SQLollaM2.InsertionEffects
import SQLollaM2.RegexSubset
import SQLollaM2.FunctionResolution
import SQLollaM2.OutputLabels
import SQLollaM2.ConsumerLineage
import SQLollaM2.TupleVersion
import SQLollaM2.XidScalars
import SQLollaM2.BackendSignals
import SQLollaM2.RelationInitialization
import SQLollaM2.AssistanceBoundary

/-!
Maintained M2 contracts extend the existing M0 specification.
These definitions and conditional lemmas do not implement PostgreSQL execution
or establish preservation by the Rust interpreter, parser or Alloy encoder.
-/
