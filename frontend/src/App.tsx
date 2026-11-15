/**
 * SQLolla evidence workspace combines explicit local analysis, labelled fixtures,
 * immutable evidence, draft building, review feedback and optional AI assistance.
 */
import type { JSX } from "react";
import { createLocalTransport } from "./analysis/transports";
import type { AnalysisTransport } from "./analysis/types";
import { WorkspaceView } from "./components/WorkspaceView";
import { useWorkspace } from "./workspace/useWorkspace";

const localTransport = createLocalTransport();

/** Optional transport injection for isolated tests and explicit local host integrations. */
export interface AppProps {
  readonly transport?: AnalysisTransport;
  readonly initialScenarioId?: string;
}

/** Compose the local workspace controller and presentational shell. */
export function App({ transport = localTransport, initialScenarioId }: AppProps): JSX.Element {
  const workspace = useWorkspace(transport, initialScenarioId);
  return <WorkspaceView workspace={workspace} />;
}
