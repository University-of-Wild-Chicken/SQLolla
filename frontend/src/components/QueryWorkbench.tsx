/** Production editor entry points share one explicit database model. */
import { useRef, useState, type JSX } from "react";
import type { SqlDraft } from "../ai/draft";
import { parseCatalog } from "../analysis/adapter";
import type { CatalogModel } from "../analysis/types";
import { currentCatalog } from "../workspace/schema";
import type { Workspace } from "../workspace/useWorkspace";
import { AiDraftPanel } from "./AiDraftPanel";
import { BuilderPanel } from "./BuilderPanel";
import { NativeAnalysisPanel } from "./NativeAnalysisPanel";
import { SchemaPanel } from "./SchemaPanel";

type Tab = "analysis" | "builder" | "ai";

/** Switch modes without sending a query or provider request. */
function useQueryTabs(workspace: Workspace) {
  const [ai, setAi] = useState(false);
  const { navigation } = workspace;
  const catalog = currentCatalog(workspace.native.text);
  const tab: Tab = ai ? "ai" : navigation.leftTab === "builder" ? "builder" : "analysis";
  const select = (next: Tab): void => {
    setAi(next === "ai");
    navigation.setLeftTab(next === "builder" ? "builder" : "analysis");
    if (next === "builder" && !navigation.draft) workspace.startDraft();
  };
  return { tab, select, catalog };
}

/** Extend an active builder only when the declaration adds one table to the current catalog. */
function useContextEdits(workspace: Workspace, active: Tab) {
  const { navigation, native } = workspace;
  const change = (request: Record<string, unknown>): void => {
    const catalog = parseCatalog(request.catalog, "request catalog");
    native.stage(request);
    if (navigation.draft) navigation.dispatchDraft({ type: "extend-catalog", catalog });
    else if (active === "builder") navigation.dispatchDraft({ type: "start", catalog, starterDefaults: false });
  };
  const edited = (): void => navigation.dispatchDraft({ type: "reset" });
  return { change, edited };
}

/** Let users choose a direct query, relational cards or a first AI draft. */
export function QueryWorkbench({ workspace }: { readonly workspace: Workspace }): JSX.Element {
  const { tab, select, catalog } = useQueryTabs(workspace);
  const edits = useContextEdits(workspace, tab);
  const used = useRef(new Set<string>());
  const stage = (draft: SqlDraft, prompt: string): void => {
    workspace.archive.close();
    workspace.navigation.dispatchDraft({ type: "reset" });
    if (draft.modelRequest) workspace.native.stage(draft.modelRequest);
    else workspace.native.editSql(draft.sql);
    workspace.refinement.setInstruction(prompt);
    select("analysis");
  };
  return (
    <section className="workbench-panel query-workbench" id="query-editor" aria-label="Query editor">
      <QueryTabs tab={tab} select={select} />
      <SchemaPanel analysis={workspace.native} catalog={catalog} {...edits} />
      {tab === "ai" ? (
        <AiEditor workspace={workspace} catalog={catalog} used={used.current} stage={stage} />
      ) : tab === "builder" ? (
        <QueryBuilder workspace={workspace} catalog={catalog} />
      ) : (
        <NativeAnalysisPanel analysis={workspace.native} compact />
      )}
    </section>
  );
}

/** Give the three creation paths equal visibility from the blank workbench. */
function QueryTabs({ tab, select }: { readonly tab: Tab; readonly select: (tab: Tab) => void }): JSX.Element {
  return (
    <div className="workbench-tabs" aria-label="Query editor mode">
      <button type="button" aria-pressed={tab === "analysis"} onClick={() => select("analysis")}>
        SQL editor
      </button>
      <button type="button" aria-pressed={tab === "builder"} onClick={() => select("builder")}>
        Query builder
      </button>
      <button type="button" aria-pressed={tab === "ai"} onClick={() => select("ai")}>
        Build with AI
      </button>
    </div>
  );
}

/** Transfer a typed builder intent without sending anything until the user submits. */
function AiEditor({
  workspace,
  catalog,
  used,
  stage,
}: {
  readonly workspace: Workspace;
  readonly catalog: CatalogModel | null;
  readonly used: Set<string>;
  readonly stage: (draft: SqlDraft, prompt: string) => void;
}): JSX.Element {
  return (
    <AiDraftPanel
      requestText={workspace.native.text}
      validCatalog={!!catalog && Object.keys(catalog.tables).length > 0}
      settings={workspace.dialogs.aiSettings}
      initialPrompt={workspace.navigation.draft?.intent || ""}
      used={used}
      markProposed={(prompt) => used.add(prompt)}
      openSettings={workspace.dialogs.openSettings}
      stage={stage}
    />
  );
}

/** Pass the parsed current editor catalog to both builder display and deterministic checking. */
function QueryBuilder({ workspace, catalog }: { readonly workspace: Workspace; readonly catalog: CatalogModel | null }): JSX.Element {
  const { navigation } = workspace;
  const schema = catalog && Object.keys(catalog.tables).length ? catalog : null;
  const scenario = { ...navigation.selectedScenario, builderCatalog: schema };
  return (
    <BuilderPanel
      draft={navigation.draft}
      scenarios={[scenario]}
      selectedId={navigation.selectedId}
      onStartDraft={workspace.startDraft}
      onDiscardDraft={workspace.discardDraft}
      dispatchDraft={navigation.dispatchDraft}
      onCheckDraft={workspace.checkDraft}
      onStageDraft={workspace.stageDraft}
    />
  );
}
