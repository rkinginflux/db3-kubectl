import { beforeEach, expect, test, vi } from "vitest";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { QueryView } from "./QueryView";
import type { QueryResponse, Snapshot } from "../types";

const runQuery = vi.fn();
vi.mock("../api", () => ({ runQuery: (...args: unknown[]) => runQuery(...args) }));

const pod = {
  namespace: "db3",
  name: "db3-querier-0",
  owner_kind: "StatefulSet",
  owner_name: "db3-querier",
  component: "querier",
  cluster_id: "lab",
  node: null,
  ready: true,
  labels: {},
  phase: "Running",
  restarts: 0,
  start_time: null,
  age: "1m",
  inferred_component: false,
  images: [],
  containers: [{ name: "influxdb3", image: "influxdb:3", ready: true, restarts: 0 }],
  mode: "query",
};

const snapshot = { pods: [pod] } as unknown as Snapshot;

function response(overrides: Partial<QueryResponse> = {}): QueryResponse {
  return {
    namespace: "db3",
    pod: "db3-querier-0",
    container: "influxdb3",
    database: "mydb",
    language: "sql",
    statement: "SELECT 1",
    columns: ["time", "value"],
    rows: [
      { time: "2026-01-01T00:00:00Z", value: 1.5 },
      { time: "2026-01-01T01:00:00Z", value: 2.5 },
    ],
    row_count: 2,
    truncated: false,
    redacted: true,
    error: null,
    ...overrides,
  };
}

beforeEach(() => {
  runQuery.mockReset();
});

async function typeDatabase() {
  await userEvent.type(screen.getByLabelText(/database name/i), "mydb");
}

test("renders the returned rows and the redaction marker", async () => {
  runQuery.mockResolvedValue(response());
  render(<QueryView snapshot={snapshot} />);
  await typeDatabase();
  await userEvent.click(screen.getByRole("button", { name: /^run$/i }));
  await waitFor(() => expect(screen.getByText("2026-01-01T00:00:00Z")).toBeInTheDocument());
  expect(screen.getByText("2 rows")).toBeInTheDocument();
  expect(screen.getByText("token redacted")).toBeInTheDocument();
  expect(screen.getByRole("columnheader", { name: "value" })).toBeInTheDocument();
});

test("run stays disabled until a database is provided", async () => {
  render(<QueryView snapshot={snapshot} />);
  expect(screen.getByRole("button", { name: /^run$/i })).toBeDisabled();
  await typeDatabase();
  expect(screen.getByRole("button", { name: /^run$/i })).toBeEnabled();
});

test("shows the server rejection instead of pretending the query ran", async () => {
  runQuery.mockRejectedValue(new Error("the query terminal is disabled; restart the console with --query"));
  render(<QueryView snapshot={snapshot} />);
  await typeDatabase();
  await userEvent.click(screen.getByRole("button", { name: /^run$/i }));
  await waitFor(() => expect(screen.getByText(/query terminal is disabled/i)).toBeInTheDocument());
  expect(screen.queryByRole("table")).not.toBeInTheDocument();
});

test("shows a database error from a successful response", async () => {
  runQuery.mockResolvedValue(
    response({ columns: [], rows: [], row_count: 0, error: "query exited with status 3: unknown measurement" }),
  );
  render(<QueryView snapshot={snapshot} />);
  await typeDatabase();
  await userEvent.click(screen.getByRole("button", { name: /^run$/i }));
  await waitFor(() => expect(screen.getByText(/unknown measurement/i)).toBeInTheDocument());
});
