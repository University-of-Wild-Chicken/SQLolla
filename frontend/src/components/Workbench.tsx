import type { JSX, ReactNode } from "react";
import markUrl from "../assets/sqlolla-mark.png";
import { LiveAssistancePanel } from "../ai/LiveAssistancePanel";
import { useHostConnection } from "../workspace/hostConnection";
import type { Workspace } from "../workspace/useWorkspace";
import { JournalPanel } from "./JournalPanel";
import { QueryWorkbench } from "./QueryWorkbench";

type WorkbenchProps = { readonly workspace: Workspace; readonly results: ReactNode; readonly reviews: ReactNode };

/** Give page navigation and provider settings persistent, accessible controls. */
function WorkbenchHeader({ workspace }: Pick<WorkbenchProps, "workspace">): JSX.Element {
  return (
    <header className="workbench-header">
      <a className="workbench-brand" href="#review-workspace" aria-label="SQLolla workbench">
        <img src={markUrl} width="28" height="31" alt="" />
        SQLolla
      </a>
      <nav aria-label="Workspace">
        <a href="#query-editor">Query</a>
        <a href="#review-bins">Saved cases</a>
        <button type="button" className="ghost-button" onClick={workspace.dialogs.openSettings}>
          AI settings {workspace.dialogs.aiSettings.apiKey ? <span className="key-dot" aria-label="Key saved in this tab" /> : null}
        </button>
      </nav>
    </header>
  );
}

/** Report browser-to-host capability separately from solver and OpenAI readiness. */
function HostConnection(): JSX.Element {
  const { connection, retry } = useHostConnection();
  if (connection.state === "unavailable") {
    return (
      <div className="workbench-connection workbench-connection--error">
        <span role="status">{connection.message}</span>
        <button type="button" className="text-button" onClick={retry}>
          Retry connection
        </button>
      </div>
    );
  }
  return (
    <p className="workbench-connection" role="status">
      {connection.state === "ready" ? "Analysis server reachable" : "Checking analysis server…"}
    </p>
  );
}

/** Offer AI setup on the first visit while keeping query analysis available. */
function AiSetupPrompt({ workspace }: Pick<WorkbenchProps, "workspace">): JSX.Element | null {
  const { showAiSetup, openSettings, dismissAiSetup } = workspace.dialogs;
  if (!showAiSetup) return null;
  return (
    <section className="workbench-panel workbench-ai-setup" aria-labelledby="ai-setup-heading">
      <div>
        <h2 id="ai-setup-heading">Set up AI assistance</h2>
        <p>Add your OpenAI key to draft a model or query and explain solver cases. You can also build and analyze queries yourself.</p>
      </div>
      <div className="workbench-ai-setup-actions">
        <button type="button" className="primary-button" onClick={openSettings}>
          Set up AI
        </button>
        <button type="button" className="ghost-button" onClick={dismissAiSetup}>
          Skip for now
        </button>
      </div>
    </section>
  );
}

/** Offer live explanation only for a full, canonical solver bundle. */
function WorkbenchAssistance({ workspace }: Pick<WorkbenchProps, "workspace">): JSX.Element | null {
  const { currentEvidence: evidence, dialogs, archive, session } = workspace;
  const bundleId = archive.visible?.result?.bundleId ?? session.state.bundleId;
  if (!bundleId || !evidence?.bundle.model.trim() || !evidence.bundle.ir) return null;
  return (
    <section className="workbench-panel workbench-assistance" aria-label="AI explanation">
      <h2>Explain this result</h2>
      {dialogs.aiSettings.apiKey ? (
        <LiveAssistancePanel bundleId={bundleId} evidence={evidence} settings={dialogs.aiSettings} onInspectFact={dialogs.inspectFact} />
      ) : (
        <>
          <p>Ask why this result occurred or what to check next.</p>
          <button type="button" className="primary-button" onClick={dialogs.openSettings}>
            Add an OpenAI key
          </button>
        </>
      )}
    </section>
  );
}

/** Keep journal export and conflict review one click away from the three saved-case bins. */
function WorkbenchJournal({ workspace }: Pick<WorkbenchProps, "workspace">): JSX.Element {
  return (
    <details id="review-ledger" className="workbench-panel workbench-journal">
      <summary>Review history and export</summary>
      <JournalPanel
        journal={workspace.journal}
        currentRevision={workspace.currentEvidence?.instance.modelRevisionSha256 ?? null}
        onWithdraw={workspace.journal.withdraw}
      />
    </details>
  );
}

/** Lead with the query and result, then group review and revision on the same page. */
export function Workbench({ workspace, results, reviews }: WorkbenchProps): JSX.Element {
  return (
    <div className="workbench-shell">
      <WorkbenchHeader workspace={workspace} />
      <main className="workbench" id="review-workspace">
        <div className="workbench-title">
          <div>
            <h1>Query workbench</h1>
            <p>Test a SQL query against a rule, then review what Alloy finds.</p>
          </div>
          <HostConnection />
        </div>
        <AiSetupPrompt workspace={workspace} />
        <div className="workbench-grid">
          <div className="workbench-side">
            <QueryWorkbench workspace={workspace} />
            <WorkbenchAssistance workspace={workspace} />
          </div>
          <div className="workbench-results" id="review-center">
            {results}
          </div>
        </div>
        {reviews}
        <WorkbenchJournal workspace={workspace} />
      </main>
    </div>
  );
}
