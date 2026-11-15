/**
 * Exact before/after/output relation tables. Every row occurrence is separate,
 * duplicate rows are not collapsed, typed NULL is explicit, and supplied facts
 * remain available from the table view.
 */
import { useMemo, type JSX } from "react";
import { canonicalJson, cellText } from "../analysis/format";
import type { CellModel, ColumnModel, EvidenceView, EffectObservation, OccurrenceView, RelationView } from "../analysis/types";

interface TableProps {
  readonly relation: RelationView;
  readonly label: string;
  readonly onInspectFact: (reference: string) => void;
  readonly note?: string;
}

/** Render a typed cell; typed NULL remains explicit. */
function CellValue({ cell }: { readonly cell: CellModel }): JSX.Element {
  if (cell.type === "null")
    return (
      <span className="typed-null">
        <span>{cellText(cell)}</span>
      </span>
    );
  return <span className="cell-value">{cellText(cell)}</span>;
}

/** Group exact duplicate occurrences without collapsing rows. */
function duplicateCounts(rows: readonly OccurrenceView[]): ReadonlyMap<string, number> {
  const counts = new Map<string, number>();
  for (const row of rows) {
    const key = canonicalJson(row.cells.map((cell) => cell.raw));
    counts.set(key, (counts.get(key) ?? 0) + 1);
  }
  return counts;
}

/** Derive complete display columns even when metadata is absent. */
function tableColumns(relation: RelationView): readonly ColumnModel[] {
  const width = Math.max(relation.columns.length, ...relation.rows.map((row) => row.cells.length));
  return Array.from(
    { length: width },
    (_, index) =>
      relation.columns[index] ?? {
        name: `column_${index + 1}`,
        ty: "unknown",
        nullable: true,
        qualifier: null,
        schema: null,
        raw: null,
      },
  );
}

/** Render a supplied fact link. */
function FactCell({ reference, onInspectFact }: { readonly reference: string | null; readonly onInspectFact: (reference: string) => void }): JSX.Element {
  return reference ? (
    <button type="button" className="fact-link" onClick={() => onInspectFact(reference)}>
      Inspect fact
    </button>
  ) : (
    <span className="cell-missing">not supplied</span>
  );
}

/** Render one row's occurrence identity. */
function RowIdentity({ position, duplicateCount }: { readonly position: number; readonly duplicateCount: number }): JSX.Element {
  return (
    <th scope="row">
      <span className="occurrence-number">#{position}</span>
      {duplicateCount > 1 ? <span className="duplicate-badge">duplicate ×{duplicateCount}</span> : null}
    </th>
  );
}

/** Render one row's typed cells. */
function RowCells({ row, columns }: { readonly row: OccurrenceView; readonly columns: readonly ColumnModel[] }): JSX.Element {
  return (
    <>
      {columns.map((column, index) => (
        <td key={`${column.name}-${index}`} className={row.changedColumns.includes(index) ? "cell-changed" : undefined}>
          {row.cells[index] ? <CellValue cell={row.cells[index]} /> : <span className="cell-missing">—</span>}
        </td>
      ))}
    </>
  );
}

/** Render one row occurrence. */
function OccurrenceRow({
  row,
  columns,
  duplicateCount,
  onInspectFact,
}: {
  readonly row: OccurrenceView;
  readonly columns: readonly ColumnModel[];
  readonly duplicateCount: number;
  readonly onInspectFact: (reference: string) => void;
}): JSX.Element {
  return (
    <tr>
      <RowIdentity position={row.position} duplicateCount={duplicateCount} />
      <RowCells row={row} columns={columns} />
      <td>
        <FactCell reference={row.factReference} onInspectFact={onInspectFact} />
      </td>
    </tr>
  );
}

/** Render one relation's table header. */
function RelationTableHead({ columns }: { readonly columns: readonly ColumnModel[] }): JSX.Element {
  return (
    <thead>
      <tr>
        <th scope="col">Occurrence</th>
        {columns.map((column) => (
          <th key={column.name} scope="col">
            {column.name}
            <span className="column-type">
              {column.ty}
              {column.nullable ? " · nullable" : ""}
            </span>
          </th>
        ))}
        <th scope="col">Fact</th>
      </tr>
    </thead>
  );
}

/** Render one relation table. */
export function RelationTable({ relation, label, onInspectFact, note }: TableProps): JSX.Element {
  const counts = useMemo(() => duplicateCounts(relation.rows), [relation.rows]);
  const columns = tableColumns(relation);
  return (
    <section className="relation-card" aria-label={`${label} ${relation.tableId}`}>
      <RelationHead relation={relation} label={label} onInspectFact={onInspectFact} note={note} />
      <div className="table-scroll">
        <table>
          <caption>
            {relation.tableId} {label.toLowerCase()} rows. Each occurrence is a separate row.
          </caption>
          <RelationTableHead columns={columns} />
          <tbody>
            {relation.rows.map((row) => (
              <OccurrenceRow
                key={`${row.ordinal}-${row.factReference ?? "row"}`}
                row={row}
                columns={columns}
                duplicateCount={counts.get(canonicalJson(row.cells.map((cell) => cell.raw))) ?? 1}
                onInspectFact={onInspectFact}
              />
            ))}
          </tbody>
        </table>
      </div>
    </section>
  );
}

/** Render relation heading and optional note. */
function RelationHead({ relation, label, note }: TableProps): JSX.Element {
  return (
    <div className="relation-card__head">
      <div>
        <span className="mono-label">{label}</span>
        <h4>{relation.tableId}</h4>
        <p>
          {relation.rows.length} row occurrence{relation.rows.length === 1 ? "" : "s"} · supplied schema {relation.schemaSupplied ? "present" : "missing"}
        </p>
      </div>
      {note ? <p className="relation-note">{note}</p> : null}
    </div>
  );
}

/** Render corresponding before/after tables. */
function BeforeAfter({
  table,
  after,
  onInspectFact,
}: {
  readonly table: RelationView;
  readonly after: RelationView | undefined;
  readonly onInspectFact: (reference: string) => void;
}): JSX.Element {
  return (
    <div className="before-after">
      <RelationTable relation={table} label="BEFORE" onInspectFact={onInspectFact} note="Input occurrence order as supplied; no row identity is invented." />
      {after ? (
        <RelationTable
          relation={after}
          label="AFTER"
          onInspectFact={onInspectFact}
          note="Changed-cell marks compare positional occurrences only; they are not row lineage."
        />
      ) : null}
    </div>
  );
}

/** Render supplied effect fields. */
function EffectFields({ fields }: { readonly fields: EffectObservation["fields"] }): JSX.Element {
  return (
    <dl>
      {Object.entries(fields).map(([key, value]) => (
        <div key={key}>
          <dt>{key}</dt>
          <dd>{value}</dd>
        </div>
      ))}
    </dl>
  );
}

/** Render one retained effect. */
function EffectCard({ effect, onInspectFact }: { readonly effect: EffectObservation; readonly onInspectFact: (reference: string) => void }): JSX.Element {
  return (
    <article className="effect-card">
      <h5>{effect.label}</h5>
      <EffectFields fields={effect.fields} />
      {effect.reference ? (
        <button type="button" className="fact-link" onClick={() => onInspectFact(effect.reference!)}>
          Inspect effect fact
        </button>
      ) : (
        <p>No fact reference supplied.</p>
      )}
    </article>
  );
}

/** Render retained effects. */
function EffectsSection({
  effects,
  onInspectFact,
}: {
  readonly effects: readonly EffectObservation[];
  readonly onInspectFact: (reference: string) => void;
}): JSX.Element | null {
  if (effects.length === 0) return null;
  return (
    <section className="effect-section" aria-label="Retained effects">
      <div className="section-heading">
        <span className="mono-label">RETAINED EFFECTS</span>
        <h4>Observations that live outside the result rows</h4>
      </div>
      <div className="effect-grid">
        {effects.map((effect, index) => (
          <EffectCard key={`${effect.kind}-${index}`} effect={effect} onInspectFact={onInspectFact} />
        ))}
      </div>
    </section>
  );
}

/** Adapt normalized output rows to a relation view. */
function outputRelation(evidence: EvidenceView): RelationView {
  return {
    tableId: "result (statement output)",
    schemaName: "statement",
    tableName: "output",
    columns: evidence.output.columns,
    rows: evidence.output.rows,
    schemaSupplied: true,
  };
}

/** Render the output relation. */
function OutputTable({ evidence, onInspectFact }: { readonly evidence: EvidenceView; readonly onInspectFact: (reference: string) => void }): JSX.Element {
  const ordering =
    evidence.output.ordering === "ordered"
      ? "ordered result positions"
      : evidence.output.ordering === "bag"
        ? "bag; supplied order is presentation only"
        : "not supplied";
  return <RelationTable relation={outputRelation(evidence)} label="OUTPUT" onInspectFact={onInspectFact} note={`Ordering: ${ordering}.`} />;
}

/** Render rows exposed before a modeled failure. */
function ExposedRows({
  evidence,
  onInspectFact,
}: {
  readonly evidence: EvidenceView;
  readonly onInspectFact: (reference: string) => void;
}): JSX.Element | null {
  if (!evidence.statement.error || evidence.statement.error.exposedRows.length === 0) return null;
  const relation = {
    ...outputRelation(evidence),
    tableId: "exposed (before failure)",
    rows: evidence.statement.error.exposedRows,
  };
  return (
    <RelationTable
      relation={relation}
      label="EXPOSED"
      onInspectFact={onInspectFact}
      note="These rows were exposed before the modeled failure; rollback does not erase the observation."
    />
  );
}

/** Render the complete table tab. */
export function RelationTables({
  evidence,
  onInspectFact,
}: {
  readonly evidence: EvidenceView;
  readonly onInspectFact: (reference: string) => void;
}): JSX.Element {
  const poststate = new Map(evidence.statement.poststateTables.map((table) => [table.tableId, table]));
  return (
    <div className="relation-stack">
      {evidence.inputTables.map((table) => (
        <BeforeAfter key={table.tableId} table={table} after={poststate.get(table.tableId)} onInspectFact={onInspectFact} />
      ))}
      <ExposedRows evidence={evidence} onInspectFact={onInspectFact} />
      <EffectsSection effects={evidence.statement.effects} onInspectFact={onInspectFact} />
      {evidence.output.rows.length > 0 || evidence.output.columns.length > 0 ? (
        <OutputTable evidence={evidence} onInspectFact={onInspectFact} />
      ) : (
        <p className="empty-note">No result occurrences were supplied for this stored observation.</p>
      )}
    </div>
  );
}
