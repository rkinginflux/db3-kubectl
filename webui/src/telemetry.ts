export interface TelemetrySetting {
  option: string;
  value: string | null;
  source: string;
  state: string;
  documented_default: string | null;
  cli_option_supported: boolean | null;
  recognized_environment_names: string[] | null;
  configmap_or_spec_drift: boolean | null;
  container_environment: { name: string; value: string | null }[];
  declared_environment: { name: string; value: string | null; source: string }[];
}
export interface TelemetryPod {
  pod: string; node: string; image: string; ready: boolean; restarts: number;
  started_at: string | null; chart: string | null;
  resources: { requests?: Record<string, string>; limits?: Record<string, string> };
  settings: TelemetrySetting[];
}
export interface FreshSection {
  observed_at: string; max_age_seconds: number; fresh: boolean; status: string;
}
export interface Metric {
  id: string; unit: string; status: string;
  samples: { labels: Record<string, string>; value: number | null; evaluated_at: string }[];
}
export interface TelemetrySource {
  job: string; pod?: string; role?: string; node?: string;
  health: string; last_scrape: string | null; age_seconds: number | null;
}
export interface Termination {
  reason: string | null; oom_killed: boolean; exit_code: number | null;
  signal: number | null; started_at: string | null; finished_at: string | null;
}
export interface ContainerHealth {
  pod: string; pod_uid: string; container: string; container_id: string | null;
  ready: boolean; restarts: number; current_state: string;
  last_termination: Termination | null; current_termination: Termination | null;
}
export interface TelemetrySnapshot {
  schema_version: string; served_at: string; status: string; instance_id: string;
  collection_errors: Record<string, unknown>;
  config: (FreshSection & { namespace: string; context: string; release: string; cluster_ids: string[];
    snapshot_id: string; errors: string[]; pods: TelemetryPod[]; nodes: unknown[] }) | null;
  metrics: (FreshSection & { metrics: Metric[]; sources: TelemetrySource[];
    container_health?: { observed_at: string; status: string; pods: ContainerHealth[] } }) | null;
}
export interface TelemetryHistory {
  schema_version: string; metric: string; unit: string; start: number; end: number; step: number;
  series: { labels: Record<string, string>; values: [number, number | null][] }[];
  gaps_are_unknown: boolean; annotations_present: boolean;
}

export function sectionFresh(section: FreshSection | null | undefined, now = Date.now()): boolean {
  if (!section?.fresh || section.status === "stale") return false;
  const age = now - Date.parse(section.observed_at);
  return Number.isFinite(age) && age >= -30000 && age <= section.max_age_seconds * 1000;
}

export function sourceRelevant(id: string, source: TelemetrySource): boolean {
  if (["query_datafusion_query_execution_ooms_total", "datafusion_mem_pool_bytes", "execution_mem_pool_utilization_ratio"].includes(id)) return source.job === "influxdb3";
  if (id.startsWith("compactor_")) return source.job === "influxdb3" && source.role === "compactor";
  if (["database_scrape_health", "file_cache_bytes_by_pod"].includes(id)) return source.job === "influxdb3";
  if (["object_storage_logical_bytes", "object_storage_file_count", "object_storage_scan_age_seconds"].includes(id)) return source.job === "object-storage";
  if (["pod_cpu_requested_cores", "pod_memory_limit_bytes", "object_storage_requested_bytes", "pod_restarts"].includes(id)) return source.job === "kube-state-metrics";
  if (/^(pod_|database_|monitoring_)/.test(id)) return source.job === "cadvisor";
  return id === "scrape_health" || source.job === "node";
}

export function sourceFresh(source: TelemetrySource, now = Date.now()): boolean {
  const age = source.last_scrape ? now - Date.parse(source.last_scrape) : NaN;
  return source.health === "up" && Number.isFinite(age) && age >= -30000 && age <= 90000;
}

export function metricUsable(metric: Metric | undefined, section: TelemetrySnapshot["metrics"], now = Date.now()): boolean {
  if (!metric || !sectionFresh(section, now) || metric.status !== "ok") return false;
  const sources = section!.sources.filter(source => sourceRelevant(metric.id, source));
  return sources.length > 0 && sources.every(source => sourceFresh(source, now));
}

export function scalar(snapshot: TelemetrySnapshot | null, id: string, now = Date.now()): number | null {
  const metric = snapshot?.metrics?.metrics.find(row => row.id === id);
  if (!metricUsable(metric, snapshot?.metrics ?? null, now)) return null;
  const values = metric!.samples.map(sample => sample.value);
  if (values.length !== 1 || values[0] === null || !Number.isFinite(values[0])) return null;
  return values[0];
}

export function valueText(value: number | null | undefined, unit: string): string {
  if (value === null || value === undefined || !Number.isFinite(value)) return "\u2014";
  if (unit === "bytes" || unit === "bytes_per_second") {
    const units = ["B", "KiB", "MiB", "GiB", "TiB"];
    let number = value, index = 0;
    while (Math.abs(number) >= 1024 && index < units.length - 1) { number /= 1024; index++; }
    return `${number.toLocaleString(undefined, { maximumFractionDigits: index ? 1 : 0 })} ${units[index]}${unit.endsWith("per_second") ? "/s" : ""}`;
  }
  if (unit === "percent") return `${value.toFixed(1)}%`;
  if (unit === "ratio") return `${(value * 100).toFixed(1)}%`;
  if (unit === "seconds") return `${value.toLocaleString(undefined, { maximumFractionDigits: 1 })} s`;
  if (unit === "cores") return `${value.toFixed(2)} cores`;
  if (unit === "boolean") return value === 1 ? "Yes" : "No";
  return value.toLocaleString(undefined, { maximumFractionDigits: 2 }) + (unit.endsWith("_per_second") ? "/s" : "");
}

export function shortPod(pod: string): string {
  return pod.match(/(?:^|-)((?:ingester|querier|compactor|processor|webui)-\d+)$/)?.[1] ?? pod;
}

export function currentContainerHealth(snapshot: TelemetrySnapshot | null, now = Date.now()): ContainerHealth[] {
  const health = snapshot?.metrics?.container_health;
  const age = health ? now - Date.parse(health.observed_at) : NaN;
  return sectionFresh(snapshot?.metrics, now) && health?.status === "ok" && age >= -30000 && age <= 90000 ? health.pods : [];
}

export function executionPools(snapshot: TelemetrySnapshot | null, now = Date.now()) {
  const metric = snapshot?.metrics?.metrics.find(m => m.id === "datafusion_mem_pool_bytes");
  if (!metricUsable(metric, snapshot?.metrics ?? null, now)) return [];
  if (metric!.samples.some(sample => !sample.labels.pod)) return [];
  const pods = [...new Set(metric!.samples.map(s => s.labels.pod))];
  return pods.map(pod => {
    const reservations = metric!.samples.filter(s => s.labels.pod === pod && s.labels.state === "reserved");
    const limits = metric!.samples.filter(s => s.labels.pod === pod && s.labels.state === "limit");
    const reserved = reservations.length === 1 ? reservations[0].value : null;
    const limit = limits.length === 1 ? limits[0].value : null;
    return { pod, role: reservations[0]?.labels.role, reserved, limit,
      utilization: reserved !== null && limit !== null && limit > 0 ? reserved / limit : null };
  });
}

export interface ExecutionBaseline {
  observed_at: string;
  counters: Map<string, { identity: string; count: number }>;
}

export function makeExecutionBaseline(snapshot: TelemetrySnapshot | null, now = Date.now()): ExecutionBaseline | null {
  const metric = snapshot?.metrics?.metrics.find(m => m.id === "query_datafusion_query_execution_ooms_total");
  if (!metricUsable(metric, snapshot?.metrics ?? null, now)) return null;
  const health = currentContainerHealth(snapshot, now);
  const rows = metric!.samples.filter(s => s.labels.role !== "webui");
  const expected = new Set(snapshot!.metrics!.sources
    .filter(source => source.job === "influxdb3" && source.role !== "webui")
    .map(source => source.pod));
  if (!rows.length || !expected.size || expected.has(undefined) || rows.length !== expected.size) return null;
  const counters: ExecutionBaseline["counters"] = new Map();
  for (const row of rows) {
    const pod = health.find(p => p.pod === row.labels.pod);
    if (!expected.has(row.labels.pod) || !pod?.container_id || !pod.pod_uid
      || pod.current_state !== "running" || row.value === null || !Number.isFinite(row.value) || row.value < 0) return null;
    if (counters.has(pod.pod)) return null;
    counters.set(pod.pod, { identity: `${pod.pod_uid}/${pod.container_id}`, count: row.value });
  }
  return { observed_at: snapshot!.metrics!.observed_at, counters };
}

export function executionOomDelta(snapshot: TelemetrySnapshot | null, baseline: ExecutionBaseline | null, now = Date.now()) {
  const current = makeExecutionBaseline(snapshot, now);
  const total = current ? [...current.counters.values()].reduce((sum, item) => sum + item.count, 0) : null;
  if (!current || !baseline) return { value: null, total, reset: false, byPod: new Map<string, number>() };
  const byPod = new Map<string, number>();
  for (const [pod, sample] of current.counters) {
    const prior = baseline.counters.get(pod);
    if (!prior || prior.identity !== sample.identity || sample.count < prior.count) {
      return { value: null, total, reset: true, byPod: new Map<string, number>() };
    }
    byPod.set(pod, sample.count - prior.count);
  }
  if (current.counters.size !== baseline.counters.size) return { value: null, total, reset: true, byPod: new Map<string, number>() };
  return { value: [...byPod.values()].reduce((sum, value) => sum + value, 0), total, reset: false, byPod };
}
