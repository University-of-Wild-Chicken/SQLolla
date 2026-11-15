import { HostError, hostJson } from "./host";
import type { AnalysisTransport, TransportResult } from "./types";

/** Report a host failure without substituting example evidence. */
async function hostCommand(command: unknown): Promise<TransportResult> {
  try {
    return { ok: true, payload: await hostJson("/api/command", command) };
  } catch (error) {
    return {
      ok: false,
      error: {
        kind: "transport",
        code: error instanceof HostError ? error.code : "transport.host_unavailable",
        message: error instanceof HostError ? error.message : "SQLolla could not reach the analysis server. Try again.",
        retryable: true,
      },
    };
  }
}

/** All production cases are created, reopened and advanced by the same-origin Rust host. */
export const localTransport: AnalysisTransport = {
  id: "sqlolla-local-host-v1",
  label: "SQLolla analysis host",
  kind: "local-host",
  open: hostCommand,
  next: hostCommand,
  start: hostCommand,
  command: hostCommand,
};
