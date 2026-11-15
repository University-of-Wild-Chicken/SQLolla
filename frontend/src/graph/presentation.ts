/**
 * Deterministic presentation-graph derivation from validated evidence. It shapes
 * copy and graph layout only; it never interprets SQL execution or renders
 * imported DOT. All labels are escaped plain text and no external assets exist.
 */
import { cellText } from "../analysis/format";
import type { CellModel, ColumnModel, EffectObservation, EvidenceView, OccurrenceView, RelationView } from "../analysis/types";

/** Default number of visible presentation nodes before a partial-view warning. */
export const DEFAULT_GRAPH_NODE_LIMIT = 18;

/** One inspectable presentation node with an optional supplied fact pointer. */
export interface PresentationNode {
  readonly id: string;
  readonly groupId: string;
  readonly kind: "before" | "output" | "after" | "error" | "effect" | "query";
  readonly labelLines: readonly string[];
  readonly factReference: string | null;
  readonly heading: string;
}

/** One visual cluster of related nodes. */
export interface PresentationGroup {
  readonly id: string;
  readonly label: string;
  readonly kind: "before" | "output" | "after" | "effects";
  readonly nodes: readonly PresentationNode[];
}

/** An edge is structural membership, not inferred row-level lineage. */
export interface PresentationEdge {
  readonly from: string;
  readonly to: string;
  readonly label: string;
}

/** Complete graph metadata, visible subset and derived DOT. */
export interface PresentationGraph {
  readonly nodes: readonly PresentationNode[];
  readonly groups: readonly PresentationGroup[];
  readonly edges: readonly PresentationEdge[];
  readonly dot: string;
  readonly totalNodeCount: number;
  readonly visibleNodeCount: number;
  readonly omittedNodeCount: number;
  readonly viewComplete: boolean;
  readonly limitations: readonly string[];
}

const MAX_LINE = 44;

/** Collapse untrusted text to one safe presentation line and report clipping. */
function clipLine(text: string): { text: string; clipped: boolean } {
  const flat = text.replace(/\s+/g, " ").trim();
  return flat.length > MAX_LINE ? { text: `${flat.slice(0, MAX_LINE - 1)}…`, clipped: true } : { text: flat, clipped: false };
}

/** Format up to three cell lines, reporting label truncation for the view. */
function cellLines(columns: readonly ColumnModel[], cells: readonly CellModel[], changed: readonly number[]): { lines: string[]; clipped: boolean } {
  const limited = columns.slice(0, 3);
  const lines = limited.map((column, index) => {
    const clipped = clipLine(`${changed.includes(index) ? "Δ " : ""}${column.name}: ${cells[index] ? cellText(cells[index]) : "—"}`);
    return clipped;
  });
  const extra = columns.length > 3 ? `+${columns.length - 3} more column${columns.length - 3 === 1 ? "" : "s"}` : null;
  return {
    lines: [...lines.map((line) => line.text), ...(extra ? [extra] : [])],
    clipped: lines.some((line) => line.clipped),
  };
}

/** Build node labels for one relation occurrence while tracking presentation loss. */
function occurrenceNode(
  id: string,
  groupId: string,
  kind: "before" | "output" | "after",
  heading: string,
  columns: readonly ColumnModel[],
  occurrence: OccurrenceView,
  clipped: { value: boolean },
): PresentationNode {
  const cells = cellLines(columns, occurrence.cells, occurrence.changedColumns);
  if (cells.clipped) clipped.value = true;
  return {
    id,
    groupId,
    kind,
    heading,
    labelLines: [`${heading} · #${occurrence.position}`, ...cells.lines],
    factReference: occurrence.factReference,
  };
}

/** Build one cluster for all occurrences of a before or after table. */
function tableGroup(table: RelationView, kind: "before" | "after", tableIndex: number, clipped: { value: boolean }): PresentationGroup {
  const prefix = kind === "before" ? "i" : "a";
  const groupId = `${kind}:${table.tableId}`;
  const nodes = table.rows.map((row, index) => occurrenceNode(`${prefix}${tableIndex}-${index}`, groupId, kind, table.tableName, table.columns, row, clipped));
  return {
    id: groupId,
    label: `${kind.toUpperCase()} · ${table.tableId}`,
    kind,
    nodes,
  };
}

/** Build all output occurrence nodes in one cluster. */
function outputGroup(evidence: EvidenceView, clipped: { value: boolean }): PresentationGroup {
  const nodes = evidence.output.rows.map((row, index) => occurrenceNode("o" + index, "output", "output", "RETURNED", evidence.output.columns, row, clipped));
  return { id: "output", label: "OUTPUT · supplied occurrences", kind: "output", nodes };
}

/** Build all before/output/after row nodes before applying the view cap. */
function rowNodes(evidence: EvidenceView): {
  nodes: PresentationNode[];
  groups: PresentationGroup[];
  clipped: boolean;
} {
  const clipped = { value: false };
  const groups = [
    ...evidence.inputTables.map((table, index) => tableGroup(table, "before", index, clipped)),
    outputGroup(evidence, clipped),
    ...evidence.statement.poststateTables.map((table, index) => tableGroup(table, "after", index, clipped)),
  ];
  return { nodes: groups.flatMap((group) => group.nodes), groups, clipped: clipped.value };
}

/** Build the observed SQL error node when an error diagnostic exists. */
function errorGroup(evidence: EvidenceView): PresentationGroup | null {
  const error = evidence.statement.error;
  if (!error) return null;
  const node: PresentationNode = {
    id: "error",
    groupId: "errors",
    kind: "error",
    heading: "SQL error observation",
    labelLines: [
      "SQL error observed",
      `SQLSTATE ${error.sqlstate} · ${error.phase}`,
      error.constraintName ? `constraint: ${error.constraintName}` : "no constraint name supplied",
    ],
    factReference: null,
  };
  return { id: "errors", label: "OBSERVED ERROR", kind: "output", nodes: [node] };
}

/** Build retained effect nodes when effects are supplied. */
function effectsGroup(evidence: EvidenceView): PresentationGroup | null {
  const effectNodes = evidence.statement.effects.map((effect, index) => effectNode(effect, index));
  return effectNodes.length > 0 ? { id: "effects", label: "RETAINED EFFECTS", kind: "effects", nodes: effectNodes } : null;
}

/** Build error and effect nodes as observations, not inferred SQL semantics. */
function specialNodes(evidence: EvidenceView): {
  nodes: PresentationNode[];
  groups: PresentationGroup[];
} {
  const groups = [errorGroup(evidence), effectsGroup(evidence)].filter((group): group is PresentationGroup => group !== null);
  return { nodes: groups.flatMap((group) => group.nodes), groups };
}

/** Build one effect node from exact supplied fields. */
function effectNode(effect: EffectObservation, index: number): PresentationNode {
  const fields = Object.entries(effect.fields)
    .slice(0, 2)
    .map(([key, value]) => clipLine(`${key}: ${value}`));
  return {
    id: `e${index}`,
    groupId: "effects",
    kind: "effect",
    heading: effect.label,
    labelLines: [effect.label, ...fields.map((field) => field.text)],
    factReference: effect.reference,
  };
}

/** Build the central descriptive node; it makes no row-level causal claim. */
function queryNode(evidence: EvidenceView): PresentationNode {
  const inputs = evidence.inputTables.reduce((sum, table) => sum + table.rows.length, 0);
  return {
    id: "query",
    groupId: "query",
    kind: "query",
    heading: "Review focus",
    labelLines: [
      "REVIEW FOCUS",
      evidence.instance.kind === "counterexample" ? "counterexample" : "stored example",
      `${inputs} before → ${evidence.output.rows.length} returned`,
    ],
    factReference: null,
  };
}

/** Rank nodes so the review focus, errors/effects and one node per group survive the cap. */
function selectVisible(all: readonly PresentationNode[], limit: number): Set<string> {
  const selected = new Set<string>();
  const essentials = all.filter((node) => ["query", "error", "effect"].includes(node.kind));
  for (const node of essentials) {
    if (selected.size < limit) selected.add(node.id);
  }
  const groupOrder = [...new Set(all.map((node) => node.groupId))];
  for (const groupId of groupOrder) {
    const first = all.find((node) => node.groupId === groupId);
    if (first && selected.size < limit) selected.add(first.id);
  }
  for (const node of all) {
    if (selected.size < limit) selected.add(node.id);
  }
  return selected;
}

/** Escape plain label text for a quoted DOT string without HTML labels. */
function dotLine(text: string): string {
  return text.replaceAll("\\", "⧵").replaceAll('"', '\\"').replaceAll("\r", " ");
}

/** Quote a DOT string value built from intentional escaped line breaks. */
function dotQuote(text: string): string {
  return `"${text.replaceAll("\\", "⧵").replaceAll('"', '\\"')}"`;
}

/** Render one node declaration using only fixed, non-external attributes. */
function dotNode(node: PresentationNode): string {
  const fills: Record<PresentationNode["kind"], [string, string, string]> = {
    before: ["#f4eee3", "#b9a98e", "#1c2d33"],
    output: ["#dcefeb", "#087b80", "#163c3d"],
    after: ["#f7e9d8", "#9b641f", "#3a2b17"],
    error: ["#f3d8d2", "#a73c32", "#4b1f1a"],
    effect: ["#e6e9f0", "#43546b", "#1f2a39"],
    query: ["#087b80", "#055d61", "#f8fbf7"],
  };
  const [fill, stroke, font] = fills[node.kind];
  const label = node.labelLines.map(dotLine).join("\\n");
  return `  ${dotQuote(node.id)} [label="${label}", fillcolor=${dotQuote(fill)}, color=${dotQuote(stroke)}, fontcolor=${dotQuote(font)}];`;
}

/** Render fixed DOT graph/node/edge attributes plus the central focus node. */
function dotHeader(evidence: EvidenceView): readonly string[] {
  const inputs = evidence.inputTables.reduce((sum, table) => sum + table.rows.length, 0);
  return [
    "digraph sqlolla_review {",
    '  graph [rankdir="LR", bgcolor="transparent", pad="0.18", nodesep="0.32", ranksep="0.62", fontname="IBM Plex Mono"];',
    '  node [shape="box", style="rounded,filled", fontname="Space Grotesk", fontsize="11", margin="0.16,0.10"];',
    '  edge [color="#8ab9b5", fontname="IBM Plex Mono", fontsize="9", fontcolor="#bcded8", arrowsize="0.6"];',
    `  ${dotQuote("query")} [label="REVIEW FOCUS\\n${dotLine(evidence.instance.kind)}\\n${inputs} before → ${evidence.output.rows.length} returned", fillcolor="#087b80", color="#055d61", fontcolor="#f8fbf7"];`,
  ];
}

/** Render one visible cluster of nodes. */
function dotGroup(group: PresentationGroup, visible: ReadonlySet<string>): readonly string[] {
  const nodes = group.nodes.filter((node) => visible.has(node.id));
  if (nodes.length === 0) return [];
  return [
    `  subgraph ${dotQuote(`cluster_${group.id.replace(/[^a-z0-9]/gi, "_")}`)} {`,
    `    label=${dotQuote(group.label)}; style="rounded,dashed"; color="#557c7b"; fontcolor="#bcded8"; fontname="IBM Plex Mono"; fontsize="9";`,
    ...nodes.map(dotNode),
    "  }",
  ];
}

/** Render visible structural edges. */
function dotEdges(edges: readonly PresentationEdge[], visible: ReadonlySet<string>): readonly string[] {
  return edges
    .filter((edge) => visible.has(edge.from) && visible.has(edge.to))
    .map((edge) => `  ${dotQuote(edge.from)} -> ${dotQuote(edge.to)} [label=${dotQuote(edge.label)}, style="dashed"];`);
}

/** Render the complete visible DOT graph with clusters and neutral structural edges. */
function buildDot(
  groups: readonly PresentationGroup[],
  nodes: readonly PresentationNode[],
  edges: readonly PresentationEdge[],
  evidence: EvidenceView,
): string {
  const visible = new Set(nodes.map((node) => node.id));
  return [...dotHeader(evidence), ...groups.flatMap((group) => dotGroup(group, visible)), ...dotEdges(edges, visible), "}"].join("\n");
}

/** Detect a literal backslash in string values, not a JSON quote escape. */
function containsStringBackslash(value: unknown): boolean {
  if (typeof value === "string") return value.includes("\\");
  if (Array.isArray(value)) return value.some(containsStringBackslash);
  if (typeof value === "object" && value !== null) {
    return Object.values(value as Record<string, unknown>).some(containsStringBackslash);
  }
  return false;
}

/** Keep only clusters that contain at least one visible node. */
function visibleGroups(groups: readonly PresentationGroup[], selected: ReadonlySet<string>): readonly PresentationGroup[] {
  return groups.map((group) => ({ ...group, nodes: group.nodes.filter((node) => selected.has(node.id)) })).filter((group) => group.nodes.length > 0);
}

/** Describe every deliberate presentation loss without inheriting complete evidence. */
function graphLimitations(allCount: number, visibleCount: number, rowsClipped: boolean, rawHasBackslash: boolean): readonly string[] {
  const limitations: string[] = [];
  if (allCount > visibleCount) {
    limitations.push(`Graph shows ${visibleCount} of ${allCount} nodes; ${allCount - visibleCount} are omitted from this view only.`);
  }
  if (rowsClipped) {
    limitations.push("Some graph labels are shortened; Table view and original JSON retain full values.");
  }
  if (rawHasBackslash) {
    limitations.push("Graph labels display a backslash as ⧵; exact text remains in Table view.");
  }
  return limitations;
}

/** Build a deterministic presentation graph from validated evidence. */
export function buildPresentationGraph(evidence: EvidenceView, limit: number = DEFAULT_GRAPH_NODE_LIMIT): PresentationGraph {
  const rows = rowNodes(evidence);
  const specials = specialNodes(evidence);
  const allNodes = [queryNode(evidence), ...rows.nodes, ...specials.nodes];
  const allGroups = [...rows.groups, ...specials.groups];
  const selected = selectVisible(allNodes, Math.max(1, Math.floor(limit)));
  const nodes = allNodes.filter((node) => selected.has(node.id));
  const groups = visibleGroups(allGroups, selected);
  const edges = buildEdges(nodes);
  const omittedNodeCount = allNodes.length - nodes.length;
  const rawHasBackslash = evidence.facts.some((fact) => containsStringBackslash(fact.value));
  return {
    nodes,
    groups,
    edges,
    dot: buildDot(groups, nodes, edges, evidence),
    totalNodeCount: allNodes.length,
    visibleNodeCount: nodes.length,
    omittedNodeCount,
    viewComplete: omittedNodeCount === 0 && !rows.clipped && !rawHasBackslash,
    limitations: graphLimitations(allNodes.length, nodes.length, rows.clipped, rawHasBackslash),
  };
}

/** Build only edges whose endpoints can be visible; no row provenance is inferred. */
function buildEdges(nodes: readonly PresentationNode[]): readonly PresentationEdge[] {
  const result: PresentationEdge[] = [];
  for (const node of nodes) {
    if (node.kind === "before") result.push({ from: node.id, to: "query", label: "before" });
    if (node.kind === "output") result.push({ from: "query", to: node.id, label: "returned" });
    if (node.kind === "after") result.push({ from: "query", to: node.id, label: "after" });
    if (node.kind === "error") result.push({ from: "query", to: node.id, label: "rejected" });
    if (node.kind === "effect") result.push({ from: "query", to: node.id, label: "retained" });
  }
  return result;
}
