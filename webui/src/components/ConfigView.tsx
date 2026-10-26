import { useMemo, useState } from "react";
import { Badge, Card, DataTable, Empty } from "./ui";
import type { Column } from "./ui";
import { formatBytes } from "../format";
import type { ConfigMapRow, SecretRow, Snapshot } from "../types";

function KeyTable({ data }: { data: Record<string, string> }) {
  const entries = Object.entries(data);
  if (entries.length === 0) return <div className="muted" style={{ padding: 12, fontSize: 12 }}>No inline keys</div>;
  return (
    <table className="cfg">
      <tbody>
        {entries.map(([key, value]) => (
          <tr key={key}>
            <td>{key}</td>
            <td>{value}</td>
          </tr>
        ))}
      </tbody>
    </table>
  );
}

export function ConfigView({ snapshot }: { snapshot: Snapshot }) {
  const [namespace, setNamespace] = useState("all");
  const namespaces = [...new Set([...snapshot.configmaps, ...snapshot.secrets].map((item) => item.namespace))].sort();

  const configmaps = snapshot.configmaps.filter((item) => namespace === "all" || item.namespace === namespace);
  const secrets = snapshot.secrets.filter((item) => namespace === "all" || item.namespace === namespace);

  const envRows = useMemo(
    () =>
      snapshot.pods.flatMap((pod) =>
        pod.containers.flatMap((container) =>
          container.env.map((env) => ({
            pod: pod.name,
            namespace: pod.namespace,
            container: container.name,
            key: env.key,
            value: env.value,
            redacted: env.redacted,
          })),
        ),
      ),
    [snapshot.pods],
  );

  const secretColumns: Column<SecretRow>[] = [
    {
      key: "name",
      label: "Secret",
      render: (row) => (
        <div style={{ display: "grid", gap: 1 }}>
          <span className="mono" style={{ fontSize: 11.5 }}>{row.name}</span>
          <span className="muted" style={{ fontSize: 11 }}>{row.namespace}</span>
        </div>
      ),
      sortValue: (row) => `${row.namespace}/${row.name}`,
    },
    {
      key: "type",
      label: "Type",
      render: (row) => <Badge tone="neutral">{row.type}</Badge>,
      sortValue: (row) => row.type,
    },
    {
      key: "keys",
      label: "Keys (values discarded)",
      render: (row) => (
        <div className="key-list">
          {row.keys.map((key) => (
            <span className="key secret" key={key}>
              {key}
            </span>
          ))}
          {row.keys.length === 0 && <span className="muted">—</span>}
        </div>
      ),
      sortable: false,
    },
    {
      key: "age",
      label: "Age",
      render: (row) => <span className="nowrap">{row.age}</span>,
      sortValue: (row) => row.age,
    },
  ];

  const envColumns: Column<(typeof envRows)[number]>[] = [
    {
      key: "key",
      label: "Variable",
      render: (row) => <span className="mono" style={{ fontSize: 11.5 }}>{row.key}</span>,
      sortValue: (row) => row.key,
    },
    {
      key: "value",
      label: "Value",
      render: (row) => (
        <div className="row">
          <span className="mono" style={{ fontSize: 11, wordBreak: "break-all" }}>{row.value ?? "—"}</span>
          {row.redacted && <Badge tone="warn">masked</Badge>}
        </div>
      ),
      sortable: false,
    },
    {
      key: "pod",
      label: "Pod",
      render: (row) => (
        <div style={{ display: "grid", gap: 1 }}>
          <span className="mono" style={{ fontSize: 11 }}>{row.pod}</span>
          <span className="muted" style={{ fontSize: 10.5 }}>{row.container}</span>
        </div>
      ),
      sortValue: (row) => `${row.namespace}/${row.pod}/${row.container}`,
    },
  ];

  return (
    <>
      <div className="toolbar">
        <select value={namespace} onChange={(event) => setNamespace(event.target.value)} aria-label="Filter configuration namespace">
          <option value="all">All namespaces</option>
          {namespaces.map((name) => (
            <option key={name} value={name}>
              {name}
            </option>
          ))}
        </select>
        <span className="chip">
          <b>{configmaps.length}</b> ConfigMaps
        </span>
        <span className="chip">
          <b>{secrets.length}</b> Secrets
        </span>
        <span className="chip">
          <b>{envRows.length}</b> env vars
        </span>
      </div>

      <div className="error-banner" style={{ borderColor: "color-mix(in srgb, var(--warn) 40%, transparent)", background: "var(--warn-soft)", color: "var(--warn)" }}>
        Secret collection is off by default. Opt in with --include-secret-metadata to show names and keys; Kubernetes still reads full Secret objects before values are discarded. All ConfigMap and environment values are masked before display.
      </div>

      <Card title="ConfigMaps" subtitle="Keys from the collected namespaces; opaque values hidden" tight>
        {configmaps.length === 0 ? (
          <Empty glyph="▤">No ConfigMaps in scope</Empty>
        ) : (
          <div className="card-body" style={{ display: "grid", gap: 8 }}>
            {configmaps.map((configmap: ConfigMapRow) => (
              <details className="disclosure" key={`${configmap.namespace}/${configmap.name}`}>
                <summary>
                  <span className="mono" style={{ fontSize: 11.5 }}>{configmap.name}</span>
                  <Badge tone="neutral">{configmap.namespace}</Badge>
                  <span className="muted" style={{ fontSize: 11 }}>
                    {configmap.keys.length} keys · {formatBytes(configmap.keys.reduce((sum, key) => sum + key.bytes, 0))}
                  </span>
                  <span style={{ flex: 1 }} />
                  <span className="muted" style={{ fontSize: 11 }}>{configmap.age}</span>
                </summary>
                <KeyTable data={configmap.data} />
              </details>
            ))}
          </div>
        )}
      </Card>

      <Card title="Secrets" subtitle="Names, types and key names only — data is dropped during collection" tight>
        <DataTable
          rows={secrets}
          columns={secretColumns}
          rowKey={(row) => `${row.namespace}/${row.name}`}
          searchText={(row) => `${row.namespace} ${row.name} ${row.type} ${row.keys.join(" ")}`}
          searchPlaceholder="Filter secrets…"
          emptyMessage="Secret collection disabled by default, or no Secrets observed in scope"
          pageSize={30}
        />
      </Card>

      <Card title="Container environment" subtitle="Collected env names; all inline and referenced values masked" tight>
        <DataTable
          rows={envRows}
          columns={envColumns}
          rowKey={(row) => `${row.namespace}/${row.pod}/${row.container}/${row.key}`}
          searchText={(row) => `${row.namespace} ${row.pod} ${row.key} ${row.value ?? ""}`}
          searchPlaceholder="Filter by variable, pod, value…"
          emptyMessage="No environment variables collected"
          pageSize={40}
          initialSort={{ key: "key", direction: "asc" }}
        />
      </Card>
    </>
  );
}
