import type { LogsResponse, QueryResponse, Snapshot } from "./types";

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
    throw new Error(text.trim() || `HTTP ${response.status}`);
  }
  return (await response.json()) as T;
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
