/**
 * Production workbench shell: an application title and menu bar, a purpose banner, one window
 * per task area and a status bar that reports server and AI state without sending anything.
 */
import type { JSX, ReactNode } from "react";
import markUrl from "../assets/sqlolla-mark.png";
import wordmarkUrl from "../assets/sqlolla-wordmark.png";
import { LiveAssistancePanel } from "../ai/LiveAssistancePanel";
import { useHostConnection } from "../workspace/hostConnection";
import type { Workspace } from "../workspace/useWorkspace";
import { AiIntro, AiTargetNote, aiStatusText } from "./AiAssistant";
import { JournalPanel } from "./JournalPanel";
import { QueryWorkbench } from "./QueryWorkbench";
import { Window } from "./Window";

type WorkbenchProps = { readonly workspace: Workspace; readonly results: ReactNode; readonly reviews: ReactNode };
type Connection = ReturnType<typeof useHostConnection>;

/** Application title bar plus a menu bar of page links and the AI settings command. */
function WorkbenchHeader({ workspace }: Pick<WorkbenchProps, "workspace">): JSX.Element {
  return (
    <header className="workbench-header">
      <div className="app-titlebar">
        <a className="workbench-brand" href="#review-workspace" aria-label="SQLolla workbench">
          <img src={markUrl} width="16" height="21" alt="" />
          SQLolla — Query Workbench
        </a>
        <span className="app-titlebar__controls" aria-hidden="true">
          <span>_</span>
          <span>□</span>
          <span>×</span>
        </span>
      </div>
      <nav className="menubar" aria-label="Workspace">
        <a href="#query-editor">Query</a>
        <a href="#review-center">Results</a>
        <a href="#review-bins">Saved cases</a>
        <a className="workbench-docs-link" href="/docs/" target="_blank" rel="noopener">
          Docs<span className="sr-only"> (opens in a new tab)</span>
        </a>
        <button type="button" className="menubar__button" onClick={workspace.dialogs.openSettings}>
          AI settings {workspace.dialogs.aiSettings.apiKey ? <span className="key-dot" aria-label="Key saved in this tab" /> : null}
        </button>
      </nav>
    </header>
  );
}

/** The four-step loop, so a first-time user knows what the page is for. */
const STEPS = [
  ["Build", "Write SQL, use the query builder, or draft it with AI."],
  ["Check", "Analyze it against your rule inside a small, declared model of your tables."],
  ["Review", "Mark each example or counterexample Yes, No or Unsure."],
  ["Refine", "Fix the query; every labeled case is checked again."],
] as const;

/** Purpose banner with the reused logo, plain-language summary and server reachability. */
function WorkbenchHero({ connection }: { readonly connection: Connection }): JSX.Element {
  return (
    <section className="hero" aria-labelledby="workbench-heading">
      <div className="hero__brand">
        <img className="hero__mark" src={markUrl} width="64" height="84" alt="" />
        <img className="hero__wordmark" src={wordmarkUrl} width="320" height="70" alt="SQLolla" />
        <p className="hero__tagline">Bounded checks for PostgreSQL queries</p>
      </div>
      <div className="hero__copy">
        <div className="hero__heading">
          <h1 id="workbench-heading">Query workbench</h1>
          <HostConnection connection={connection} />
        </div>
        <p>
          See what a SQL query can really do before it reaches your data. Declare a few tables and a rule; SQLolla’s solver searches every case within those
          limits for examples that follow the rule and counterexamples that break it. No database is connected or changed.
        </p>
        <ol className="hero__steps">
          {STEPS.map(([name, text], index) => (
            <li key={name}>
              <span className="hero__step-index">{index + 1}</span>
              <strong>{name}</strong> <span>{text}</span>
            </li>
          ))}
        </ol>
      </div>
    </section>
  );
}

/** Report browser-to-host capability separately from solver and provider readiness. */
function HostConnection({ connection: { connection, retry } }: { readonly connection: Connection }): JSX.Element {
  if (connection.state === "unavailable") {
    return (
      <div className="workbench-connection workbench-connection--error">
        <span role="status">{connection.message}</span>
        <button type="button" className="ghost-button" onClick={retry}>
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
function AiSetup({ workspace }: Pick<WorkbenchProps, "workspace">): JSX.Element | null {
  const { showAiSetup, openSettings, dismissAiSetup } = workspace.dialogs;
  return showAiSetup ? <AiIntro onConnect={openSettings} onSkip={dismissAiSetup} /> : null;
}

/** Offer live explanation only for a full, canonical solver bundle. */
function WorkbenchAssistance({ workspace }: Pick<WorkbenchProps, "workspace">): JSX.Element | null {
  const { currentEvidence: evidence, dialogs, archive, session } = workspace;
  const bundleId = archive.visible?.result?.bundleId ?? session.state.bundleId;
  if (!bundleId || !evidence?.bundle.model.trim() || !evidence.bundle.ir) return null;
  return (
    <Window title="Ask AI about this result" className="workbench-assistance">
      <p className="ai-purpose">
        Ask why this case happened or what to check next. Each AI claim links to the evidence fact it cites; the result itself does not change.
      </p>
      {dialogs.aiSettings.apiKey ? (
        <>
          <AiTargetNote settings={dialogs.aiSettings} />
          <LiveAssistancePanel bundleId={bundleId} evidence={evidence} settings={dialogs.aiSettings} onInspectFact={dialogs.inspectFact} />
        </>
      ) : (
        <button type="button" className="primary-button" onClick={dialogs.openSettings}>
          Connect AI provider…
        </button>
      )}
    </Window>
  );
}

/** Keep journal export and conflict review one click away from the three saved-case bins. */
function WorkbenchJournal({ workspace }: Pick<WorkbenchProps, "workspace">): JSX.Element {
  return (
    <details id="review-ledger" className="win workbench-journal">
      <summary className="win__title">Review history and export</summary>
      <div className="win__body">
        <JournalPanel
          journal={workspace.journal}
          currentRevision={workspace.currentEvidence?.instance.modelRevisionSha256 ?? null}
          onWithdraw={workspace.journal.withdraw}
        />
      </div>
    </details>
  );
}

/** Status bar: server reachability, AI provider and model, and the analysis boundary. */
function StatusBar({ workspace, connection }: Pick<WorkbenchProps, "workspace"> & { readonly connection: Connection }): JSX.Element {
  const state = connection.connection.state;
  const server = state === "ready" ? "Server online" : state === "checking" ? "Server: checking" : "Server offline";
  return (
    <footer className="statusbar">
      <span className={`statusbar__panel statusbar__panel--${state}`}>
        <span className="statusbar__led" aria-hidden="true" />
        {server}
      </span>
      <button type="button" className="statusbar__panel statusbar__ai" onClick={workspace.dialogs.openSettings}>
        {aiStatusText(workspace.dialogs.aiSettings)}
      </button>
      <span className="statusbar__panel statusbar__grow">Finite model · your database is never contacted</span>
    </footer>
  );
}

/** Lead with the purpose and query, then the result, review bins and history on one page. */
export function Workbench({ workspace, results, reviews }: WorkbenchProps): JSX.Element {
  const connection = useHostConnection();
  return (
    <div className="workbench-shell">
      <WorkbenchHeader workspace={workspace} />
      <main className="workbench" id="review-workspace">
        <WorkbenchHero connection={connection} />
        <AiSetup workspace={workspace} />
        <div className="workbench-grid">
          <div className="workbench-side">
            <QueryWorkbench workspace={workspace} />
            <WorkbenchAssistance workspace={workspace} />
          </div>
          <div className="workbench-results" id="review-center">
            <Window title="Results" className="results-window">
              {results}
            </Window>
          </div>
        </div>
        {reviews}
        <WorkbenchJournal workspace={workspace} />
      </main>
      <StatusBar workspace={workspace} connection={connection} />
    </div>
  );
}
