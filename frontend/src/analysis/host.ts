/** Same-origin local host boundary; CSRF state is memory-only and provider credentials are never cached. */
import { asRecord, stringField } from "./guards";
import { parseJson, stringifyJson } from "./json";

/** Exact public host capability snapshot, independent of provider authentication. */
export interface HostStatus {
  readonly schema: "sqlolla.host.v1";
  readonly model: "gpt-6.1-sol";
  readonly csrf_token: string;
}

/** One request's cancellation and optional transient Authorization header. */
export interface HostOptions {
  readonly signal?: AbortSignal;
  readonly headers?: Readonly<Record<string, string>>;
}

/** Host failure retains a code but never includes request headers or body credentials. */
export class HostError extends Error {
  /** Retain a typed failure code without storing request data or credentials. */
  constructor(
    public readonly code: string,
    message: string,
  ) {
    super(message);
    this.name = "HostError";
  }
}

let statusRequest: Promise<HostStatus> | null = null;
const MAX_RESPONSE = 32 * 1024 * 1024;

/** Validate only the declared local host identity and nonce, without interpreting solver claims. */
function statusPayload(value: unknown): HostStatus {
  const object = asRecord(value, "host status");
  if (object["schema"] !== "sqlolla.host.v1" || object["model"] !== "gpt-6.1-sol") {
    throw new HostError("host.protocol", "This local host uses an unsupported protocol or model");
  }
  const token = stringField(object, "csrf_token", "host status");
  if (!/^[0-9a-f]{64}$/.test(token)) throw new HostError("host.protocol", "Invalid local host nonce");
  return { schema: "sqlolla.host.v1", model: "gpt-6.1-sol", csrf_token: token };
}

/** Read a bounded response without passing JSON numbers through response.json(). */
async function responseJson(response: Response): Promise<unknown> {
  if (response.type === "opaqueredirect" || (response.status >= 300 && response.status < 400)) {
    statusRequest = null;
    throw new HostError("host.sign_in", "Sign in to SQLolla again, then retry this action.");
  }
  if (!response.headers.get("content-type")?.toLowerCase().includes("application/json")) {
    if (response.status === 401 || response.status === 403) {
      statusRequest = null;
      throw new HostError("host.sign_in", "The gateway refused this session. Sign in to SQLolla again.");
    }
    throw new HostError("host.invalid_response", "The site did not return a SQLolla API response. Check that the analysis server is connected.");
  }
  const text = await response.text();
  if (text.length > MAX_RESPONSE) throw new HostError("host.response_limit", "Local response exceeds the browser limit");
  let payload: unknown;
  try {
    payload = parseJson(text);
  } catch {
    throw new HostError("host.invalid_json", "Local host returned invalid JSON");
  }
  if (!response.ok) {
    if (response.status === 403) statusRequest = null;
    const object = typeof payload === "object" && payload !== null ? (payload as Record<string, unknown>) : {};
    throw new HostError(
      typeof object["code"] === "string" ? object["code"] : `host.http_${response.status}`,
      typeof object["message"] === "string" ? object["message"] : "Local request failed",
    );
  }
  return payload;
}

/** Read the local nonce once; failures remain retryable and never fall back to another endpoint. */
export function hostStatus(): Promise<HostStatus> {
  statusRequest ??= fetch("/api/status", { cache: "no-store", credentials: "same-origin", redirect: "manual" })
    .then(responseJson)
    .then(statusPayload)
    .catch((error: unknown) => {
      statusRequest = null;
      throw error;
    });
  return statusRequest;
}

/** Send one exact command or AI request to the same origin, with request-lifetime headers only. */
export async function hostJson(path: string, body: unknown, options: HostOptions = {}): Promise<unknown> {
  if (!path.startsWith("/api/") || path.includes("\\") || path.includes("..")) throw new HostError("host.path", "Expected a local API path");
  const status = await hostStatus();
  const headers = new Headers(options.headers);
  headers.set("Content-Type", "application/json");
  headers.set("X-SQLolla-CSRF", status.csrf_token);
  const response = await fetch(path, {
    method: "POST",
    body: stringifyJson(body),
    headers,
    signal: options.signal,
    credentials: "same-origin",
    cache: "no-store",
    redirect: "manual",
  });
  return responseJson(response);
}

/** Clear only cached public host session state, for explicit reconnect and isolated tests. */
export function resetHostSession(): void {
  statusRequest = null;
}
