import { useCallback, useEffect, useMemo, useState } from "react";
import { Activity, CircleAlert, Cpu, Database, Download, HardDrive, MemoryStick, Network, RotateCcw, Search, Workflow } from "lucide-react";
import { fetchTelemetry, fetchTelemetryHistory } from "../api";
import { relativeTime } from "../format";
import { currentContainerHealth, executionOomDelta, executionPools, makeExecutionBaseline, metricUsable, scalar, sectionFresh, shortPod, sourceFresh, valueText } from "../telemetry";
import type { ExecutionBaseline, TelemetryHistory, TelemetrySnapshot } from "../telemetry";
import { Badge, DataTable, Empty } from "./ui";
import { TelemetryChart } from "./TelemetryChart";
import type { ReactNode } from "react";

const CHARTS = [
  ["node_cpu_percent", "Node CPU utilization"],
  ["pod_cpu_cores", "CPU by pod"],
  ["pod_memory_working_set_bytes", "Working-set memory by pod"],
  ["datafusion_mem_pool_bytes", "Execution pool: reserved and limit"],
  ["execution_mem_pool_utilization_ratio", "Execution-pool utilization"],
  ["query_datafusion_query_execution_ooms_total", "Execution OOM counters by pod"],
  ["pod_cpu_throttled_period_ratio", "CPU throttled-period ratio by pod"],
  ["compactor_slices_queued", "Compaction slices queued"],
  ["compactor_snapshots_pending", "Compaction snapshots pending"],
  ["compactor_snapshot_lag_seconds", "Compaction snapshot lag"],
  ["compactor_output_bytes_per_second", "Compactor output throughput"],
  ["shared_filesystem_available_bytes", "Shared filesystem available"],
  ["object_storage_logical_bytes", "Object-directory logical size"],
];

function Indicator({ label, value, note, icon, warning = false }: {
  label: string; value: string; note: string; icon: ReactNode; warning?: boolean;
}) {
  return <div className={`telemetry-stat${warning ? " caution" : ""}`}>
    <div className="telemetry-stat-label">{icon}<span>{label}</span></div>
    <strong>{value}</strong><span className="telemetry-stat-note">{note}</span>
  </div>;
}

export function TelemetryView({ intervalSeconds, refreshTick }: { intervalSeconds: number; refreshTick: number }) {
  const [snapshot, setSnapshot] = useState<TelemetrySnapshot | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);
  const [tab, setTab] = useState<"performance" | "configuration" | "signals">("performance");
  const [now, setNow] = useState(Date.now());
  const [metricId, setMetricId] = useState("node_cpu_percent");
  const [seconds, setSeconds] = useState(3600);
  const [history, setHistory] = useState<TelemetryHistory | null>(null);
  const [historyError, setHistoryError] = useState<string | null>(null);
  const [historyLoading, setHistoryLoading] = useState(false);
  const [selectedPod, setSelectedPod] = useState("");
  const [search, setSearch] = useState("");
  const [settingFilter, setSettingFilter] = useState("all");
  const [executionBaseline, setExecutionBaseline] = useState<ExecutionBaseline | null>(null);

  useEffect(() => {
    let cancelled = false, busy = false;
    async function load() {
      if (busy) return;
      busy = true;
      try {
        const data = await fetchTelemetry();
        if (!cancelled) { setSnapshot(data); setError(null); }
      } catch (cause) {
        if (!cancelled) setError(cause instanceof Error ? cause.message : "Telemetry unavailable");
      } finally { busy = false; if (!cancelled) setLoading(false); }
    }
    void load();
    const timer = intervalSeconds ? window.setInterval(() => void load(), intervalSeconds * 1000) : undefined;
    return () => { cancelled = true; if (timer) window.clearInterval(timer); };
  }, [intervalSeconds, refreshTick]);

  useEffect(() => { const timer = window.setInterval(() => setNow(Date.now()), 5000); return () => window.clearInterval(timer); }, []);

  useEffect(() => {
    if (tab !== "performance" || !snapshot || error) return;
    let cancelled = false, busy = false;
    let retry: number | undefined;
    async function load(attempt = 0) {
      if (busy) return;
      busy = true;
      setHistoryLoading(true);
      try {
        const data = await fetchTelemetryHistory(metricId, seconds);
        if (!cancelled) { setHistory(data); setHistoryError(null); }
      } catch (cause) {
        if (!cancelled) {
          if ((cause as { status?: number }).status === 429 && attempt === 0) {
            retry = window.setTimeout(() => void load(1), 5100);
          } else setHistoryError(cause instanceof Error ? cause.message : "History unavailable");
        }
      } finally { busy = false; if (!cancelled) setHistoryLoading(false); }
    }
    setHistory(null); setHistoryError(null);
    void load();
    const timer = intervalSeconds ? window.setInterval(() => void load(), intervalSeconds * 1000) : undefined;
    return () => { cancelled = true; if (timer) window.clearInterval(timer); if (retry) window.clearTimeout(retry); };
    // Snapshot refreshes do not restart the independent history timer.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [tab, metricId, seconds, intervalSeconds, refreshTick, !!snapshot, error]);

  const download = useCallback(() => {
    if (!snapshot) return;
    const url = URL.createObjectURL(new Blob([JSON.stringify(snapshot, null, 2)], { type: "application/json" }));
    const link = document.createElement("a"); link.href = url; link.download = `cluster-telemetry-${new Date().toISOString().replace(/[:.]/g, "-")}.json`;
    link.click(); window.setTimeout(() => URL.revokeObjectURL(url), 1000);
  }, [snapshot]);

  const config = snapshot?.config ?? null, metrics = snapshot?.metrics ?? null;
  const pods = config?.pods ?? [];
  const configFresh = !error && sectionFresh(config, now);
  const metricsFresh = !error && sectionFresh(metrics, now);
  const get = (id: string) => error ? null : scalar(snapshot, id, now);
  useEffect(() => {
    if (!error && !executionBaseline) {
      const baseline = makeExecutionBaseline(snapshot);
      if (baseline) setExecutionBaseline(baseline);
    }
  }, [snapshot, error, executionBaseline]);
  const pools = error ? [] : executionPools(snapshot, now);
  const pool = pools.filter(p => p.role !== "webui").sort((a, b) => (b.utilization ?? -1) - (a.utilization ?? -1)
    || Number(b.pod.includes("compactor")) - Number(a.pod.includes("compactor")))[0];
  const oom = executionOomDelta(error ? null : snapshot, executionBaseline, now);
  const health = error ? [] : currentContainerHealth(snapshot, now);
  const cpu = get("node_cpu_percent"), memory = get("node_memory_used_bytes"), memoryTotal = get("node_memory_total_bytes");
  const memoryPercent = memory !== null && memoryTotal ? memory / memoryTotal * 100 : null;
  const sources = metrics?.sources ?? [];
  const up = error ? 0 : sources.filter(source => sourceFresh(source, now)).length;
  const observedPod = selectedPod || pods[0]?.pod || "*";
  const configRows = useMemo(() => pods.filter(pod => observedPod === "*" || pod.pod === observedPod)
    .flatMap(pod => pod.settings.map(setting => ({ ...setting, pod: pod.pod })))
    .filter(setting => !search || [setting.option, setting.pod, setting.source,
      ...setting.container_environment.map(e => e.name)].join(" ").toLowerCase().includes(search.toLowerCase()))
    .filter(setting => settingFilter === "all" || (settingFilter === "observed" && setting.value !== null)
      || (settingFilter === "unsupported" && setting.source === "unrecognized_environment_input")
      || (settingFilter === "drift" && setting.configmap_or_spec_drift)), [pods, observedPod, search, settingFilter]);
  const unknownSignals = metrics?.metrics.filter(metric => !metricUsable(metric, metrics, now)).length ?? 0;
  const title = CHARTS.find(([id]) => id === metricId)?.[1] ?? metricId;
  const metric = (id: string) => metrics?.metrics.find(row => row.id === id);
  const podValue = (id: string, pod: string) => {
    const row = metric(id);
    return !error && metricUsable(row, metrics, now) ? row!.samples.find(s => s.labels.pod === pod)?.value ?? null : null;
  };
  const hasHistory = history?.series.some(series => series.values.some(([, value]) => value !== null));

  return <div className="telemetry-view">
    <div className="telemetry-heading">
      <div><div className="telemetry-cluster-name">{config?.cluster_ids.join(", ") || "Cluster telemetry"}</div>
        <div className="telemetry-metadata">{config?.namespace || "\u2014"}<span>Enterprise {pods[0]?.image.match(/:(.+?)-enterprise/)?.[1] || "\u2014"}</span>
          <span>Contract {snapshot?.schema_version || "1.0"}</span></div></div>
      <div className="row"><Badge tone={error ? "danger" : metricsFresh ? "ok" : "warn"}>{error ? "Disconnected" : metricsFresh ? "Live observations" : loading ? "Connecting" : "Stale observations"}</Badge>
        <button type="button" className="btn telemetry-icon" title="Download sanitized snapshot" aria-label="Download sanitized snapshot" onClick={download} disabled={!snapshot}><Download size={17} /></button></div>
    </div>
    {error && <div className="error-banner" role="alert">{error}{snapshot ? ". Retained observations are not current." : ""}</div>}
    <div className="telemetry-tabs" role="tablist" aria-label="Telemetry views">
      {(["performance", "configuration", "signals"] as const).map(name => <button type="button" role="tab" key={name}
        aria-selected={tab === name} onClick={() => setTab(name)}>{name === "performance" ? "Performance" : name === "configuration" ? "Running configuration" : "Signals & sources"}</button>)}
      <span className="telemetry-updated">{metrics ? relativeTime(metrics.observed_at) : "No observations"}</span>
    </div>
    {loading && !snapshot && <div className="skeleton" style={{ height: 220 }} />}
    {!loading && !snapshot && !error && <Empty>No telemetry observations</Empty>}
    {snapshot && tab === "performance" && <>
      <div className="telemetry-stats">
        <Indicator label="Node CPU" value={valueText(cpu, "percent")} note={`${valueText(get("node_cpu_cores"), "cores")} capacity`} icon={<Cpu size={16} />} />
        <Indicator label="Node memory" value={valueText(memoryPercent, "percent")} note={`${valueText(memory, "bytes")} / ${valueText(memoryTotal, "bytes")}`} icon={<MemoryStick size={16} />} warning={memoryPercent !== null && memoryPercent > 85} />
        <Indicator label="Execution pool" value={valueText(pool?.utilization, "ratio")} note={pool ? `${shortPod(pool.pod)}: ${valueText(pool.reserved, "bytes")} / ${valueText(pool.limit, "bytes")}` : "Pool reservation unavailable"} icon={<MemoryStick size={16} />} warning={pool?.utilization !== null && (pool?.utilization ?? 0) > 0.85} />
        <Indicator label="New execution OOMs" value={valueText(oom.value, "events")} note={oom.reset ? "Process changed or counter reset" : executionBaseline ? `${valueText(oom.total, "events")} cumulative; observing since ${new Date(executionBaseline.observed_at).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" })}` : "Observation baseline unavailable"} icon={<CircleAlert size={16} />} warning={oom.reset || (oom.value ?? 0) > 0} />
        <Indicator label="Compaction queue" value={valueText(get("compactor_slices_queued"), "slices")} note={`${valueText(get("compactor_snapshots_pending"), "snapshots")} snapshots pending`} icon={<Workflow size={16} />} />
        <Indicator label="DB working set" value={valueText(get("database_memory_working_set_bytes_total"), "bytes")} note="Processing pods" icon={<Database size={16} />} />
        <Indicator label="Filesystem available" value={valueText(get("shared_filesystem_available_bytes"), "bytes")} note="Observed shared filesystem" icon={<HardDrive size={16} />} />
        <Indicator label="Scrape sources" value={`${up} / ${sources.length}`} note={up === sources.length && sources.length ? "All sources current" : "Sources missing or stale"} icon={<Network size={16} />} warning={up !== sources.length} />
      </div>
      <section className="telemetry-band">
        <div className="telemetry-section-heading"><h2><Activity size={17} />Performance history</h2>
          <div className="row"><select aria-label="History metric" value={metricId} onChange={event => setMetricId(event.target.value)}>
            {CHARTS.map(([id, label]) => <option key={id} value={id}>{label}</option>)}</select>
            <select aria-label="History time range" value={seconds} onChange={event => setSeconds(Number(event.target.value))}>
              <option value={900}>15 minutes</option><option value={3600}>1 hour</option><option value={21600}>6 hours</option><option value={86400}>24 hours</option></select>
          </div></div>
        <div className="telemetry-chart-frame" aria-busy={historyLoading}>
          {historyLoading && <span className="telemetry-chart-status">Updating</span>}
          {historyError && <div className="telemetry-chart-error" role="alert">{historyError}</div>}
          {hasHistory && history ? <TelemetryChart data={history} title={title} /> : <Empty>{historyLoading ? "Loading history" : "No finite measurements in this period"}</Empty>}
        </div>
        <div className="telemetry-footnote">{title}<span>{history ? `${history.series.length} series \u00b7 ${history.step}s resolution` : "\u2014"}</span></div>
      </section>
      <section className="telemetry-band">
        <div className="telemetry-section-heading"><h2>Enterprise pod resources</h2><span className="muted">{configFresh ? "Observed configuration" : "Configuration not current"}</span></div>
        <DataTable rows={pods} rowKey={pod => pod.pod} columns={[
          { key: "pod", label: "Pod", render: pod => <span title={pod.pod}>{shortPod(pod.pod)}</span>, sortValue: pod => pod.pod, mono: true },
          { key: "ready", label: "State", render: pod => <Badge tone={!configFresh ? "warn" : pod.ready ? "ok" : "danger"}>{!configFresh ? "Stale" : pod.ready ? "Ready" : "Not ready"}</Badge> },
          { key: "cpu", label: "CPU", render: pod => valueText(podValue("pod_cpu_cores", pod.pod), "cores"), align: "right" },
          { key: "memory", label: "Working set", render: pod => valueText(podValue("pod_memory_working_set_bytes", pod.pod), "bytes"), align: "right" },
          { key: "limit", label: "Memory limit", render: pod => pod.resources.limits?.memory || "Not set", align: "right" },
          { key: "throttle", label: "CPU throttling", render: pod => valueText(podValue("pod_cpu_throttled_period_ratio", pod.pod), "ratio"), align: "right" },
          { key: "restarts", label: "Restarts", render: pod => podValue("pod_restarts", pod.pod) ?? "\u2014", align: "right" },
        ]} />
      </section>
      <section className="telemetry-band">
        <div className="telemetry-section-heading"><h2>Execution memory and container exits</h2>
          <button type="button" className="btn telemetry-icon" aria-label="Reset execution-OOM observation baseline" title="Reset execution-OOM observation baseline" disabled={!makeExecutionBaseline(error ? null : snapshot, now)} onClick={() => setExecutionBaseline(makeExecutionBaseline(snapshot, now))}><RotateCcw size={16} /></button></div>
        <DataTable rows={pods} rowKey={pod => pod.pod} columns={[
          { key: "pod", label: "Pod", render: pod => shortPod(pod.pod), mono: true },
          { key: "reserved", label: "Exec reserved", render: pod => valueText(pools.find(p => p.pod === pod.pod)?.reserved, "bytes"), align: "right" },
          { key: "limit", label: "Exec pool limit", render: pod => valueText(pools.find(p => p.pod === pod.pod)?.limit, "bytes"), align: "right" },
          { key: "utilization", label: "Pool used", render: pod => valueText(pools.find(p => p.pod === pod.pod)?.utilization, "ratio"), align: "right" },
          { key: "ooms", label: "Exec OOM counter", render: pod => valueText(podValue("query_datafusion_query_execution_ooms_total", pod.pod), "events"), align: "right" },
          { key: "new_ooms", label: "New in this view", render: pod => valueText(oom.byPod.get(pod.pod), "events"), align: "right" },
          { key: "last_exit", label: "Container exit", render: pod => {
            const state = health.find(h => h.pod === pod.pod);
            const exit = state?.current_termination || state?.last_termination;
            return !state ? "Unavailable" : !exit ? "None recorded" : <Badge tone={exit.oom_killed ? "danger" : "warn"} title={exit.finished_at || undefined}>{exit.reason || "Unknown"}{exit.exit_code !== null ? ` (${exit.exit_code})` : ""}</Badge>;
          } },
        ]} />
      </section>
      <div className="telemetry-details-grid">
        <section className="telemetry-band"><div className="telemetry-section-heading"><h2>Compactor</h2><Badge tone={get("compactor_primary") === 1 ? "ok" : "warn"}>{get("compactor_primary") === 1 ? "Primary observed" : "Primary unknown"}</Badge></div>
          <dl className="telemetry-kv">
            {[["Memory in use", "compactor_memory_used_bytes", "bytes"], ["Input budget", "compactor_memory_budget_bytes", "bytes"], ["Active plans", "compactor_plans_active", "plans"], ["Output rate", "compactor_output_bytes_per_second", "bytes_per_second"]].map(([label, id, unit]) => <div key={id}><dt>{label}</dt><dd>{valueText(get(id), unit)}</dd></div>)}
          </dl></section>
        <section className="telemetry-band"><div className="telemetry-section-heading"><h2>Object directory</h2><span className="muted">Logical size</span></div>
          <dl className="telemetry-kv">{[["Regular-file bytes", "object_storage_logical_bytes", "bytes"], ["File count", "object_storage_file_count", "files"], ["Scan age", "object_storage_scan_age_seconds", "seconds"]].map(([label, id, unit]) => <div key={id}><dt>{label}</dt><dd>{valueText(get(id), unit)}</dd></div>)}</dl></section>
      </div>
      {unknownSignals > 0 && <button type="button" className="telemetry-signal-notice" onClick={() => setTab("signals")}><CircleAlert size={15} />{unknownSignals} unavailable or idle {unknownSignals === 1 ? "signal" : "signals"}</button>}
    </>}
    {snapshot && tab === "configuration" && <section className="telemetry-band">
      <div className="telemetry-section-heading"><h2>Startup settings</h2><Badge tone={configFresh ? "ok" : "warn"}>{configFresh ? "Current observation" : "Stale or unavailable"}</Badge></div>
      <div className="telemetry-filters"><select aria-label="Configuration pod" value={observedPod} onChange={event => setSelectedPod(event.target.value)}><option value="*">All Enterprise pods</option>{pods.map(pod => <option value={pod.pod} key={pod.pod}>{shortPod(pod.pod)}</option>)}</select>
        <label className="telemetry-search"><Search size={15} /><input type="search" aria-label="Search startup settings" placeholder="Find a setting or environment name" value={search} onChange={event => setSearch(event.target.value)} /></label>
        <select aria-label="Configuration filter" value={settingFilter} onChange={event => setSettingFilter(event.target.value)}><option value="all">All options</option><option value="observed">Observed inputs</option><option value="unsupported">Unrecognized inputs</option><option value="drift">Configuration drift</option></select>
        <span className="muted">{configRows.length} settings</span></div>
      <DataTable rows={configRows} rowKey={row => `${row.pod}/${row.option}`} pageSize={50} columns={[
        ...(observedPod === "*" ? [{ key: "pod", label: "Pod", render: (row: typeof configRows[number]) => shortPod(row.pod), mono: true }] : []),
        { key: "option", label: "Option", render: row => <span className="telemetry-option" title={row.container_environment.map(e => `${e.name}=${e.value ?? "unknown"}`).join("\n")}>--{row.option}</span>, sortValue: row => row.option },
        { key: "value", label: "Observed input", render: row => row.value ?? <span className="muted">Unknown</span>, mono: true },
        { key: "default", label: "Help default", render: row => row.documented_default ?? <span className="muted">{"\u2014"}</span>, mono: true },
        { key: "source", label: "Evidence", render: row => <Badge tone={row.source === "unrecognized_environment_input" ? "warn" : row.value !== null ? "info" : "neutral"} title={row.source}>{row.source === "unrecognized_environment_input" ? "Unrecognized env" : row.source === "pod_spec_argument" ? "Pod argument" : row.source === "container_environment" ? "Container env" : row.source === "not_observed" ? "Not observed" : row.source.replaceAll("_", " ")}</Badge> },
        { key: "supported", label: "CLI support", render: row => row.cli_option_supported === null ? "Unknown" : row.cli_option_supported ? "Yes" : "No" },
        { key: "declared", label: "Declared now", render: row => row.declared_environment.map(e => e.value ?? "Unknown").join(", ") || "\u2014", mono: true },
        { key: "drift", label: "Drift", render: row => row.configmap_or_spec_drift === null ? "Unknown" : row.configmap_or_spec_drift ? <Badge tone="warn">Changed</Badge> : "No" },
      ]} />
      <div className="telemetry-footnote"><span>Observed {relativeTime(config?.observed_at)}</span><span className="mono" title={config?.snapshot_id}>Snapshot {config?.snapshot_id.slice(0, 12) || "\u2014"}</span></div>
    </section>}
    {snapshot && tab === "signals" && <>
      <section className="telemetry-band"><div className="telemetry-section-heading"><h2>Collection sources</h2><Badge tone={up === sources.length && metricsFresh ? "ok" : "warn"}>{up}/{sources.length} current</Badge></div>
        <DataTable rows={sources} rowKey={source => `${source.job}/${source.pod || source.node || ""}`} columns={[
          { key: "job", label: "Collector", render: source => source.job, sortValue: source => source.job },
          { key: "pod", label: "Target", render: source => source.pod ? shortPod(source.pod) : source.node || "\u2014", mono: true },
          { key: "health", label: "Status", render: source => <Badge tone={!error && sourceFresh(source, now) ? "ok" : "warn"}>{!error && sourceFresh(source, now) ? "Current" : source.health === "up" ? "Stale" : source.health}</Badge> },
          { key: "time", label: "Last scrape", render: source => relativeTime(source.last_scrape) },
        ]} /></section>
      <section className="telemetry-band"><div className="telemetry-section-heading"><h2>Metric signals</h2><span className="muted">{metrics?.metrics.length ?? 0} catalog entries</span></div>
        <DataTable rows={metrics?.metrics ?? []} rowKey={row => row.id} searchText={row => row.id} searchPlaceholder="Filter metric signals" columns={[
          { key: "id", label: "Signal", render: row => row.id, sortValue: row => row.id, mono: true },
          { key: "status", label: "Quality", render: row => { const usable = !error && metricUsable(row, metrics, now); return <Badge tone={usable ? "ok" : "warn"}>{usable ? "Current" : row.status === "non_finite" ? "Idle / not finite" : row.status === "ok" ? "Stale" : row.status.replaceAll("_", " ")}</Badge>; } },
          { key: "value", label: "Value", render: row => row.samples.map(sample => valueText(sample.value, row.unit)).join(", ") || "\u2014" },
          { key: "unit", label: "Unit", render: row => row.unit },
          { key: "series", label: "Series", render: row => row.samples.length, align: "right" },
        ]} /></section>
    </>}
  </div>;
}
