import { beforeEach, expect, test, vi } from "vitest";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { App } from "./App";
import { DataTable } from "./ui";
import { ThemeProvider } from "../theme";
import type { Snapshot } from "../types";

const snapshot: Snapshot = {
  generated_at: "2026-10-26T00:00:00Z", tool_version: "test", context: "fixture", namespace_filter: null,
  cluster: { server: "https://fixture.invalid", version: "v1.35.4", platform: "linux/amd64" },
  nodes: [], namespaces: [], pods: [], workloads: [], services: [], storage: { classes: [], volumes: [], claims: [] },
  helm: [], configmaps: [], secrets: [], secret_values_exposed: false, network_policies: [], events: [], db3: [], issues: [],
  collectors: [{ name: "fixture-source", ok: false, error: "collector unavailable or forbidden", duration_ms: 1, count: 0 }],
};
beforeEach(() => {
  document.head.innerHTML = '<meta name="db3-session" content="react-session">';
  vi.stubGlobal("fetch", vi.fn().mockResolvedValue({ ok: true, json: async () => structuredClone(snapshot) }));
});
const mount = () => render(<ThemeProvider><App /></ThemeProvider>);

test("theme defaults dark, toggles by button and keyboard, and persists", async () => {
  mount();
  await screen.findByText("fixture-source");
  expect(document.documentElement.dataset.theme).toBe("dark");
  await userEvent.click(screen.getByRole("button", { name: "Switch to light theme" }));
  expect(localStorage.getItem("db3-webui-theme")).toBe("light");
  fireEvent.keyDown(window, { key: "t" });
  expect(document.documentElement.dataset.theme).toBe("dark");
});

test("all existing views remain navigable, including logs and derived DB details", async () => {
  mount();
  await screen.findByText("fixture-source");
  for (const [label, title] of [["Topology", "Topology"], ["InfluxDB 3", "InfluxDB 3"], ["Kubernetes", "Kubernetes"], ["Helm", "Helm"], ["Config & secrets", "Config & secrets"], ["Events", "Events"], ["Logs", "Logs"], ["Overview", "Overview"]]) {
    await userEvent.click(screen.getByRole("button", { name: label }));
    expect(screen.getByRole("heading", { name: title, level: 1 })).toBeInTheDocument();
  }
});

test("collector source failures are visible without hiding the snapshot", async () => {
  mount();
  expect(await screen.findByText("collector unavailable or forbidden")).toBeInTheDocument();
  expect(screen.getByText("fixture-source")).toBeInTheDocument();
  expect(screen.getByText("https://fixture.invalid")).toBeInTheDocument();
});

test("refresh uses authenticated API and preserves the established query shape", async () => {
  mount();
  await screen.findByText("fixture-source");
  await userEvent.click(screen.getByRole("button", { name: /Refresh$/ }));
  await waitFor(() => expect(fetch).toHaveBeenCalledTimes(2));
  expect(vi.mocked(fetch).mock.calls[1]).toEqual(["/api/snapshot?refresh=1", expect.objectContaining({ headers: expect.objectContaining({ "X-DB3-Session": "react-session" }) })]);
});

test("transport errors mark retained observations stale after disconnect", async () => {
  mount();
  await screen.findByText("fixture-source");
  vi.mocked(fetch).mockRejectedValueOnce(new Error("fixture server stopped"));
  await userEvent.click(screen.getByRole("button", { name: /Refresh$/ }));
  expect(await screen.findByText(/Disconnected.*cached observations are stale/)).toBeInTheDocument();
  expect(screen.getByText("fixture-source")).toBeInTheDocument();
});

test("transport errors are readable and a refresh recovers", async () => {
  vi.mocked(fetch).mockRejectedValueOnce(new Error("fixture offline"));
  mount();
  expect(await screen.findByText(/fixture offline/)).toBeInTheDocument();
  await userEvent.click(screen.getByRole("button", { name: /Refresh$/ }));
  expect(await screen.findByText("fixture-source")).toBeInTheDocument();
});

test("search filters real table rows and escapes untrusted data", async () => {
  const payload = '<img src=x onerror="alert(1)">';
  render(<DataTable rows={["alpha", payload]} columns={[{ key: "value", label: "Value", render: value => value }]} rowKey={value => value} searchText={value => value} />);
  expect(screen.getByText(payload)).toBeInTheDocument();
  expect(document.querySelector("img")).toBeNull();
  await userEvent.type(screen.getByRole("searchbox"), "alpha");
  expect(screen.getByText("alpha")).toBeInTheDocument();
  expect(screen.queryByText(payload)).not.toBeInTheDocument();
});

test("configuration accurately explains default no-Secrets and opaque value masking", async () => {
  mount();
  await screen.findByText("fixture-source");
  await userEvent.click(screen.getByRole("button", { name: "Config & secrets" }));
  expect(screen.getByText(/Secret collection is off by default/)).toBeInTheDocument();
  expect(screen.queryByText(/values never read/i)).not.toBeInTheDocument();
});

test("UI identifies registered trademarks and independent nonendorsement", async () => {
  mount();
  expect(await screen.findByText(/InfluxDB.*registered trademark/)).toBeInTheDocument();
  expect(screen.getByText(/not endorsed/i)).toBeInTheDocument();
});


test("Helm disabled status is explicit and partial observations do not assert health", async () => {
  mount();
  await screen.findByText("fixture-source");
  expect(screen.queryByText("Every collected object reported a healthy state.")).not.toBeInTheDocument();
  expect(screen.getByText(/No issues detected in available observations/)).toBeInTheDocument();
  await userEvent.click(screen.getByRole("button", { name: "Helm" }));
  expect(screen.getByText(/Helm disabled; enable --helm/)).toBeInTheDocument();
  expect(screen.queryByText(/No Helm releases reported/)).not.toBeInTheDocument();
});
