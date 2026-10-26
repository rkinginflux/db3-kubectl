import { Badge, Card, DataTable, Empty, Meter } from "./ui";
import type { Column } from "./ui";
import { formatTime, relativeTime, roleOrder, shortImage } from "../format";
import type { PodRow, Snapshot } from "../types";
import { useState } from "react";

export function Db3View({ snapshot }: { snapshot: Snapshot }) {
  const [namespace, setNamespace] = useState<string>("all");
  const clusters = snapshot.db3.filter((cluster) => namespace === "all" || cluster.namespace === namespace);
  const namespaces = [...new Set(snapshot.db3.map((cluster) => cluster.namespace))].sort();

  const pods = snapshot.pods.filter(
    (pod) => pod.component && (namespace === "all" || pod.namespace === namespace),
  );

  const columns: Column<PodRow>[] = [
    {
      key: "name",
      label: "Pod",
      render: (pod) => (
        <div style={{ display: "grid", gap: 1 }}>
          <span className="mono" style={{ fontSize: 11.5 }}>
            {pod.name}
          </span>
          <span className="muted" style={{ fontSize: 11 }}>
            {pod.owner_kind?.toLowerCase() ?? "pod"} {pod.owner_name ?? ""}
          </span>
        </div>
      ),
      sortValue: (pod) => pod.name,
      width: "24%",
    },
    {
      key: "role",
      label: "Role",
      render: (pod) => (
        <div className="row">
          <Badge tone="accent">
            {pod.component}
            {pod.inferred_component && <span title="inferred from the pod name">~</span>}
          </Badge>
          {pod.mode && <span className="key">{pod.mode}</span>}
        </div>
      ),
      sortValue: (pod) => roleOrder(pod.component ?? "zzz"),
      width: "16%",
    },
    {
      key: "status",
      label: "Status",
      render: (pod) => (
        <div className="row">
          <Badge tone={pod.ready ? "ok" : pod.phase === "Pending" ? "warn" : "danger"}>{pod.phase}</Badge>
          {pod.restarts > 0 && (
            <span className="muted" style={{ fontSize: 11 }}>
              {pod.restarts} restart{pod.restarts === 1 ? "" : "s"}
            </span>
          )}
        </div>
      ),
      sortValue: (pod) => (pod.ready ? 1 : 0),
    },
    {
      key: "node",
      label: "Node",
      render: (pod) => <span className="mono" style={{ fontSize: 11.5 }}>{pod.node ?? "unscheduled"}</span>,
      sortValue: (pod) => pod.node ?? "",
    },
    {
      key: "images",
      label: "Image",
      render: (pod) => (
        <div style={{ display: "grid", gap: 2 }}>
          {pod.images.slice(0, 2).map((image) => (
            <span key={image} className="mono truncate" style={{ fontSize: 11, maxWidth: 260 }} title={image}>
              {shortImage(image)}
            </span>
          ))}
        </div>
      ),
      sortValue: (pod) => pod.images[0] ?? "",
    },
    {
      key: "age",
      label: "Started",
      render: (pod) => (
        <span title={formatTime(pod.start_time)} className="nowrap">
          {relativeTime(pod.start_time)}
        </span>
      ),
      sortValue: (pod) => pod.start_time ?? "",
    },
  ];

  return (
    <>
      <div className="toolbar">
        <span className="muted" style={{ fontSize: 12 }}>
          Namespace
        </span>
        <select value={namespace} onChange={(event) => setNamespace(event.target.value)} aria-label="Filter database namespace">
          <option value="all">All namespaces ({snapshot.db3.length} clusters)</option>
          {namespaces.map((name) => (
            <option key={name} value={name}>
              {name}
            </option>
          ))}
        </select>
        <span className="chip">
          <b>{pods.filter((pod) => pod.ready).length}</b>/{pods.length} pods ready
        </span>
        <span className="chip">{snapshot.db3.filter((cluster) => cluster.namespace === namespace || namespace === "all").length} cluster{snapshot.db3.length === 1 ? "" : "s"} discovered</span>
      </div>

      {clusters.length === 0 ? (
        <Card title="InfluxDB 3 deployments">
          <Empty glyph="◍">No database deployment matched this namespace filter.</Empty>
        </Card>
      ) : (
        <div className="grid two">
          {clusters.map((cluster) => {
            const ready = cluster.roles.reduce((sum, role) => sum + role.ready, 0);
            const desired = cluster.roles.reduce((sum, role) => sum + (role.desired ?? 0), 0);
            return (
              <Card
                key={cluster.key}
                title={
                  <span className="row" style={{ gap: 8 }}>
                    {cluster.cluster_id}
                    <Badge tone="neutral">{cluster.namespace}</Badge>
                  </span>
                }
                subtitle={`${ready}/${cluster.roles.some((role) => role.desired === null) ? "?" : desired} replicas ready · ${cluster.pod_count} pods`}
              >
                <div className="stack">
                  <dl className="kv" style={{ gridTemplateColumns: "130px 1fr" }}>
                    <dt>Object store</dt>
                    <dd className="mono">{cluster.object_store ?? "not declared"}</dd>
                    <dt>Services</dt>
                    <dd>
                      {cluster.services.length ? (
                        <div className="key-list">
                          {cluster.services.map((service) => (
                            <span className="key" key={service}>
                              {service}
                            </span>
                          ))}
                        </div>
                      ) : (
                        <span className="muted">none matched</span>
                      )}
                    </dd>
                    <dt>Helm release</dt>
                    <dd>
                      {cluster.helm_releases.length ? (
                        <span className="mono">{cluster.helm_releases.join(", ")}</span>
                      ) : (
                        <span className="muted">Helm association not observed (collection may be disabled)</span>
                      )}
                    </dd>
                    <dt>License</dt>
                    <dd>
                      {cluster.license ? (
                        <span className="row">
                          <Badge tone={cluster.license.token_present ? "ok" : "warn"}>
                            {cluster.license.type ?? "unknown type"}
                          </Badge>
                          {cluster.license.email && <span className="muted">{cluster.license.email}</span>}
                          {!cluster.license.token_present && <span className="muted">no license key mounted</span>}
                        </span>
                      ) : (
                        <span className="muted">no license keys found in workload environment</span>
                      )}
                    </dd>
                  </dl>

                  <div className="sep" />
                  <div className="stack" style={{ gap: 10 }}>
                    {cluster.roles.map((role) => (
                      <div key={role.role} style={{ display: "grid", gap: 4 }}>
                        <div className="row between" style={{ fontSize: 12 }}>
                          <span style={{ textTransform: "capitalize", fontWeight: 600 }}>{role.role}</span>
                          <span className="muted mono" style={{ fontSize: 11 }}>
                            {role.ready}/{role.desired ?? "?"}
                            {role.mode ? ` · ${role.mode}` : ""}
                          </span>
                        </div>
                        {role.desired === null ? <span className="muted">Owner capacity unverified</span> : <Meter ready={role.ready} desired={role.desired} />}
                        <div className="row" style={{ gap: 6 }}>
                          {role.pods.map((pod) => (
                            <span
                              key={pod.name}
                              className="key"
                              title={`${pod.name} · ${pod.node ?? "unscheduled"} · ${pod.restarts} restarts`}
                              style={
                                pod.ready
                                  ? undefined
                                  : { borderColor: "color-mix(in srgb, var(--danger) 45%, transparent)", color: "var(--danger)" }
                              }
                            >
                              {pod.name}
                            </span>
                          ))}
                        </div>
                      </div>
                    ))}
                  </div>
                </div>
              </Card>
            );
          })}
        </div>
      )}

      <Card title="Database pods" subtitle="Containers, images, scheduling and restart counts" tight>
        <DataTable
          rows={pods}
          columns={columns}
          rowKey={(pod) => `${pod.namespace}/${pod.name}`}
          searchText={(pod) => `${pod.namespace} ${pod.name} ${pod.component} ${pod.node} ${pod.images.join(" ")}`}
          searchPlaceholder="Filter by pod, role, node, image…"
          initialSort={{ key: "role", direction: "asc" }}
          emptyMessage="No InfluxDB 3 pods in scope"
        />
      </Card>
    </>
  );
}
