import { expect, test } from "vitest";
import { render, screen } from "@testing-library/react";
import { Topology } from "./Topology";
import type { PodRow, Snapshot, WorkloadRow, ServiceRow } from "../types";

const pod = (n: number): PodRow => ({ namespace: "db3", name: `pod-${n}`, owner_kind: "StatefulSet", owner_name: `db-${n}`, component: "ingester", cluster_id: `cluster-${n}`, node: null, ready: true, labels: { app: "db3" }, phase: "Running", restarts: 0, start_time: null, age: "1m", inferred_component: false, images: [], containers: [], mode: "ingest" });
const work = (n: number): WorkloadRow => ({ namespace: "db3", name: `db-${n}`, kind: "StatefulSet", component: "ingester", cluster_id: `cluster-${n}`, desired: 1, ready: 1, updated: 1, images: [], age: "1m" });
const service = (n: number): ServiceRow => ({ namespace: "db3", name: `svc-${n}`, type: "ClusterIP", cluster_ip: "10.0.0.1", external_ips: [], ports: [], selector: { app: "db3" }, endpoints: [], component: "ingester", cluster_id: null });
const snapshot = (count: number) => ({ pods: Array.from({ length: count }, (_, n) => pod(n)), workloads: Array.from({ length: count }, (_, n) => work(n)), services: Array.from({ length: count }, (_, n) => service(n)), nodes: [] } as unknown as Snapshot);

test("same component is not evidence of pod ownership", () => {
  const data = snapshot(2);
  data.pods = [pod(0)];
  render(<Topology snapshot={data} />);
  expect(screen.queryByRole("button", { name: /db-1,/ })).not.toBeInTheDocument();
  expect(screen.getByRole("button", { name: /db-0,/ })).toBeInTheDocument();
});
test("large graphs have explicit node edge and selector work budgets with partial notice", () => {
  const { container } = render(<Topology snapshot={snapshot(100)} />);
  expect(screen.getByText(/partial topology/i)).toBeInTheDocument();
  expect(container.querySelectorAll(".topo-node").length).toBeLessThanOrEqual(240);
  expect(container.querySelectorAll(".topo-edge").length).toBeLessThanOrEqual(500);
});
