import { Badge, Card, Dot, Empty, Stat } from "./ui";
import { formatTime, relativeTime } from "../format";
import type { Snapshot } from "../types";
import type { ReactNode } from "react";

export function Overview({ snapshot }: { snapshot: Snapshot }) {
  const nodesReady = snapshot.nodes.filter((node) => node.ready).length;
  const db3Pods = snapshot.pods.filter((pod) => pod.component);
  const db3PodsReady = db3Pods.filter((pod) => pod.ready).length;
  const workloadsReady = snapshot.workloads.filter((workload) => workload.desired === workload.ready).length;
  const critical = snapshot.issues.filter((issue) => issue.severity === "critical").length;
  const warnings = snapshot.issues.filter((issue) => issue.severity === "warning").length;
  const failedCollectors = snapshot.collectors.filter((collector) => !collector.ok).length;
  const releases = new Set(snapshot.helm.map((release) => `${release.namespace}/${release.name}`));
  const namespaceCount = new Set(snapshot.pods.map((pod) => pod.namespace)).size;

  return (
    <>
      <div className="grid stats">
        <Stat
          label="Nodes ready"
          value={`${nodesReady}/${snapshot.nodes.length}`}
          note={snapshot.cluster.version ? `Kubernetes ${snapshot.cluster.version}` : "cluster version unavailable"}
          accent={nodesReady === snapshot.nodes.length && nodesReady > 0 ? "ok" : "warn"}
        />
        <Stat
          label="InfluxDB 3 clusters"
          value={snapshot.db3.length}
          note={db3Pods.length ? `${db3PodsReady}/${db3Pods.length} database pods ready` : "no database pods found"}
          accent={snapshot.db3.length ? "accent" : undefined}
        />
        <Stat
          label="Workloads"
          value={`${workloadsReady}/${snapshot.workloads.length}`}
          note="StatefulSets, Deployments, DaemonSets"
        />
        <Stat label="Helm releases" value={releases.size} note={snapshot.helm.length ? "across all namespaces" : "helm not available"} />
        <Stat label="Namespaces in use" value={namespaceCount} note={`${snapshot.pods.length} pods observed`} />
        <Stat
          label="Open findings"
          value={snapshot.issues.length}
          note={critical || warnings ? `${critical} critical · ${warnings} warning` : "no readiness problems detected"}
          accent={critical ? "danger" : warnings ? "warn" : "ok"}
        />
      </div>

      <div className="grid two">
        <Card title="Cluster" subtitle={snapshot.context ? `context ${snapshot.context}` : "kubectl default context"}>
          <dl className="kv">
            <dt>API server</dt>
            <dd className="mono">{snapshot.cluster.server || "unreachable"}</dd>
            <dt>Kubernetes</dt>
            <dd>{snapshot.cluster.version || "unknown"}</dd>
            <dt>Platform</dt>
            <dd className="mono">{snapshot.cluster.platform || "unknown"}</dd>
            <dt>Namespace scope</dt>
            <dd>{snapshot.namespace_filter || "all namespaces"}</dd>
            <dt>Snapshot taken</dt>
            <dd>
              {formatTime(snapshot.generated_at)} <span className="muted">({relativeTime(snapshot.generated_at)})</span>
            </dd>
            <dt>Collector</dt>
            <dd>
              db3-webui v{snapshot.tool_version} · read-only
              {failedCollectors > 0 && (
                <span style={{ marginLeft: 8 }}>
                  <Badge tone="warn">{failedCollectors} degraded</Badge>
                </span>
              )}
            </dd>
          </dl>
        </Card>

        <Card title="Findings" subtitle="Derived from live object status — not a substitute for alerting">
          {snapshot.issues.length === 0 ? (
            <Empty glyph="✓">Every collected object reported a healthy state.</Empty>
          ) : (
            <div className="stack">
              {snapshot.issues.slice(0, 8).map((issue, index) => (
                <div className={`issue ${issue.severity}`} key={`${issue.title}-${index}`}>
                  <span className="bar" />
                  <div className="body">
                    <div className="row between">
                      <span className="title">{issue.title}</span>
                      {issue.namespace && <span className="key">{issue.namespace}</span>}
                    </div>
                    <span className="detail">{issue.detail}</span>
                  </div>
                </div>
              ))}
              {snapshot.issues.length > 8 && (
                <span className="muted" style={{ fontSize: 12 }}>
                  +{snapshot.issues.length - 8} more in the findings list
                </span>
              )}
            </div>
          )}
        </Card>
      </div>

      <Card
        title="Database roles"
        subtitle="Discovered from pod labels and workload names in the collected namespaces"
        tight
      >
        {snapshot.db3.length === 0 ? (
          <Empty glyph="◍">No InfluxDB 3 pods were found. Check the namespace, context, and role labels.</Empty>
        ) : (
          <div className="card-body">
            <div className="grid three">
              {snapshot.db3.map((cluster) => (
                <div key={cluster.key} className="role-card">
                  <div className="row between">
                    <span className="role-name">{cluster.cluster_id}</span>
                    <Badge tone="accent">{cluster.namespace}</Badge>
                  </div>
                  {cluster.roles.map((role) => (
                    <div key={role.role} style={{ display: "grid", gap: 4 }}>
                      <div className="row between" style={{ fontSize: 12 }}>
                        <span style={{ textTransform: "capitalize" }}>{role.role}</span>
                        <span className="muted mono">
                          {role.ready}/{role.desired}
                        </span>
                      </div>
                      <div className={`meter ${role.ready === role.desired ? "" : role.ready === 0 ? "danger" : "warn"}`}>
                        <span style={{ width: `${role.desired ? (role.ready / role.desired) * 100 : 0}%` }} />
                      </div>
                    </div>
                  ))}
                  <div className="row" style={{ fontSize: 11.5, color: "var(--muted)" }}>
                    <Dot tone={cluster.roles.every((role) => role.ready === role.desired) ? "ok" : "warn"} />
                    {cluster.object_store ? `object store: ${cluster.object_store}` : "object store not declared"}
                  </div>
                </div>
              ))}
            </div>
          </div>
        )}
      </Card>

      <Card title="Collectors" subtitle="Each block is an independent read-only kubectl call">
        <div className="grid three">
          {snapshot.collectors.map((collector) => (
            <div key={collector.name} className="role-card" style={{ gap: 4 }}>
              <div className="row between">
                <span className="role-name mono" style={{ fontSize: 11.5 }}>
                  {collector.name}
                </span>
                <Badge tone={collector.ok ? "ok" : "danger"}>{collector.ok ? "ok" : "failed"}</Badge>
              </div>
              <span className="muted" style={{ fontSize: 11.5 }}>
                {collector.ok ? `${collector.count} objects · ${collector.duration_ms} ms` : collector.error || "unknown error"}
              </span>
            </div>
          ))}
        </div>
      </Card>
    </>
  );
}

export function InfoNote({ children }: { children: ReactNode }) {
  return (
    <p className="muted" style={{ fontSize: 12, margin: 0 }}>
      {children}
    </p>
  );
}
