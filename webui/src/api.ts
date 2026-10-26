import type { LogsResponse, Snapshot } from "./types";

export interface SnapshotParams {
  namespace: string | null;
  refresh: boolean;
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
  return json<Snapshot>(await fetch(`/api/snapshot${suffix}`, { headers: { Accept: "application/json" } }));
}

export async function fetchLogs(
  namespace: string,
  pod: string,
  container: string | null,
  tail: number,
): Promise<LogsResponse> {
  const search = new URLSearchParams({ namespace, pod, tail: String(tail) });
  if (container) search.set("container", container);
  return json<LogsResponse>(await fetch(`/api/logs?${search}`, { headers: { Accept: "application/json" } }));
}

export async function fetchHealth(): Promise<{ ok: boolean; version: string }> {
  return json(await fetch("/api/health"));
}
