import { useMemo, useState } from "react";
import type { ReactNode } from "react";
import { Badge, Card, DataTable, Empty } from "./ui";
import type { Column } from "./ui";
import { formatBytes, roleOrder } from "../format";
import type { NodeRow, PodRow, Snapshot } from "../types";

// kube-system is cluster plumbing, not something the deployment runs, and on a
// real node it is the busiest namespace by far. It is omitted from the list and
// reported as a count so that "not there" is never confused with "not collected".
const OMITTED_NAMESPACE = "kube-system";
// The collector keeps at most this many rows per resource type (src/collect.rs).
const COLLECTOR_ROW_LIMIT = 500;

interface NodeEntry {
  node: NodeRow;
  /** Pods on this node with kube-system removed, database roles first. */
  pods: PodRow[];
  /** Pods omitted because they live in kube-system. */
  omitted: number;
  /** Distinct owner references among the listed pods. */
  workloads: number;
  notReady: number;
}

function Fact({ label, children }: { label: string; children: ReactNode }) {
  return (
    <div className="topo-fact">
      <span className="topo-fact-label">{label}</span>
      <span className="topo-fact-value">{children}</span>
    </div>
  );
}

/** Roll the per-node pod list up into the counts the sidebar and summary show. */
function nodeEntry(entry: { node: NodeRow; pods: PodRow[] }, omitted: number): NodeEntry {
  const owners = new Set<string>();
  for (const pod of entry.pods) {
    if (pod.owner_kind && pod.owner_name) owners.add(`${pod.namespace}/${pod.owner_kind}/${pod.owner_name}`);
  }
  return {
    node: entry.node,
    pods: entry.pods,
    omitted,
    workloads: owners.size,
    notReady: entry.pods.filter((pod) => !pod.ready).length,
  };
}

const podColumns: Column<PodRow>[] = [
  {
    key: "pod",
    label: "Pod",
    // The namespace rides along in the subtitle: a node list mixes namespaces,
    // and a separate column costs the width the image column needs to stay legible.
    render: (pod) => (
      <div style={{ display: "grid", gap: 1 }}>
        <span className="mono nowrap" style={{ fontSize: 11.5 }}>{pod.name}</span>
        <span className="muted nowrap" style={{ fontSize: 11 }}>
          {pod.namespace} · {pod.age || "—"}
        </span>
      </div>
    ),
    sortValue: (pod) => pod.name,
    width: "30%",
  },
  {
    key: "workload",
    label: "Workload",
    render: (pod) =>
      pod.owner_kind && pod.owner_name ? (
        <div style={{ display: "grid", gap: 1 }}>
          <span className="mono nowrap" style={{ fontSize: 11.5 }}>{pod.owner_name}</span>
          <span className="muted nowrap" style={{ fontSize: 11 }}>{pod.owner_kind}</span>
        </div>
      ) : (
        <span className="muted">bare pod</span>
      ),
    sortValue: (pod) => `${pod.owner_kind ?? ""}/${pod.owner_name ?? ""}`,
    width: "28%",
  },
  {
    key: "component",
    label: "Component",
    render: (pod) =>
      pod.component ? (
        <Badge tone="accent" title={pod.inferred_component ? "Inferred from label or name, not the image" : undefined}>
          {pod.component}
          {pod.inferred_component ? " ~" : ""}
        </Badge>
      ) : (
        <span className="muted">—</span>
      ),
    sortValue: (pod) => pod.component ?? "zzz",
  },
  {
    key: "status",
    label: "Status",
    render: (pod) => (
      <div className="row" style={{ gap: 6 }}>
        <Badge
          tone={pod.ready ? "ok" : pod.phase === "Pending" ? "warn" : "danger"}
          title={`Phase ${pod.phase || "unknown"}`}
        >
          {pod.ready ? "Ready" : pod.phase && pod.phase !== "Running" ? pod.phase : "NotReady"}
        </Badge>
        {pod.restarts > 0 && (
          <span className="muted nowrap" style={{ fontSize: 11 }}>
            {pod.restarts} restart{pod.restarts === 1 ? "" : "s"}
          </span>
        )}
      </div>
    ),
    sortValue: (pod) => (pod.ready ? 1 : 0) * 1000 - pod.restarts,
  },
];

export function Topology({ snapshot }: { snapshot: Snapshot }) {
  const [picked, setPicked] = useState<string | null>(null);

  const nodes = useMemo<NodeEntry[]>(() => {
    // Bucket once against the whole collected pod set (never against another
    // column's capped list), so a node cannot lose pods to a limit elsewhere.
    const byNode = new Map<string, PodRow[]>();
    for (const pod of snapshot.pods) {
      if (!pod.node) continue;
      const bucket = byNode.get(pod.node);
      if (bucket) bucket.push(pod);
      else byNode.set(pod.node, [pod]);
    }
    return [...snapshot.nodes]
      .sort((a, b) => a.name.localeCompare(b.name, undefined, { numeric: true }))
      .map((node) => {
        const all = byNode.get(node.name) ?? [];
        const pods = all
          .filter((pod) => pod.namespace !== OMITTED_NAMESPACE)
          .sort(
            (a, b) =>
              roleOrder(a.component ?? "zzz") - roleOrder(b.component ?? "zzz") ||
              a.namespace.localeCompare(b.namespace) ||
              a.name.localeCompare(b.name, undefined, { numeric: true }),
          );
        return nodeEntry({ node, pods }, all.length - pods.length);
      });
  }, [snapshot]);

  // The first node is shown until another is picked, so the panel is never an
  // empty dropdown with nothing to read.
  const activeName = picked !== null && nodes.some((entry) => entry.node.name === picked) ? picked : nodes[0]?.node.name ?? null;
  const active = nodes.find((entry) => entry.node.name === activeName) ?? null;

  const listed = nodes.reduce((sum, entry) => sum + entry.pods.length, 0);
  const unscheduled = snapshot.pods.filter((pod) => !pod.node).length;
  const capped = snapshot.pods.length >= COLLECTOR_ROW_LIMIT;

  return (
    <Card
      title="Node workloads"
      subtitle="Pick a node to list everything running on it. kube-system is omitted."
      actions={
        <span className="muted" style={{ fontSize: 11.5 }}>
          {nodes.length} {nodes.length === 1 ? "node" : "nodes"} · {listed} pods listed
        </span>
      }
    >
      {nodes.length === 0 ? (
        <Empty glyph="◇">
          No nodes were collected, so there is nothing to explore. Check the Collectors panel for the nodes error.
        </Empty>
      ) : (
        <div className="topo-layout">
          <aside className="topo-side">
            <label className="topo-label" htmlFor="topo-node">
              Node
            </label>
            <select
              id="topo-node"
              className="topo-select"
              value={activeName ?? ""}
              aria-describedby="topo-node-hint"
              onChange={(event) => setPicked(event.target.value)}
            >
              {nodes.map((entry) => (
                <option key={entry.node.name} value={entry.node.name}>
                  {entry.node.name} — {entry.pods.length} pod{entry.pods.length === 1 ? "" : "s"}
                  {entry.node.ready ? "" : " (NotReady)"}
                </option>
              ))}
            </select>
            <span id="topo-node-hint" className="muted" style={{ fontSize: 11, lineHeight: 1.5 }}>
              Every node in the cluster, including nodes running nothing in scope.
            </span>
            {active && (
              <div className="topo-facts">
                <Fact label="Status">
                  <Badge tone={active.node.ready ? "ok" : "danger"}>{active.node.ready ? "Ready" : "NotReady"}</Badge>
                </Fact>
                <Fact label="Roles">{active.node.roles.join(", ") || "worker"}</Fact>
                <Fact label="Address">{active.node.internal_ip || "—"}</Fact>
                <Fact label="Kubernetes">{active.node.version || "—"}</Fact>
                <Fact label="Pods">
                  {active.pods.length} listed · {active.node.pods_used}/{active.node.pods_capacity} on node
                </Fact>
                <Fact label="Workloads">{active.workloads}</Fact>
                <Fact label="Not ready">{active.notReady}</Fact>
                <Fact label="CPU">
                  {active.node.cpu_allocatable || "—"}
                  <span className="muted"> of {active.node.cpu_capacity || "—"}</span>
                </Fact>
                <Fact label="Memory">
                  {formatBytes(active.node.mem_allocatable_bytes)}
                  <span className="muted"> of {formatBytes(active.node.mem_capacity_bytes)}</span>
                </Fact>
                <Fact label="Zone">{active.node.zone || "—"}</Fact>
                <Fact label="Age">{active.node.age || "—"}</Fact>
              </div>
            )}
          </aside>

          <div className="topo-main">
            {active && (
              <div className="topo-summary">
                <strong>{active.node.name}</strong>
                <span className="muted">
                  {active.pods.length} pod{active.pods.length === 1 ? "" : "s"} · {active.workloads} workload
                  {active.workloads === 1 ? "" : "s"}
                  {active.notReady > 0 ? ` · ${active.notReady} not ready` : ""}
                  {active.omitted > 0
                    ? ` · ${active.omitted} kube-system pod${active.omitted === 1 ? "" : "s"} hidden`
                    : ""}
                </span>
                {capped && (
                  <span className="muted">
                    Collection caps pods at {COLLECTOR_ROW_LIMIT} rows, so a busy node may be missing some.
                  </span>
                )}
              </div>
            )}
            {active && active.pods.length === 0 ? (
              <Empty glyph="◇">Nothing runs on {active.node.name} outside kube-system.</Empty>
            ) : (
              <DataTable
                rows={active?.pods ?? []}
                columns={podColumns}
                rowKey={(pod) => `${pod.namespace}/${pod.name}`}
                searchText={(pod) =>
                  `${pod.name} ${pod.namespace} ${pod.owner_kind ?? ""} ${pod.owner_name ?? ""} ${pod.component ?? ""}`
                }
                searchPlaceholder="Filter pods…"
                emptyMessage="No pods match the filter."
              />
            )}
            {unscheduled > 0 && (
              <p className="muted" style={{ fontSize: 11, padding: "8px 12px 0" }}>
                {unscheduled} pod{unscheduled === 1 ? " is" : "s are"} not scheduled on any node and appear in no node
                list.
              </p>
            )}
          </div>
        </div>
      )}
    </Card>
  );
}
