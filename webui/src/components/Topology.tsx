import { useMemo, useState } from "react";
import { Card, Empty } from "./ui";
import type { PodRow, ServiceRow, Snapshot, WorkloadRow } from "../types";
import { roleOrder } from "../format";

interface Box {
  id: string;
  x: number;
  y: number;
  width: number;
  height: number;
  title: string;
  subtitle: string;
  tone: "ok" | "warn" | "danger" | "neutral";
}

interface Edge {
  id: string;
  from: Box;
  to: Box;
  tone: "ok" | "danger" | "neutral";
  label?: string;
}

const BOX_W = 236;
const BOX_H = 48;
const ROW_GAP = 14;
const TOP = 46;
const COL_X = [24, 356, 688];
const VIEW_W = 948;

function truncate(value: string, length: number): string {
  return value.length > length ? `${value.slice(0, length - 1)}…` : value;
}

function spread(items: number[], minGap: number): number[] {
  const order = items.map((value, index) => ({ value, index })).sort((a, b) => a.value - b.value);
  const result = new Array<number>(items.length);
  let previous = -Infinity;
  for (const item of order) {
    const y = Math.max(item.value, previous + minGap);
    result[item.index] = y;
    previous = y;
  }
  return result;
}

// Ownership is an exact match on namespace, owner kind and owner name, so it can
// be resolved through a key set rather than a budgeted cross product.
const podOwnerKey = (pod: PodRow) => `${pod.namespace}\u0000${pod.owner_kind}\u0000${pod.owner_name}`;
const workloadOwnerKey = (workload: WorkloadRow) => `${workload.namespace}\u0000${workload.kind}\u0000${workload.name}`;

export function Topology({ snapshot }: { snapshot: Snapshot }) {
  const [selected, setSelected] = useState<string | null>(null);

  const layout = useMemo(() => {
    const LIMIT = 80; // At most 240 rendered boxes across the three columns.
    const WORK = 20_000;
    let comparisons = 0;
    let partial = false;
    const spend = (cost = 1) => {
      comparisons += cost;
      if (comparisons > WORK) { partial = true; return false; }
      return true;
    };
    const owns = (pod: PodRow, workload: WorkloadRow) => spend() && pod.namespace === workload.namespace && pod.owner_kind === workload.kind && pod.owner_name === workload.name;
    const selectorMatches = (pod: PodRow, service: ServiceRow) => spend(Object.keys(service.selector).length + 1) && matchesSelector(pod, service);

    // Eligibility is decided before the cap. Slicing first let rows that do not
    // qualify consume the budget and silently dropped qualifying rows past the
    // cap, which could hide a workload and the node it runs on.
    const eligiblePods = snapshot.pods.filter((pod) => pod.component);
    const pods = eligiblePods.slice(0, LIMIT);

    const podOwners = new Set(pods.map(podOwnerKey));
    const knownOwners = new Set(snapshot.workloads.map(workloadOwnerKey));
    const matchedWorkloads = snapshot.workloads.filter((workload) => podOwners.has(workloadOwnerKey(workload)));
    const workloads = matchedWorkloads.slice(0, LIMIT);
    // ReplicaSet -> Deployment ancestry is not collected. Leave those pods
    // unmapped rather than invent ownership from a shared component label.
    if (pods.some((pod) => !knownOwners.has(podOwnerKey(pod)))) partial = true;

    const eligibleServices = snapshot.services.filter(
      (service) => service.component || service.selector["app.kubernetes.io/component"],
    );
    const services = eligibleServices.slice(0, LIMIT);

    // Every collected node is listed, capped only by LIMIT. Deriving this column
    // from the capped pod list made a node disappear whenever the pods running on
    // it fell outside the cap, so the Nodes column under-reported the cluster.
    const nodes = snapshot.nodes.slice(0, LIMIT);

    const truncated: string[] = [];
    const noteCap = (label: string, shown: number, total: number) => {
      if (shown < total) truncated.push(`${shown} of ${total} ${label}`);
    };
    noteCap("database pods", pods.length, eligiblePods.length);
    // Workloads are derived from the displayed pods, so they can never exceed the
    // pod cap; only pods, services and nodes can be truncated independently.
    noteCap("services", services.length, eligibleServices.length);
    noteCap("nodes", nodes.length, snapshot.nodes.length);
    if (truncated.length > 0) partial = true;

    // ---------------------------------------------------------------- nodes
    const nodeBoxes: Box[] = nodes.map((node, index) => ({
      id: `node:${node.name}`,
      x: COL_X[0],
      y: TOP + index * (BOX_H + ROW_GAP),
      width: BOX_W,
      height: BOX_H,
      title: truncate(node.name, 28),
      subtitle: `${node.ready ? "Ready" : "NotReady"} · ${node.roles.join(",") || "worker"}${node.zone ? ` · ${node.zone}` : ""}`,
      tone: node.ready ? "ok" : "danger",
    }));
    const nodeY = new Map(nodeBoxes.map((box) => [box.id.replace(/^node:/, ""), box.y]));

    // ------------------------------------------------------------ workloads
    const desiredWorkloadY = workloads.map((workload) => {
      const related = pods.filter((pod) => owns(pod, workload));
      const values = related.map((pod) => nodeY.get(pod.node ?? "") ?? TOP);
      if (values.length === 0) return TOP;
      const mean = values.reduce((sum, value) => sum + value, 0) / values.length;
      return mean - BOX_H / 2 + 20;
    });
    const workY = spread(desiredWorkloadY, BOX_H + ROW_GAP);
    const workBoxes: Box[] = workloads.map((workload, index) => ({
      id: `work:${workload.namespace}/${workload.name}`,
      x: COL_X[1],
      y: workY[index],
      width: BOX_W,
      height: BOX_H,
      title: truncate(workload.name, 26),
      subtitle: `${workload.component ?? workload.kind.toLowerCase()} · ${workload.ready}/${workload.desired} · ${workload.namespace}`,
      tone: workload.desired === 0 ? "neutral" : workload.ready === workload.desired ? "ok" : workload.ready === 0 ? "danger" : "warn",
    }));

    // ------------------------------------------------------------- services
    const desiredServiceY = services.map((service) => {
      const matches = workloads.filter((workload) =>
        pods.some(
          (pod) =>
            owns(pod, workload) && selectorMatches(pod, service),
        ),
      );
      const boxes = workBoxes.filter((box) =>
        matches.some((workload) => box.id === `work:${workload.namespace}/${workload.name}`),
      );
      if (boxes.length === 0) return TOP;
      const mean = boxes.reduce((sum, box) => sum + box.y, 0) / boxes.length;
      return mean;
    });
    const serviceY = spread(desiredServiceY, BOX_H + ROW_GAP);
    const serviceBoxes: Box[] = services.map((service, index) => ({
      id: `svc:${service.namespace}/${service.name}`,
      x: COL_X[2],
      y: serviceY[index],
      width: BOX_W,
      height: BOX_H,
      title: truncate(service.name, 26),
      subtitle: `${service.type} · ${service.ports.map((port) => port.port).filter((port) => port !== null).join(",") || "no port"} · ${
        service.endpoints.filter((endpoint) => endpoint.ready).length
      } ready`,
      tone: service.endpoints.some((endpoint) => endpoint.ready) ? "ok" : "warn",
    }));

    // ---------------------------------------------------------------- edges
    const edges: Edge[] = [];
    const addEdge = (edge: Edge) => {
      if (edges.length >= 500) { partial = true; return; }
      edges.push(edge);
    };
    for (const workload of workloads) {
      const box = workBoxes.find((item) => item.id === `work:${workload.namespace}/${workload.name}`);
      if (!box) continue;
      const relatedPods = pods.filter(
        (pod) => owns(pod, workload),
      );
      const seen = new Set<string>();
      for (const pod of relatedPods) {
        if (!pod.node || seen.has(pod.node)) continue;
        seen.add(pod.node);
        const node = nodeBoxes.find((item) => item.id === `node:${pod.node}`);
        if (!node) continue;
        addEdge({
          id: `e:${node.id}->${box.id}`,
          from: node,
          to: box,
          tone: pod.ready ? "ok" : "danger",
        });
      }
    }
    for (const service of services) {
      const box = serviceBoxes.find((item) => item.id === `svc:${service.namespace}/${service.name}`);
      if (!box) continue;
      for (const workload of workloads) {
        const workBox = workBoxes.find((item) => item.id === `work:${workload.namespace}/${workload.name}`);
        if (!workBox) continue;
        const related = pods.filter(
          (pod) => owns(pod, workload) && selectorMatches(pod, service),
        );
        if (related.length === 0) continue;
        addEdge({
          id: `e:${workBox.id}->${box.id}`,
          from: workBox,
          to: box,
          tone: related.some((pod) => pod.ready) ? "ok" : "danger",
        });
      }
    }

    const lowest = Math.max(0, ...[...nodeBoxes, ...workBoxes, ...serviceBoxes].map((box) => box.y + box.height));
    return {
      partial,
      truncated,
      nodeBoxes,
      workBoxes,
      serviceBoxes,
      edges,
      height: Math.max(220, lowest + 28),
      pods,
      workloads,
      services,
    };
  }, [snapshot]);

  const columns = [
    { title: "Nodes", count: layout.nodeBoxes.length },
    { title: "InfluxDB 3 workloads", count: layout.workBoxes.length },
    { title: "Services", count: layout.serviceBoxes.length },
  ];

  const connected = useMemo(() => {
    if (!selected) return null;
    const ids = new Set<string>([selected]);
    for (const edge of layout.edges) {
      if (edge.from.id === selected) ids.add(edge.to.id);
      if (edge.to.id === selected) ids.add(edge.from.id);
    }
    return ids;
  }, [selected, layout.edges]);

  const dim = (id: string) => connected !== null && !connected.has(id);

  return (
    <Card
      title="Cluster topology"
      subtitle="Nodes → database workloads → Services. Edges are derived from pod scheduling and Service selectors."
      actions={
        selected ? (
          <button type="button" className="btn small ghost" onClick={() => setSelected(null)}>
            Clear selection
          </button>
        ) : (
          <span className="muted" style={{ fontSize: 11.5 }}>
            Click a box to trace its links
          </span>
        )
      }
    >
      {layout.partial && (
        <p role="status" className="muted">
          Partial topology{layout.truncated.length > 0 ? `: showing ${layout.truncated.join(", ")}` : ""}. Only observed owner references are linked; missing ancestry is not inferred.
        </p>
      )}
      {layout.workBoxes.length === 0 ? (
        <Empty glyph="◇">No database workloads matched the collected namespaces.</Empty>
      ) : (
        <>
          <div className="topo-wrap">
            <svg viewBox={`0 0 ${VIEW_W} ${layout.height}`} width="100%" role="img" aria-label="InfluxDB 3 cluster topology">
              {columns.map((column, index) => (
                <text key={column.title} className="topo-col-title" x={COL_X[index]} y={20}>
                  {column.title} ({column.count})
                </text>
              ))}
              {layout.edges.map((edge) => {
                const x1 = edge.from.x + edge.from.width;
                const y1 = edge.from.y + edge.from.height / 2;
                const x2 = edge.to.x;
                const y2 = edge.to.y + edge.to.height / 2;
                const mid = (x1 + x2) / 2;
                const dimmed = selected ? !(edge.from.id === selected || edge.to.id === selected) : false;
                return (
                  <path
                    key={edge.id}
                    className={`topo-edge ${edge.tone}`}
                    d={`M ${x1} ${y1} C ${mid} ${y1}, ${mid} ${y2}, ${x2} ${y2}`}
                    style={{ opacity: dimmed ? 0.12 : 0.9 }}
                  />
                );
              })}
              {[...layout.nodeBoxes, ...layout.workBoxes, ...layout.serviceBoxes].map((box) => (
                <g
                  key={box.id}
                  className="topo-node"
                  style={{ cursor: "pointer", opacity: dim(box.id) ? 0.35 : 1 }}
                  onClick={() => setSelected((current) => (current === box.id ? null : box.id))}
                  tabIndex={0}
                  role="button"
                  aria-label={`${box.title}, ${box.subtitle}`}
                  onKeyDown={(event) => {
                    if (event.key === "Enter" || event.key === " ") {
                      event.preventDefault();
                      setSelected((current) => (current === box.id ? null : box.id));
                    }
                  }}
                >
                  <rect
                    className={`tone-${box.tone}`}
                    x={box.x}
                    y={box.y}
                    width={box.width}
                    height={box.height}
                    rx={9}
                    fill="var(--panel-raised)"
                    stroke={selected === box.id ? "var(--accent)" : undefined}
                    strokeWidth={selected === box.id ? 2 : 1}
                  />
                  <circle cx={box.x + 14} cy={box.y + 16} r={3.5} fill={`var(--${box.tone === "neutral" ? "muted" : box.tone})`} />
                  <text className="name" x={box.x + 24} y={box.y + 20}>
                    {box.title}
                  </text>
                  <text className="sub" x={box.x + 14} y={box.y + 37}>
                    {box.subtitle}
                  </text>
                </g>
              ))}
            </svg>
          </div>
          <div className="topo-legend">
            <span>
              <span className="dot ok" /> ready path
            </span>
            <span>
              <span className="dot danger" /> not ready
            </span>
            <span>Edges: pod scheduling (left) and Service selector matches (right)</span>
          </div>
        </>
      )}
    </Card>
  );
}

function matchesSelector(pod: PodRow, service: ServiceRow): boolean {
  const selector = service.selector;
  if (!selector || Object.keys(selector).length === 0) return false;
  if (pod.namespace !== service.namespace) return false;
  return Object.entries(selector).every(([key, value]) => pod.labels[key] === value);
}

export function workloadRowKey(workload: WorkloadRow): string {
  return `${workload.namespace}/${workload.kind}/${workload.name}`;
}

export function podSortKey(pod: PodRow): string {
  return `${roleOrder(pod.component ?? "zzz")}-${pod.namespace}/${pod.name}`;
}
