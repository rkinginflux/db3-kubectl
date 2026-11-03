import { expect, test } from "vitest";
import { executionOomDelta, executionPools, makeExecutionBaseline, metricUsable, scalar, sectionFresh, shortPod, sourceFresh, valueText } from "./telemetry";
import type { Metric, TelemetrySnapshot } from "./telemetry";

const now = Date.now();
const source = { job: "node", health: "up", last_scrape: new Date(now).toISOString(), age_seconds: 0 };
const section = { observed_at: new Date(now).toISOString(), max_age_seconds: 90, fresh: true,
  status: "ok", sources: [source], metrics: [] };
const metric: Metric = { id: "node_cpu_percent", unit: "percent", status: "ok",
  samples: [{ labels: {}, value: 0, evaluated_at: new Date(now).toISOString() }] };

test("zero is measured, unknown and NaN are not converted to zero", () => {
  expect(valueText(0, "bytes")).toBe("0 B");
  expect(valueText(0, "percent")).toBe("0.0%");
  expect(valueText(null, "bytes")).toBe("\u2014");
  expect(valueText(NaN, "percent")).toBe("\u2014");
});
test("pod display names are independent of the lab release name", () => {
  expect(shortPod("customer-db3-compactor-0")).toBe("compactor-0");
  expect(shortPod("another-release-querier-2")).toBe("querier-2");
  expect(shortPod("custom-node-name")).toBe("custom-node-name");
});
test("age keeps advancing while auto-refresh is off", () => {
  expect(sectionFresh(section, now)).toBe(true);
  expect(sectionFresh(section, now + 91000)).toBe(false);
  expect(metricUsable(metric, section, now + 91000)).toBe(false);
});
test("cached source ages matter independently of the snapshot timestamp", () => {
  const old = { ...source, last_scrape: new Date(now - 100000).toISOString() };
  expect(metricUsable(metric, { ...section, sources: [old] }, now)).toBe(false);
  expect(sourceFresh({ ...source, last_scrape: new Date(now + 100000).toISOString() }, now)).toBe(false);
});
test("no-data, failed sources and non-finite metrics stay unusable", () => {
  expect(metricUsable({ ...metric, status: "no_data" }, section, now)).toBe(false);
  expect(metricUsable(metric, { ...section, sources: [{ ...source, health: "down" }] }, now)).toBe(false);
  expect(metricUsable(metric, { ...section, sources: [] }, now)).toBe(false);
});
test("a scalar cannot silently add unrelated series", () => {
  const snapshot = { metrics: { ...section, metrics: [metric] } } as unknown as TelemetrySnapshot;
  expect(scalar(snapshot, "node_cpu_percent", now)).toBe(0);
  snapshot.metrics!.metrics[0] = { ...metric, samples: [...metric.samples, ...metric.samples] };
  expect(scalar(snapshot, "node_cpu_percent", now)).toBeNull();
});

function executionFixture(count = 39, identity = "container-1"): TelemetrySnapshot {
  return { schema_version: "1.0", served_at: new Date(now).toISOString(), status: "ok", instance_id: "fixture",
    collection_errors: {}, config: null,
    metrics: { ...section, sources: [{ ...source, job: "influxdb3", role: "compactor", pod: "compactor" }],
    container_health: { status: "ok", observed_at: new Date(now).toISOString(), pods: [
      { pod: "compactor", pod_uid: "pod-1", container: "influxdb3", container_id: identity,
        ready: true, restarts: 0, current_state: "running", last_termination: null, current_termination: null }] },
    metrics: [
      { ...metric, id: "query_datafusion_query_execution_ooms_total", unit: "events", samples: [
        { labels: { pod: "compactor", role: "compactor" }, value: count, evaluated_at: new Date(now).toISOString() }] },
      { ...metric, id: "datafusion_mem_pool_bytes", unit: "bytes", samples: [
        { labels: { pod: "compactor", role: "compactor", state: "reserved" }, value: 100, evaluated_at: new Date(now).toISOString() },
        { labels: { pod: "compactor", role: "compactor", state: "limit" }, value: 200, evaluated_at: new Date(now).toISOString() }] },
    ] } };
}

test("execution pool divides reserved by limit without summing the two states", () => {
  const pools = executionPools(executionFixture(), now);
  expect(pools[0]).toMatchObject({ reserved: 100, limit: 200, utilization: 0.5 });
});
test("ambiguous pool series and missing pod labels cannot create a utilization headline", () => {
  const data = executionFixture();
  const pool = data.metrics!.metrics.find(metric => metric.id === "datafusion_mem_pool_bytes")!;
  pool.samples.push({ ...pool.samples[0] });
  expect(executionPools(data, now)[0].utilization).toBeNull();
  pool.samples[0] = { ...pool.samples[0], labels: { state: "reserved" } };
  expect(executionPools(data, now)).toEqual([]);
});
test("an initially missing processing-node counter cannot establish a zero-OOM baseline", () => {
  const data = executionFixture();
  data.metrics!.sources.push({ ...data.metrics!.sources[0], pod: "second-compactor" });
  expect(makeExecutionBaseline(data, now)).toBeNull();
  data.metrics!.sources.pop();
  data.metrics!.container_health!.pods[0].current_state = "terminated";
  expect(makeExecutionBaseline(data, now)).toBeNull();
});
test("execution OOMs increase without implying a pod OOMKilled or restart", () => {
  const start = executionFixture(0), end = executionFixture(39);
  const baseline = makeExecutionBaseline(start, now);
  const change = executionOomDelta(end, baseline, now);
  expect(change.value).toBe(39);
  expect(change.reset).toBe(false);
  expect(end.metrics!.container_health!.pods[0].last_termination).toBeNull();
  expect(end.metrics!.container_health!.pods[0].restarts).toBe(0);
});
test("a counter reset or replacement process makes the interval unknown, not zero", () => {
  const baseline = makeExecutionBaseline(executionFixture(), now);
  expect(executionOomDelta(executionFixture(0), baseline, now)).toMatchObject({ value: null, reset: true });
  expect(executionOomDelta(executionFixture(40, "container-2"), baseline, now)).toMatchObject({ value: null, reset: true });
});
test("missing termination provenance prevents a misleading OOM baseline", () => {
  const data = executionFixture();
  data.metrics!.container_health = undefined;
  expect(makeExecutionBaseline(data, now)).toBeNull();
});
