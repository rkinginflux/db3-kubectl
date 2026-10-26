import { expect, test } from "vitest";
import { fireEvent, render, screen } from "@testing-library/react";
import { Topology } from "./Topology";
import type { NodeRow, PodRow, Snapshot } from "../types";

const node = (name: string, overrides: Partial<NodeRow> = {}): NodeRow => ({
  name,
  ready: true,
  roles: ["control-plane"],
  version: "v1.34.1",
  os_image: "Ubuntu 24.04",
  kernel: "6.8",
  cpu_capacity: "8",
  mem_capacity_bytes: 16 * 1024 ** 3,
  cpu_allocatable: "7800m",
  mem_allocatable_bytes: 15 * 1024 ** 3,
  pods_capacity: 110,
  pods_used: 1,
  zone: "us-east-1a",
  internal_ip: "10.0.0.2",
  age: "1d",
  taints: [],
  ...overrides,
});

const pod = (name: string, overrides: Partial<PodRow> = {}): PodRow => ({
  namespace: "db3",
  name,
  owner_kind: "StatefulSet",
  owner_name: `db-${name}`,
  component: "ingester",
  cluster_id: "cluster-1",
  node: "node-a",
  phase: "Running",
  ready: true,
  restarts: 0,
  start_time: null,
  age: "1m",
  inferred_component: false,
  images: ["ghcr.io/influxdata/influxdb:3.0"],
  containers: [],
  labels: {},
  mode: "ingest",
  ...overrides,
});

const snapshot = (pods: PodRow[], nodes: NodeRow[]): Snapshot =>
  ({ pods, nodes, workloads: [], services: [] } as unknown as Snapshot);

test("the selector lists every collected node, including one running nothing", () => {
  render(
    <Topology
      snapshot={snapshot([pod("ingest-0")], [node("node-b"), node("node-a"), node("node-c")])}
    />,
  );
  const select = screen.getByLabelText("Node") as HTMLSelectElement;
  expect(Array.from(select.options).map((option) => option.value)).toEqual(["node-a", "node-b", "node-c"]);
  // node-c hosts nothing; a node list derived from pods used to drop it.
  expect(select.options[2].textContent).toMatch(/node-c/);
});

test("the first node is listed without any interaction", () => {
  render(<Topology snapshot={snapshot([pod("ingest-0"), pod("other-0", { node: "node-b" })], [node("node-a"), node("node-b")])} />);
  // A dropdown with an empty panel reads as broken, so the first node is shown.
  expect(screen.getByText("ingest-0")).toBeInTheDocument();
  expect(screen.queryByText("other-0")).not.toBeInTheDocument();
});

test("picking a node lists everything it runs, kube-system omitted and counted", () => {
  const pods = [
    pod("ingest-0"),
    pod("metrics-abc", { namespace: "db3", owner_kind: "DaemonSet", owner_name: "telegraf", component: null }),
    pod("kube-proxy-x", { namespace: "kube-system", owner_kind: "DaemonSet", owner_name: "kube-proxy", component: null }),
    pod("coredns-y", { namespace: "kube-system", owner_kind: "Deployment", owner_name: "coredns", component: null }),
  ];
  render(<Topology snapshot={snapshot(pods, [node("node-a")])} />);
  expect(screen.getByText("ingest-0")).toBeInTheDocument();
  expect(screen.getByText("metrics-abc")).toBeInTheDocument();
  // kube-system is plumbing, not deployment workload: omitted from the list but
  // reported, so the omission is never mistaken for a collection failure.
  expect(screen.queryByText("kube-proxy-x")).not.toBeInTheDocument();
  expect(screen.queryByText("coredns-y")).not.toBeInTheDocument();
  expect(screen.getByText(/2 kube-system pods hidden/)).toBeInTheDocument();
});

test("selecting another node swaps the list", () => {
  const pods = [pod("ingest-0"), pod("querier-0", { node: "node-b", component: "querier", owner_name: "db-querier" })];
  render(<Topology snapshot={snapshot(pods, [node("node-a"), node("node-b")])} />);
  expect(screen.getByText("ingest-0")).toBeInTheDocument();
  fireEvent.change(screen.getByLabelText("Node"), { target: { value: "node-b" } });
  expect(screen.getByText("querier-0")).toBeInTheDocument();
  expect(screen.queryByText("ingest-0")).not.toBeInTheDocument();
});

test("a node running only kube-system pods says so instead of looking empty", () => {
  const pods = [
    pod("ingest-0"),
    pod("kube-proxy-x", { node: "node-b", namespace: "kube-system", owner_kind: "DaemonSet", owner_name: "kube-proxy", component: null }),
  ];
  render(<Topology snapshot={snapshot(pods, [node("node-a"), node("node-b")])} />);
  fireEvent.change(screen.getByLabelText("Node"), { target: { value: "node-b" } });
  expect(screen.getByText(/Nothing runs on node-b outside kube-system/)).toBeInTheDocument();
  expect(screen.getByText(/1 kube-system pod hidden/)).toBeInTheDocument();
});

test("no collected nodes renders an empty state pointing at the collectors", () => {
  render(<Topology snapshot={snapshot([pod("ingest-0", { node: null })], [])} />);
  expect(screen.getByText(/No nodes were collected/)).toBeInTheDocument();
});

test("pods scheduled on no node are reported rather than silently dropped", () => {
  const pods = [pod("ingest-0"), pod("pending-0", { node: null, ready: false, phase: "Pending" })];
  render(<Topology snapshot={snapshot(pods, [node("node-a")])} />);
  expect(screen.getByText(/1 pod is not scheduled on any node/)).toBeInTheDocument();
});

test("database roles are ranked ahead of other workloads on the same node", () => {
  const pods = [
    pod("aaa-cluster-svc", { namespace: "db3", owner_kind: "DaemonSet", owner_name: "calico-node", component: null }),
    pod("zzz-ingest-0", { component: "ingester" }),
  ];
  const { container } = render(<Topology snapshot={snapshot(pods, [node("node-a")])} />);
  const firstRow = container.querySelector("tbody tr");
  expect(firstRow?.textContent).toContain("zzz-ingest-0");
});
