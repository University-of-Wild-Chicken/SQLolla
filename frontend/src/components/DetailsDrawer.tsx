import { stringifyJson } from "../analysis/json";
/**
 * Progressive-disclosure inspection for the SQL/IR/Alloy triple, fact index,
 * assurance/provenance and complete original JSON.
 */
import { useState, type JSX } from "react";
import { cellText, shortHash, typedValueText } from "../analysis/format";
import { describeAssurance } from "../analysis/status";
import type { EvidenceView, FactModel } from "../analysis/types";
import { Modal } from "./Modal";

/** Tabs available in the evidence inspector. */
export type DetailsTab = "sql" | "ir" | "alloy" | "facts" | "assurance" | "json";

/** Props for the evidence drawer. */
export interface DetailsDrawerProps {
  readonly open: boolean;
  readonly onClose: () => void;
  readonly evidence: EvidenceView | null;
  readonly initialTab?: DetailsTab;
  readonly focusFact?: string | null;
  readonly openRaw?: unknown;
  readonly nextRaw?: unknown;
  readonly native?: boolean;
}

const TABS: readonly { readonly id: DetailsTab; readonly label: string }[] = [
  { id: "sql", label: "SQL & scope" },
  { id: "ir", label: "Typed IR" },
  { id: "alloy", label: "Alloy" },
  { id: "facts", label: "Facts" },
  { id: "assurance", label: "Assurance" },
  { id: "json", label: "Original JSON" },
];

/** Offer a local JSON download and revoke the temporary URL immediately. */
function downloadJson(filename: string, value: unknown): void {
  const text = typeof value === "string" ? value : stringifyJson(value, 2);
  const url = URL.createObjectURL(new Blob([text], { type: "application/json" }));
  const anchor = document.createElement("a");
  anchor.href = url;
  anchor.download = filename;
  anchor.click();
  URL.revokeObjectURL(url);
}

/** Render the explicit fixture/authentication boundary. */
function FixtureBanner({ native }: { readonly native?: boolean }): JSX.Element {
  return (
    <div className="fixture-banner">
      <span className="fixture-pill">{native ? "LOCAL SOLVER EVIDENCE" : "DEMO EVIDENCE"}</span>
      <span>
        {native
          ? "Reopened from the local backend, which checks integrity and replays the stored result."
          : "Illustrative fixture. This browser does not authenticate its integrity or replay."}
      </span>
    </div>
  );
}

/** Render the inspector tab buttons. */
function DetailTabs({ tab, setTab }: { readonly tab: DetailsTab; readonly setTab: (tab: DetailsTab) => void }): JSX.Element {
  return (
    <nav className="detail-tabs" aria-label="Evidence detail views">
      {TABS.map((entry) => (
        <button key={entry.id} type="button" aria-pressed={tab === entry.id} onClick={() => setTab(entry.id)}>
          {entry.label}
        </button>
      ))}
    </nav>
  );
}

/** Render one readable code/text tab. */
function CodeTab({ title, text }: { readonly title: string; readonly text: string }): JSX.Element {
  return (
    <section className="detail-stack">
      <span className="mono-label">{title}</span>
      <pre className="code-block">{text}</pre>
    </section>
  );
}

/** Render typed SQL parameters. */
function ParameterTable({ evidence }: { readonly evidence: EvidenceView }): JSX.Element {
  const parameters = Object.entries(evidence.bundle.request.parameters);
  if (parameters.length === 0) return <p className="empty-note">No parameters supplied.</p>;
  return (
    <table className="params-table">
      <thead>
        <tr>
          <th>Identity</th>
          <th>Type</th>
          <th>Value</th>
        </tr>
      </thead>
      <tbody>
        {parameters.map(([key, cell]) => (
          <tr key={key}>
            <th scope="row">${key}</th>
            <td>{cell.type}</td>
            <td>
              <code>{typedValueText(cell)}</code>
            </td>
          </tr>
        ))}
      </tbody>
    </table>
  );
}

/** Render property, bounds and finite domain scope. */
function ScopeDetails({ evidence }: { readonly evidence: EvidenceView }): JSX.Element {
  const request = evidence.bundle.request;
  return (
    <dl className="detail-grid">
      <div>
        <dt>Mode</dt>
        <dd>{request.mode}</dd>
      </div>
      <div>
        <dt>Property</dt>
        <dd>{request.property ? stringifyJson(request.property.raw) : "none supplied"}</dd>
      </div>
      <div>
        <dt>Bounds</dt>
        <dd>{stringifyJson(request.bounds)}</dd>
      </div>
      <div>
        <dt>Domain</dt>
        <dd>{request.domain.map(cellText).join(" · ") || "none supplied"}</dd>
      </div>
    </dl>
  );
}

/** Render the explicit catalog schema. */
function SchemaDetails({ evidence }: { readonly evidence: EvidenceView }): JSX.Element {
  return (
    <>
      {Object.values(evidence.bundle.request.catalog.tables).map((table) => (
        <div className="schema-card" key={table.key}>
          <strong>{table.key}</strong>
          <p>{table.columns.map((column) => `${column.name}: ${column.ty}${column.nullable ? " NULL" : ""}`).join(" · ")}</p>
        </div>
      ))}
    </>
  );
}

/** Render exact SQL, parameters, property and schema. */
function SqlTab({ evidence }: { readonly evidence: EvidenceView }): JSX.Element {
  return (
    <div className="detail-stack">
      <section>
        <span className="mono-label">EXACT SQL</span>
        <pre className="code-block">{evidence.bundle.request.sql}</pre>
      </section>
      <section>
        <span className="mono-label">TYPED PARAMETERS</span>
        <ParameterTable evidence={evidence} />
      </section>
      <section>
        <span className="mono-label">PROPERTY & SCOPE</span>
        <ScopeDetails evidence={evidence} />
      </section>
      <section>
        <span className="mono-label">EXPLICIT SCHEMA CONTEXT</span>
        <SchemaDetails evidence={evidence} />
      </section>
    </div>
  );
}

/** Render one supplied fact row. */
function FactRow({
  fact,
  selected,
  onSelect,
}: {
  readonly fact: FactModel;
  readonly selected: boolean;
  readonly onSelect: (reference: string) => void;
}): JSX.Element {
  return (
    <details
      open={selected}
      className={selected ? "fact-row is-selected" : "fact-row"}
      onToggle={(event) => event.currentTarget.open && onSelect(fact.reference)}
    >
      <summary>
        <span className="mono-label">{fact.kind}</span>
        <code>{fact.reference}</code>
      </summary>
      <pre>{stringifyJson(fact.value, 2)}</pre>
    </details>
  );
}

/** Render fact consistency warnings. */
function FactWarning({ evidence }: { readonly evidence: EvidenceView }): JSX.Element | null {
  if (evidence.invalidFacts.length === 0) return null;
  return (
    <div className="inline-alert inline-alert--danger" role="alert">
      <strong>Fact consistency warning</strong>
      {evidence.invalidFacts.map((issue) => (
        <span key={issue.reference}>
          {issue.reference}: {issue.reason}
        </span>
      ))}
    </div>
  );
}

/** Render the supplied fact index. */
function FactsTab({
  evidence,
  selected,
  onSelect,
}: {
  readonly evidence: EvidenceView;
  readonly selected: string | null;
  readonly onSelect: (reference: string) => void;
}): JSX.Element {
  const focused = evidence.facts.find((fact) => fact.reference === selected);
  const ordered = focused ? [focused, ...evidence.facts.filter((fact) => fact.reference !== selected)] : evidence.facts;
  return (
    <div className="detail-stack">
      <p className="fact-policy">
        Each row below is a supplied JSON Pointer fact. Clickable citations resolve only to these exact references. Integrity cross-checks are local, not an
        authenticated signature.
      </p>
      <FactWarning evidence={evidence} />
      <div className="fact-list">
        {ordered.map((fact) => (
          <FactRow key={fact.reference} fact={fact} selected={fact.reference === selected} onSelect={onSelect} />
        ))}
      </div>
    </div>
  );
}

/** Render named identities and replay/completeness metadata. */
function IdentityDetails({ evidence }: { readonly evidence: EvidenceView }): JSX.Element {
  const instance = evidence.instance;
  const fields = [
    ["Instance", instance.instanceId, true],
    ["Model revision", instance.modelRevisionSha256, true],
    ["Semantic", instance.semanticSha256, true],
    ["Alloy relations", instance.alloyRelationsSha256, true],
    ["Validation", instance.validation, false],
    ["Completeness", stringifyJson(instance.completeness), false],
  ] as const;
  return (
    <section>
      <span className="mono-label">IDENTITIES & REPLAY</span>
      <dl className="detail-grid">
        {fields.map(([label, value, code]) => (
          <div key={label}>
            <dt>{label}</dt>
            <dd>{code ? <code>{value}</code> : value}</dd>
          </div>
        ))}
      </dl>
    </section>
  );
}

/** Render assurance distinctions, assumptions and provenance. */
function AssuranceTab({ evidence }: { readonly evidence: EvidenceView }): JSX.Element {
  const assurance = describeAssurance(evidence.instance.assuranceKind);
  return (
    <div className="detail-stack">
      <section>
        <span className="mono-label">ASSURANCE DISTINCTION</span>
        <h3>{assurance.label}</h3>
        <p>{assurance.plain}</p>
        <p className="detail-warning">{assurance.caveat}</p>
      </section>
      <IdentityDetails evidence={evidence} />
      <section>
        <span className="mono-label">ASSUMPTIONS</span>
        <pre className="code-block">{stringifyJson(evidence.bundle.assumptions, 2)}</pre>
      </section>
      <section>
        <span className="mono-label">PROVENANCE</span>
        <pre className="code-block">{stringifyJson(evidence.bundle.provenance, 2)}</pre>
      </section>
      <p className="details-footnote">Corpus totals are not inferred from this bundle. If corpus evidence is not supplied, no corpus metric is shown.</p>
    </div>
  );
}

/** Render a raw payload disclosure. */
function RawPayload({ title, value }: { readonly title: string; readonly value: unknown }): JSX.Element {
  return (
    <details>
      <summary>{title}</summary>
      <pre className="code-block code-block--tall">{stringifyJson(value, 2)}</pre>
    </details>
  );
}

/** Render original payloads and downloads. */
function JsonTab({ evidence, openRaw, nextRaw }: { readonly evidence: EvidenceView; readonly openRaw?: unknown; readonly nextRaw?: unknown }): JSX.Element {
  return (
    <div className="detail-stack">
      <div className="inline-alert">
        <strong>Local inspection only</strong>
        <span>
          Imported or fixture JSON may be inspected locally; its integrity and replay are not authenticated by this frontend. A partial graph never edits this
          payload.
        </span>
      </div>
      <p>
        Current instance <code>{shortHash(evidence.instance.instanceId)}</code>
      </p>
      <pre className="code-block code-block--tall">{stringifyJson(evidence.instance.raw, 2)}</pre>
      <RawPayload title="Latest open response payload" value={openRaw} />
      {nextRaw ? <RawPayload title="Latest next response payload" value={nextRaw} /> : null}
      <JsonTransfer openRaw={openRaw} nextRaw={nextRaw} />
    </div>
  );
}

/** Render JSON download actions. */
function JsonTransfer({ openRaw, nextRaw }: { readonly openRaw?: unknown; readonly nextRaw?: unknown }): JSX.Element {
  return (
    <div className="download-row">
      <button type="button" className="text-button" onClick={() => downloadJson("sqlolla-open-response.json", openRaw)}>
        Download open response
      </button>
      {nextRaw ? (
        <button type="button" className="text-button" onClick={() => downloadJson("sqlolla-next-response.json", nextRaw)}>
          Download next response
        </button>
      ) : null}
    </div>
  );
}

/** Render the active inspector tab. */
function ActiveTab({
  tab,
  evidence,
  selectedFact,
  onSelect,
  openRaw,
  nextRaw,
}: {
  readonly tab: DetailsTab;
  readonly evidence: EvidenceView;
  readonly selectedFact: string | null;
  readonly onSelect: (reference: string) => void;
  readonly openRaw?: unknown;
  readonly nextRaw?: unknown;
}): JSX.Element {
  if (tab === "ir") return <CodeTab title="Stored typed IR" text={stringifyJson(evidence.bundle.ir, 2)} />;
  if (tab === "alloy") return <CodeTab title="Stored Alloy source" text={evidence.bundle.model} />;
  if (tab === "facts") return <FactsTab evidence={evidence} selected={selectedFact} onSelect={onSelect} />;
  if (tab === "assurance") return <AssuranceTab evidence={evidence} />;
  if (tab === "json") return <JsonTab evidence={evidence} openRaw={openRaw} nextRaw={nextRaw} />;
  return <SqlTab evidence={evidence} />;
}

/** Render instance and bundle downloads. */
function EvidenceDownloads({ evidence }: { readonly evidence: EvidenceView }): JSX.Element {
  return (
    <div className="download-row">
      <button type="button" className="text-button" onClick={() => downloadJson("sqlolla-instance.json", evidence.instance.raw)}>
        Download instance JSON
      </button>
      <button type="button" className="text-button" onClick={() => downloadJson("sqlolla-bundle.json", evidence.bundle.raw)}>
        Download bundle JSON
      </button>
    </div>
  );
}

/** Render the active tab and shared footer. */
function DetailsBody({
  evidence,
  initialTab = "sql",
  focusFact,
  openRaw,
  nextRaw,
  native,
}: DetailsDrawerProps & { readonly evidence: EvidenceView }): JSX.Element {
  const [tab, setTab] = useState<DetailsTab>(initialTab);
  const [selectedFact, setSelectedFact] = useState<string | null>(focusFact ?? null);
  return (
    <div className="details-body">
      <FixtureBanner native={native} />
      <DetailTabs tab={tab} setTab={setTab} />
      <ActiveTab tab={tab} evidence={evidence} selectedFact={selectedFact} onSelect={setSelectedFact} openRaw={openRaw} nextRaw={nextRaw} />
      <p className="details-footnote">All SQL, identifiers, values and AI-style content are untrusted text. The UI never executes SQL or imported DOT.</p>
      <EvidenceDownloads evidence={evidence} />
    </div>
  );
}

/** Render the complete drawer. */
export function DetailsDrawer({ open, onClose, evidence, ...props }: DetailsDrawerProps): JSX.Element {
  return (
    <Modal open={open} onClose={onClose} title="Evidence inspector" eyebrow="PROGRESSIVE DISCLOSURE" className="details-drawer">
      {open && evidence ? <DetailsBody {...props} open={open} onClose={onClose} evidence={evidence} /> : <p className="empty-note">No evidence is selected.</p>}
    </Modal>
  );
}
