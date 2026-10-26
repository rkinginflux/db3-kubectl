import { expect, test } from "vitest";
import { render, screen } from "@testing-library/react";
import { Topology } from "./Topology";
import type { NodeRow, PodRow, Snapshot, WorkloadRow, ServiceRow } from "../types";

const pod = (n: number): PodRow => ({ namespace: "db3", name: `pod-${n}`, owner_kind: "StatefulSet", owner_name: `db-${n}`, component: "ingester", cluster_id: `cluster-${n}`, node: null, ready: true, labels: { app: "db3" }, phase: "Running", restarts: 0, start_time: null, age: "1m", inferred_component: false, images: [], containers: [], mode: "ingest" });
const work = (n: number): WorkloadRow => ({ namespace: "db3", name: `db-${n}`, kind: "StatefulSet", component: "ingester", cluster_id: `cluster-${n}`, desired: 1, ready: 1, updated: 1, images: [], age: "1m" });
const service = (n: number): ServiceRow => ({ namespace: "db3", name: `svc-${n}`, type: "ClusterIP", cluster_ip: "10.0.0.1", external_ips: [], ports: [], selector: { app: "db3" }, endpoints: [], component: "ingester", cluster_id: null });
const node = (name: string): NodeRow => ({ name, ready: true, roles: ["control-plane"], version: "v1.34.1", os_image: "Ubuntu", kernel: "6.8", cpu_capacity: "8", mem_capacity_bytes: 1, cpu_allocatable: "8", mem_allocatable_bytes: 1, pods_capacity: 110, pods_used: 1, zone: "", internal_ip: "10.0.0.2", age: "1d", taints: [] });
const snapshot = (count: number) => ({ pods: Array.from({ length: count }, (_, n) => pod(n)), workloads: Array.from({ length: count }, (_, n) => work(n)), services: Array.from({ length: count }, (_, n) => service(n)), nodes: [] } as unknown as Snapshot);

test("same component is not evidence of pod ownership", () => {
  const data = snapshot(2);
  data.pods = [pod(0)];
  render(<Topology snapshot={data} />);
  expect(screen.queryByRole("button", { name: /db-1,/ })).not.toBeInTheDocument();
  expect(screen.getByRole("button", { name: /db-0,/ })).toBeInTheDocument();
});

test("every collected node is listed, including one running no database pod", () => {
  const data = snapshot(1);
  data.pods = [{ ...pod(0), node: "node-a" }];
  data.workloads = [work(0)];
  data.services = [];
  data.nodes = [node("node-a"), node("node-b")];
  render(<Topology snapshot={data} />);
  // node-b hosts nothing this view tracks, but it is a cluster member, and the
  // column is titled Nodes. It used to be dropped because the list was derived
  // from the capped pod set.
  expect(screen.getByRole("button", { name: /^node-a,/ })).toBeInTheDocument();
  expect(screen.getByRole("button", { name: /^node-b,/ })).toBeInTheDocument();
});

test("the cap applies after eligibility, not before", () => {
  const data = snapshot(100);
  // 50 pods carry no component and 50 do. Slicing to the cap first spent the
  // budget on ineligible rows, so only 30 qualifying pods survived.
  data.pods = Array.from({ length: 100 }, (_, n) => ({ ...pod(n), component: n < 50 ? null : "ingester" }));
  data.services = [];
  data.nodes = [];
  render(<Topology snapshot={data} />);
  expect(screen.getByText(/InfluxDB 3 workloads/).textContent).toMatch(/\(50\)/);
});

test("large graphs have explicit node edge and selector work budgets with partial notice", () => {
  const { container } = render(<Topology snapshot={snapshot(100)} />);
  expect(screen.getByText(/partial topology/i)).toBeInTheDocument();
  expect(container.querySelectorAll(".topo-node").length).toBeLessThanOrEqual(240);
  expect(container.querySelectorAll(".topo-edge").length).toBeLessThanOrEqual(500);
});

test("truncation reports what is shown against what exists", () => {
  const data = snapshot(100);
  data.nodes = Array.from({ length: 100 }, (_, n) => node(`node-${n}`));
  render(<Topology snapshot={data} />);
  const notice = screen.getByText(/partial topology/i).textContent ?? "";
  expect(notice).toMatch(/80 of 100 database pods/);
  expect(notice).toMatch(/80 of 100 services/);
  expect(notice).toMatch(/80 of 100 nodes/);
});

test("every collected service is listed, including one with no component", () => {
  const data = snapshot(1);
  data.services = [
    { ...service(0), name: "db3-ingest", component: "ingester" },
    { ...service(1), name: "kubernetes", component: null, selector: {} },
  ];
  render(<Topology snapshot={data} />);
  // The second service is not database-related; it used to be filtered out
  // entirely, which made the Services column under-report the cluster.
  expect(screen.getByRole("button", { name: /db3-ingest,/ })).toBeInTheDocument();
  expect(screen.getByRole("button", { name: /kubernetes,/ })).toBeInTheDocument();
});

test("when services exceed the cap, database services keep their place", () => {
  const data = snapshot(1);
  data.pods = [pod(0)];
  data.workloads = [work(0)];
  // 100 unrelated services ahead of 5 database ones; a plain cap would drop the
  // database services entirely.
  data.services = [
    ...Array.from({ length: 100 }, (_, n) => ({ ...service(n), name: `cluster-${n}`, component: null, selector: {} })),
    ...Array.from({ length: 5 }, (_, n) => ({ ...service(200 + n), name: `db3-${n}`, component: "ingester" })),
  ];
  render(<Topology snapshot={data} />);
  expect(screen.getByRole("button", { name: /db3-0,/ })).toBeInTheDocument();
  expect(screen.getByText(/partial topology/i).textContent).toMatch(/80 of 105 services/);
});
