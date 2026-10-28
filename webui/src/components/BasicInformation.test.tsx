import { beforeEach, expect, test, vi } from "vitest";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { BasicInformation } from "./BasicInformation";
import type { Snapshot } from "../types";

const fetchInfo = vi.fn();
vi.mock("../api", () => ({ fetchInfo: (...args: unknown[]) => fetchInfo(...args) }));

const pod = {
  namespace: "db3",
  name: "db3-ingester-0",
  component: "ingester",
  ready: true,
  containers: [{ name: "influxdb3", image: "influxdb:3", ready: true, restarts: 0 }],
};

function snapshotWith(extra: Record<string, unknown> = {}): Snapshot {
  return { pods: [pod], info_enabled: true, ...extra } as unknown as Snapshot;
}

beforeEach(() => {
  fetchInfo.mockReset();
});

test("without --info the panel explains the flag instead of vanishing", () => {
  render(<BasicInformation snapshot={snapshotWith({ info_enabled: false })} />);
  expect(screen.getByText(/restart the console with/i)).toBeInTheDocument();
  expect(screen.getByText("--info")).toBeInTheDocument();
  expect(screen.queryByRole("button", { name: /read/i })).not.toBeInTheDocument();
  expect(fetchInfo).not.toHaveBeenCalled();
});

test("a snapshot from an older binary also reads as disabled", () => {
  render(<BasicInformation snapshot={snapshotWith({ info_enabled: undefined })} />);
  expect(screen.getByText(/restart the console with/i)).toBeInTheDocument();
});

test("read renders both commands as pretty JSON", async () => {
  fetchInfo.mockResolvedValue({
    enabled: true,
    namespace: "db3",
    pod: "db3-ingester-0",
    container: "influxdb3",
    nodes: { nodes: [{ node_id: "node-a", role: "ingest" }] },
    license: { license_type: "enterprise", expires: "2027-01-01" },
    errors: {},
  });

  render(<BasicInformation snapshot={snapshotWith()} />);
  await userEvent.click(screen.getByRole("button", { name: /read/i }));

  await waitFor(() => expect(fetchInfo).toHaveBeenCalledWith({
    namespace: "db3",
    pod: "db3-ingester-0",
    container: "influxdb3",
  }));

  expect(await screen.findByText("influxdb3 show nodes --format json")).toBeInTheDocument();
  expect(screen.getByText("influxdb3 show license info --format json")).toBeInTheDocument();

  // Pretty, not minified: the rendered block has indented lines.
  const blocks = document.querySelectorAll("pre.logs");
  expect(blocks).toHaveLength(2);
  expect(blocks[0].textContent).toContain('\n  "nodes": [');
  expect(blocks[1].textContent).toContain('"license_type": "enterprise"');
});

test("a failed command keeps its diagnostic visible instead of showing empty JSON", async () => {
  fetchInfo.mockResolvedValue({
    enabled: true,
    namespace: "db3",
    pod: "db3-ingester-0",
    container: null,
    nodes: { nodes: [] },
    license: null,
    errors: { "license-info": "command returned invalid JSON" },
  });

  render(<BasicInformation snapshot={snapshotWith()} />);
  await userEvent.click(screen.getByRole("button", { name: /read/i }));

  expect(await screen.findByText(/license-info: command returned invalid JSON/)).toBeInTheDocument();
  expect(screen.getByText("(no output)")).toBeInTheDocument();
});

test("a transport failure surfaces the server message", async () => {
  fetchInfo.mockRejectedValue(new Error("the Basic Information panel is disabled; restart the console with --info to enable it"));
  render(<BasicInformation snapshot={snapshotWith()} />);
  await userEvent.click(screen.getByRole("button", { name: /read/i }));
  expect(await screen.findByText(/panel is disabled/)).toBeInTheDocument();
});
