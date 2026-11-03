import type { InfoResponse, LogsResponse, QueryResponse, Snapshot } from "./types";
import type { TelemetryHistory, TelemetrySnapshot } from "./telemetry";

export interface SnapshotParams {
  namespace: string | null;
  refresh: boolean;
}

function request(path: string): Promise<Response> {
  const session = document.querySelector<HTMLMetaElement>('meta[name="db3-session"]')?.content;
  if (!session || session.startsWith("__DB3_")) throw new Error("Console session unavailable; reload the page");
  return fetch(path, { cache: "no-store", credentials: "omit", headers: { Accept: "application/json", "X-DB3-Session": session } });
}

async function json<T>(response: Response): Promise<T> {
  if (!response.ok) {
    const text = await response.text();
    let message = text.trim() || `HTTP ${response.status}`;
    try { message = JSON.parse(text).error ?? message; } catch { /* plain-text errors remain readable */ }
    throw Object.assign(new Error(message), { status: response.status });
  }
  return (await response.json()) as T;
}

export async function fetchTelemetry(): Promise<TelemetrySnapshot> {
  const result = await json<TelemetrySnapshot>(await request("/api/telemetry"));
  if (result.schema_version !== "1.0") throw new Error("Unsupported telemetry contract");
  return result;
}

export async function fetchTelemetryHistory(metric: string, seconds: number): Promise<TelemetryHistory> {
  const end = Math.floor(Date.now() / 1000);
  const search = new URLSearchParams({ metric, start: String(end - seconds), end: String(end),
    step: String(seconds <= 3600 ? 30 : seconds <= 21600 ? 120 : 600) });
  return json<TelemetryHistory>(await request(`/api/telemetry/history?${search}`));
}

export async function fetchSnapshot(params: SnapshotParams): Promise<Snapshot> {
  const search = new URLSearchParams();
  if (params.namespace) search.set("namespace", params.namespace);
  if (params.refresh) search.set("refresh", "1");
  const suffix = search.toString() ? `?${search}` : "";
  return json<Snapshot>(await request(`/api/snapshot${suffix}`));
}

export async function fetchLogs(
  namespace: string,
  pod: string,
  container: string | null,
  tail: number,
): Promise<LogsResponse> {
  const search = new URLSearchParams({ namespace, pod, tail: String(tail) });
  if (container) search.set("container", container);
  return json<LogsResponse>(await request(`/api/logs?${search}`));
}

export async function fetchHealth(): Promise<{ ok: boolean; version: string }> {
  return json(await request("/api/health"));
}

export interface QueryParams {
  namespace: string;
  pod: string;
  container: string | null;
  database: string;
  language: "sql" | "influxql";
  statement: string;
}

export async function runQuery(params: QueryParams): Promise<QueryResponse> {
  const search = new URLSearchParams({
    namespace: params.namespace,
    pod: params.pod,
    database: params.database,
    language: params.language,
    q: params.statement,
  });
  if (params.container) search.set("container", params.container);
  return json<QueryResponse>(await request(`/api/query?${search}`));
}

/** Runs `influxdb3 show nodes` and `show license info` in the pod via pods/exec. */
export async function fetchInfo(params: {
  namespace: string;
  pod: string;
  container: string | null;
}): Promise<InfoResponse> {
  const search = new URLSearchParams({ namespace: params.namespace, pod: params.pod });
  if (params.container) search.set("container", params.container);
  return json<InfoResponse>(await request(`/api/info?${search}`));
}
