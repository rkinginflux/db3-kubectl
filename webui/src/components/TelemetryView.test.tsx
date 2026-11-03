import { beforeEach, expect, test, vi } from "vitest";
import { fireEvent, render, screen } from "@testing-library/react";
import { TelemetryView } from "./TelemetryView";
import { ThemeProvider } from "../theme";
import { fetchTelemetry, fetchTelemetryHistory } from "../api";
import type { TelemetrySnapshot } from "../telemetry";

vi.mock("../api", () => ({ fetchTelemetry: vi.fn(), fetchTelemetryHistory: vi.fn() }));
vi.mock("./TelemetryChart", () => ({ TelemetryChart: () => <div role="img" aria-label="Fixture history chart" /> }));

function fixture(): TelemetrySnapshot {
  const now = new Date().toISOString();
  return { schema_version: "1.0", served_at: now, status: "partial", instance_id: "fixture", collection_errors: {},
    config: { observed_at: now, max_age_seconds: 900, fresh: true, status: "ok", namespace: "influxdb3",
      context: "lab", release: "example-db3", cluster_ids: ["test-cluster"], snapshot_id: "a".repeat(64), errors: [], nodes: [],
      pods: [{ pod: "example-db3-compactor-0", node: "node-1", image: "influxdb:3.12.1-enterprise", ready: true,
        restarts: 0, started_at: now, chart: "chart", resources: { limits: { memory: "1536Mi" } }, settings: [
          { option: "exec-mem-pool-size", value: "20%", source: "container_environment", state: "observed",
            documented_default: "20%", cli_option_supported: true, recognized_environment_names: [], configmap_or_spec_drift: false,
            container_environment: [], declared_environment: [] },
          { option: "num-io-threads", value: null, source: "unrecognized_environment_input", state: "unknown_or_suppressed",
            documented_default: null, cli_option_supported: false, recognized_environment_names: [], configmap_or_spec_drift: false,
            container_environment: [{ name: "INFLUXDB3_NUM_IO_THREADS", value: "1" }], declared_environment: [] },
        ] }] },
    metrics: { observed_at: now, max_age_seconds: 90, fresh: true, status: "partial",
      sources: [{ job: "node", health: "up", last_scrape: now, age_seconds: 0 }],
      metrics: [{ id: "node_cpu_percent", unit: "percent", status: "ok", samples: [{ labels: {}, value: 0, evaluated_at: now }] },
        { id: "compactor_slice_latency_p95_seconds", unit: "seconds", status: "non_finite", samples: [{ labels: {}, value: null, evaluated_at: now }] }] },
  };
}

beforeEach(() => {
  vi.mocked(fetchTelemetry).mockReset().mockResolvedValue(fixture());
  vi.mocked(fetchTelemetryHistory).mockReset().mockResolvedValue({ schema_version: "1.0", metric: "node_cpu_percent", unit: "percent",
    start: 1, end: 300, step: 30, series: [{ labels: {}, values: [[1, 0], [300, 1]] }], gaps_are_unknown: true, annotations_present: false });
});
const mount = () => render(<ThemeProvider><TelemetryView intervalSeconds={0} refreshTick={0} /></ThemeProvider>);

test("renders actual cluster identity and zero CPU without implying idle latency is zero", async () => {
  mount();
  expect(await screen.findByText("test-cluster")).toBeInTheDocument();
  expect(screen.getByText("0.0%")).toBeInTheDocument();
  expect(await screen.findByRole("img", { name: "Fixture history chart" })).toBeInTheDocument();
  fireEvent.click(screen.getByRole("tab", { name: "Signals & sources" }));
  expect(screen.getByText("Idle / not finite")).toBeInTheDocument();
});

test("configuration separates observed values, help defaults and unrecognized environment", async () => {
  mount(); await screen.findByText("test-cluster");
  fireEvent.click(screen.getByRole("tab", { name: "Running configuration" }));
  expect(screen.getByText("--exec-mem-pool-size")).toBeInTheDocument();
  expect(screen.getByText("Unrecognized env")).toBeInTheDocument();
  expect(screen.queryByText("\\u2014")).not.toBeInTheDocument();
  fireEvent.change(screen.getByRole("searchbox", { name: "Search startup settings" }), { target: { value: "NUM_IO" } });
  expect(screen.getByText("--num-io-threads")).toBeInTheDocument();
  expect(screen.queryByText("--exec-mem-pool-size")).not.toBeInTheDocument();
});

test("disabled provider is an explicit status rather than a fabricated healthy dashboard", async () => {
  vi.mocked(fetchTelemetry).mockRejectedValue(new Error("telemetry not configured"));
  mount();
  expect(await screen.findByRole("alert")).toHaveTextContent("telemetry not configured");
  expect(screen.getByText("Disconnected")).toBeInTheDocument();
  expect(fetchTelemetryHistory).not.toHaveBeenCalled();
});
