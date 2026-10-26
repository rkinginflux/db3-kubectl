import { Badge, Card, DataTable, Empty } from "./ui";
import type { Column } from "./ui";
import { formatTime, relativeTime } from "../format";
import type { HelmReleaseRow, Snapshot } from "../types";

const statusTone = (status: string): "ok" | "warn" | "danger" | "neutral" => {
  const value = status.toLowerCase();
  if (value === "deployed") return "ok";
  if (value.includes("pending") || value.includes("uninstalling")) return "warn";
  if (value === "failed") return "danger";
  return "neutral";
};

export function HelmView({ snapshot }: { snapshot: Snapshot }) {
  const columns: Column<HelmReleaseRow>[] = [
    {
      key: "name",
      label: "Release",
      render: (release) => (
        <div style={{ display: "grid", gap: 1 }}>
          <span className="mono" style={{ fontSize: 11.5 }}>{release.name}</span>
          <span className="muted" style={{ fontSize: 11 }}>{release.namespace}</span>
        </div>
      ),
      sortValue: (release) => `${release.namespace}/${release.name}`,
    },
    {
      key: "chart",
      label: "Chart",
      render: (release) => <span className="mono" style={{ fontSize: 11.5 }}>{release.chart}</span>,
      sortValue: (release) => release.chart,
    },
    {
      key: "app",
      label: "App version",
      render: (release) => <span className="mono" style={{ fontSize: 11.5 }}>{release.app_version || "—"}</span>,
      sortValue: (release) => release.app_version,
    },
    {
      key: "status",
      label: "Status",
      render: (release) => <Badge tone={statusTone(release.status)}>{release.status}</Badge>,
      sortValue: (release) => release.status,
    },
    {
      key: "revision",
      label: "Revision",
      render: (release) => <span className="mono" style={{ fontSize: 11.5 }}>{release.revision}</span>,
      align: "right",
      sortValue: (release) => Number(release.revision) || 0,
    },
    {
      key: "updated",
      label: "Updated",
      render: (release) => (
        <span className="nowrap" title={formatTime(release.updated)}>
          {relativeTime(release.updated)}
        </span>
      ),
      sortValue: (release) => release.updated,
    },
  ];

  const degraded = snapshot.helm.filter((release) => release.status.toLowerCase() !== "deployed");
  const collectors = snapshot.collectors.filter((collector) => collector.name.startsWith("helm"));

  if (!snapshot.helm_enabled) {
    return <Card title="Helm releases" subtitle="Collection is opt-in">
      <Empty glyph="⎈">Helm disabled; enable --helm to collect releases. This explicitly allows Helm storage Secret reads using your operator credentials; the base RBAC grants no Secret reads.</Empty>
    </Card>;
  }

  return (
    <>
      <div className="toolbar">
        <span className="chip">
          <b>{snapshot.helm.length}</b> releases
        </span>
        <span className="chip">
          <b>{degraded.length}</b> not deployed
        </span>
        <span className="chip mono">helm list -A -o json</span>
      </div>

      {collectors.some((collector) => !collector.ok) && (
        <div className="error-banner">
          <code>helm</code> could not be read: {collectors.find((collector) => !collector.ok)?.error}. Releases managed outside Helm will
          not appear below.
        </div>
      )}

      <Card title="Helm releases" subtitle="Chart versions and revision state as reported by the local Helm client" tight>
        {snapshot.helm.length === 0 ? (
          <Empty glyph="⎈">No Helm releases reported. The deployment may be managed with plain manifests or Kustomize.</Empty>
        ) : (
          <DataTable
            rows={snapshot.helm}
            columns={columns}
            rowKey={(release) => `${release.namespace}/${release.name}`}
            searchText={(release) => `${release.namespace} ${release.name} ${release.chart} ${release.status}`}
            searchPlaceholder="Filter releases…"
            initialSort={{ key: "name", direction: "asc" }}
            emptyMessage="No Helm releases"
          />
        )}
      </Card>

      <Card title="What the console does with Helm data" subtitle="Read-only by design">
        <p className="muted" style={{ margin: 0, fontSize: 12.5 }}>
          The console runs <code>helm list -A -o json</code> with your current kubeconfig. Helm may read and decode Secret-backed release storage with your credentials, including stored values. The console does not expose those values or render charts. Chart pins and revision numbers are enough to compare a running deployment with the values file you
          keep in source control; use <code>helm get values</code> yourself when you need the exact overlay.
        </p>
      </Card>
    </>
  );
}
