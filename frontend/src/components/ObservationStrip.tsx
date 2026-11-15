/**
 * Compact, always-visible statement observation strip. Errors and retained
 * effects are never hidden behind the graph/table switch.
 */
import type { JSX } from "react";
import type { ErrorObservation, EvidenceView, StatementView } from "../analysis/types";

interface ObservationRowsProps {
  readonly rows: readonly (readonly [string, string | number])[];
}

/** Render compact key/value statement observation rows. */
function ObservationRows({ rows }: ObservationRowsProps): JSX.Element {
  return (
    <dl>
      {rows.map(([label, value]) => (
        <div key={label}>
          <dt>{label}</dt>
          <dd>{value}</dd>
        </div>
      ))}
    </dl>
  );
}

/** Render a modeled SQL rejection with its supplied diagnostics. */
function SqlErrorStrip({ error }: { readonly error: ErrorObservation }): JSX.Element {
  const rows = [
    ["Phase", error.phase],
    ...(error.constraintName ? [["Constraint", error.constraintName] as const] : []),
    ...(error.columnName ? [["Column", error.columnName] as const] : []),
    ["Exposed rows", error.exposedRows.length],
  ] as const;
  return (
    <section className="observation-strip observation-strip--error" aria-label="SQL error observation">
      <div>
        <span className="mono-label">OBSERVED SQL ERROR</span>
        <strong>SQLSTATE {error.sqlstate}</strong>
      </div>
      <ObservationRows rows={rows} />
    </section>
  );
}

/** Render a successful finite observation's command and affected counts. */
function SuccessStrip({ statement }: { readonly statement: StatementView }): JSX.Element {
  const tables = statement.poststateTables.length;
  const rows = [
    ["Affected", statement.affected ?? "unknown"],
    ["Result shape", statement.ordering],
    ["Retained effects", statement.effects.length],
    ["Poststate", `${tables} table${tables === 1 ? "" : "s"}`],
  ] as const;
  return (
    <section className="observation-strip" aria-label="Statement observation">
      <div>
        <span className="mono-label">STATEMENT OBSERVATION</span>
        <strong>{statement.commandTag}</strong>
      </div>
      <ObservationRows rows={rows} />
    </section>
  );
}

/** Render the absence of a statement-level outcome without inference. */
function ReadStrip({ rows }: { readonly rows: number }): JSX.Element {
  return (
    <section className="observation-strip observation-strip--quiet" aria-label="Read observation">
      <div>
        <span className="mono-label">READ OBSERVATION</span>
        <strong>
          {rows} returned {rows === 1 ? "row" : "rows"}
        </strong>
      </div>
      <p>This read case contains output rows and replay evidence. It has no statement effects to review.</p>
    </section>
  );
}

/** Render statement outcome facts in plain language without safety inference. */
export function ObservationStrip({ evidence }: { readonly evidence: EvidenceView }): JSX.Element {
  const { statement } = evidence;
  if (statement.error) return <SqlErrorStrip error={statement.error} />;
  if (statement.outcome === "succeeded") return <SuccessStrip statement={statement} />;
  return <ReadStrip rows={evidence.output.rows.length} />;
}
