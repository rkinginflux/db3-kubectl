import { Badge, Card, DataTable, Empty, Stat } from "./ui";
import type { Column } from "./ui";
import { formatBytes, shortImage } from "../format";
import type { ClaimRow, NetworkPolicyRow, NodeRow, ServiceRow, Snapshot, WorkloadRow } from "../types";

export function KubernetesView({ snapshot }: { snapshot: Snapshot }) {
  const nodeColumns: Column<NodeRow>[] = [
    {
      key: "name",
      label: "Node",
      render: (node) => (
        <div style={{ display: "grid", gap: 1 }}>
          <span className="mono" style={{ fontSize: 11.5 }}>{node.name}</span>
          <span className="muted" style={{ fontSize: 11 }}>
            {node.roles.join(", ") || "worker"} · {node.internal_ip || "no IP"}
          </span>
        </div>
      ),
      sortValue: (node) => node.name,
    },
    {
      key: "status",
      label: "Status",
      render: (node) => (
        <div className="row">
          <Badge tone={node.ready ? "ok" : "danger"}>{node.ready ? "Ready" : "NotReady"}</Badge>
          {node.taints.map((taint) => (
            <span
              className="key"
              key={taint}
              title={taint}
              style={{ maxWidth: 130, overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}
            >
              {taint}
            </span>
          ))}
        </div>
      ),
      sortValue: (node) => (node.ready ? 1 : 0),
    },
    {
      key: "version",
      label: "Version",
      render: (node) => <span className="mono" style={{ fontSize: 11.5 }}>{node.version}</span>,
      sortValue: (node) => node.version,
    },
    {
      key: "cpu",
      label: "CPU",
      render: (node) => (
        <span className="mono nowrap" style={{ fontSize: 11.5 }}>
          {node.cpu_allocatable}
          <span className="muted"> / {node.cpu_capacity}</span>
        </span>
      ),
      align: "right",
      sortValue: (node) => Number.parseFloat(node.cpu_capacity) || 0,
    },
    {
      key: "memory",
      label: "Memory",
      render: (node) => (
        <span className="mono nowrap" style={{ fontSize: 11.5 }}>
          {formatBytes(node.mem_allocatable_bytes)}
          <span className="muted"> / {formatBytes(node.mem_capacity_bytes)}</span>
        </span>
      ),
      align: "right",
      sortValue: (node) => node.mem_capacity_bytes,
    },
    {
      key: "pods",
      label: "Pods",
      render: (node) => (
        <span className="mono" style={{ fontSize: 11.5 }}>
          {node.pods_used}/{node.pods_capacity}
        </span>
      ),
      align: "right",
      sortValue: (node) => node.pods_used,
    },
    {
      key: "zone",
      label: "Zone / OS",
      render: (node) => (
        <div style={{ display: "grid", gap: 1 }}>
          <span style={{ fontSize: 11.5 }}>{node.zone || "—"}</span>
          <span className="muted truncate" style={{ fontSize: 11, maxWidth: 240 }} title={node.os_image}>
            {node.os_image}
          </span>
        </div>
      ),
      sortValue: (node) => node.zone,
    },
    {
      key: "age",
      label: "Age",
      render: (node) => <span className="nowrap">{node.age}</span>,
      sortValue: (node) => node.age,
    },
  ];

  const workloadColumns: Column<WorkloadRow>[] = [
    {
      key: "name",
      label: "Workload",
      render: (workload) => (
        <div style={{ display: "grid", gap: 1 }}>
          <span className="mono" style={{ fontSize: 11.5 }}>{workload.name}</span>
          <span className="muted" style={{ fontSize: 11 }}>
            {workload.namespace} · {workload.kind}
          </span>
        </div>
      ),
      sortValue: (workload) => `${workload.namespace}/${workload.name}`,
    },
    {
      key: "role",
      label: "Component",
      render: (workload) =>
        workload.component ? <Badge tone="accent">{workload.component}</Badge> : <span className="muted">—</span>,
      sortValue: (workload) => workload.component ?? "",
    },
    {
      key: "replicas",
      label: "Replicas",
      render: (workload) => (
        <div className="row" style={{ justifyContent: "flex-end" }}>
          <Badge tone={workload.ready === workload.desired ? "ok" : workload.ready === 0 ? "danger" : "warn"}>
            {workload.ready}/{workload.desired}
          </Badge>
        </div>
      ),
      align: "right",
      sortValue: (workload) => workload.ready - workload.desired,
    },
    {
      key: "image",
      label: "Image",
      render: (workload) => (
        <span className="mono truncate" style={{ fontSize: 11, maxWidth: 300 }} title={workload.images[0]}>
          {shortImage(workload.images[0] ?? "")}
        </span>
      ),
      sortValue: (workload) => workload.images[0] ?? "",
    },
    {
      key: "age",
      label: "Updated",
      render: (workload) => <span className="nowrap">{workload.age}</span>,
      sortValue: (workload) => workload.updated,
    },
  ];

  const serviceColumns: Column<ServiceRow>[] = [
    {
      key: "name",
      label: "Service",
      render: (service) => (
        <div style={{ display: "grid", gap: 1 }}>
          <span className="mono" style={{ fontSize: 11.5 }}>{service.name}</span>
          <span className="muted" style={{ fontSize: 11 }}>{service.namespace}</span>
        </div>
      ),
      sortValue: (service) => `${service.namespace}/${service.name}`,
    },
    {
      key: "type",
      label: "Type",
      render: (service) => <Badge tone="neutral">{service.type}</Badge>,
      sortValue: (service) => service.type,
    },
    {
      key: "port",
      label: "Ports",
      render: (service) => (
        <div className="row" style={{ gap: 4 }}>
          {service.ports.length === 0 && <span className="muted">—</span>}
          {service.ports.map((port, index) => (
            <span className="key" key={`${port.name}-${index}`}>
              {port.port ?? "?"}→{port.target_port}/{port.protocol}
            </span>
          ))}
        </div>
      ),
      sortValue: (service) => service.ports[0]?.port ?? 0,
    },
    {
      key: "endpoints",
      label: "Endpoints",
      render: (service) => {
        const ready = service.endpoints.filter((endpoint) => endpoint.ready).length;
        return (
          <span className="row" style={{ justifyContent: "flex-end" }}>
            <Badge tone={ready ? "ok" : "warn"}>{ready} ready</Badge>
            <span className="muted" style={{ fontSize: 11 }}>
              {service.endpoints.length} total
            </span>
          </span>
        );
      },
      align: "right",
      sortValue: (service) => service.endpoints.filter((endpoint) => endpoint.ready).length,
    },
    {
      key: "selector",
      label: "Selector",
      render: (service) => (
        <span className="mono truncate" style={{ fontSize: 10.5, maxWidth: 320 }} title={JSON.stringify(service.selector)}>
          {Object.entries(service.selector)
            .map(([key, value]) => `${key}=${value}`)
            .join(", ") || "—"}
        </span>
      ),
      sortable: false,
    },
  ];

  const claimColumns: Column<ClaimRow>[] = [
    {
      key: "name",
      label: "Claim",
      render: (claim) => (
        <div style={{ display: "grid", gap: 1 }}>
          <span className="mono" style={{ fontSize: 11.5 }}>{claim.name}</span>
          <span className="muted" style={{ fontSize: 11 }}>{claim.namespace}</span>
        </div>
      ),
      sortValue: (claim) => `${claim.namespace}/${claim.name}`,
    },
    {
      key: "status",
      label: "Phase",
      render: (claim) => <Badge tone={claim.phase === "Bound" ? "ok" : "warn"}>{claim.phase}</Badge>,
      sortValue: (claim) => claim.phase,
    },
    {
      key: "class",
      label: "Storage class",
      render: (claim) => <span className="mono" style={{ fontSize: 11.5 }}>{claim.storage_class || "—"}</span>,
      sortValue: (claim) => claim.storage_class,
    },
    {
      key: "capacity",
      label: "Capacity",
      render: (claim) => formatBytes(claim.capacity_bytes),
      align: "right",
      sortValue: (claim) => claim.capacity_bytes,
    },
    {
      key: "volume",
      label: "Volume",
      render: (claim) => <span className="mono" style={{ fontSize: 11 }}>{claim.volume || "—"}</span>,
      sortValue: (claim) => claim.volume,
    },
  ];

  const netpolColumns: Column<NetworkPolicyRow>[] = [
    {
      key: "name",
      label: "Policy",
      render: (policy) => (
        <div style={{ display: "grid", gap: 1 }}>
          <span className="mono" style={{ fontSize: 11.5 }}>{policy.name}</span>
          <span className="muted" style={{ fontSize: 11 }}>{policy.namespace}</span>
        </div>
      ),
      sortValue: (policy) => `${policy.namespace}/${policy.name}`,
    },
    {
      key: "types",
      label: "Types",
      render: (policy) => (
        <div className="row" style={{ gap: 4 }}>
          {policy.policy_types.map((type) => (
            <Badge key={type} tone="info">
              {type}
            </Badge>
          ))}
        </div>
      ),
      sortValue: (policy) => policy.policy_types.join(","),
    },
    {
      key: "pod",
      label: "Pod selector",
      render: (policy) => <span className="mono truncate" style={{ fontSize: 10.5, maxWidth: 340 }}>{policy.pod_selector}</span>,
      sortable: false,
    },
    {
      key: "rules",
      label: "Rules",
      render: (policy) => (
        <span className="muted" style={{ fontSize: 11.5 }}>
          {policy.ingress_rules} ingress · {policy.egress_rules} egress
        </span>
      ),
      align: "right",
      sortValue: (policy) => policy.ingress_rules + policy.egress_rules,
    },
  ];

  const notReady = snapshot.pods.filter((pod) => !pod.ready).length;
  const pendingClaims = snapshot.storage.claims.filter((claim) => claim.phase !== "Bound").length;

  return (
    <>
      <div className="grid stats">
        <Stat label="Nodes" value={snapshot.nodes.length} note={`${snapshot.nodes.filter((node) => node.ready).length} ready`} />
        <Stat label="Pods" value={snapshot.pods.length} note={`${notReady} not ready`} accent={notReady ? "warn" : "ok"} />
        <Stat label="Workloads" value={snapshot.workloads.length} note={`${snapshot.workloads.filter((workload) => workload.ready === workload.desired).length} fully ready`} />
        <Stat label="Services" value={snapshot.services.length} note={`${snapshot.services.filter((service) => service.endpoints.some((endpoint) => endpoint.ready)).length} with ready endpoints`} />
        <Stat label="Storage claims" value={snapshot.storage.claims.length} note={`${pendingClaims} not bound`} accent={pendingClaims ? "warn" : undefined} />
        <Stat label="Network policies" value={snapshot.network_policies.length} note="namespace isolation objects" />
      </div>

      <Card title="Nodes" subtitle="Capacity, allocatable resources and scheduling state" tight>
        <DataTable
          rows={snapshot.nodes}
          columns={nodeColumns}
          rowKey={(node) => node.name}
          searchText={(node) => `${node.name} ${node.roles.join(" ")} ${node.os_image} ${node.zone}`}
          searchPlaceholder="Filter nodes…"
          emptyMessage="No nodes returned"
        />
      </Card>

      <Card title="Workloads" subtitle="StatefulSets, Deployments and DaemonSets across the collected namespaces" tight>
        <DataTable
          rows={snapshot.workloads}
          columns={workloadColumns}
          rowKey={(workload) => `${workload.namespace}/${workload.kind}/${workload.name}`}
          searchText={(workload) => `${workload.namespace} ${workload.name} ${workload.kind} ${workload.component ?? ""}`}
          searchPlaceholder="Filter workloads…"
          emptyMessage="No workloads returned"
          initialSort={{ key: "role", direction: "asc" }}
        />
      </Card>

      <Card title="Services and endpoints" subtitle="Endpoint readiness comes from EndpointSlices" tight>
        {snapshot.services.length === 0 ? (
          <Empty>No Services returned</Empty>
        ) : (
          <DataTable
            rows={snapshot.services}
            columns={serviceColumns}
            rowKey={(service) => `${service.namespace}/${service.name}`}
            searchText={(service) => `${service.namespace} ${service.name} ${service.type} ${service.selector ? JSON.stringify(service.selector) : ""}`}
            searchPlaceholder="Filter services…"
            emptyMessage="No Services returned"
          />
        )}
      </Card>

      <div className="grid two">
        <Card title="Storage classes" subtitle="Provisioners available to claims" tight>
          <DataTable
            rows={snapshot.storage.classes}
            columns={[
              {
                key: "name",
                label: "Class",
                render: (row) => (
                  <span className="row">
                    <span className="mono" style={{ fontSize: 11.5 }}>{row.name}</span>
                    {row.is_default && <Badge tone="accent">default</Badge>}
                  </span>
                ),
                sortValue: (row) => row.name,
              },
              {
                key: "provisioner",
                label: "Provisioner",
                render: (row) => <span className="mono" style={{ fontSize: 11 }}>{row.provisioner}</span>,
                sortValue: (row) => row.provisioner,
              },
              {
                key: "binding",
                label: "Binding",
                render: (row) => <span className="muted">{row.binding_mode}</span>,
                sortValue: (row) => row.binding_mode,
              },
            ]}
            rowKey={(row) => row.name}
            emptyMessage="No StorageClasses"
          />
        </Card>
        <Card title="Persistent volume claims" subtitle="Database state and object-store scratch volumes" tight>
          <DataTable
            rows={snapshot.storage.claims}
            columns={claimColumns}
            rowKey={(claim) => `${claim.namespace}/${claim.name}`}
            searchText={(claim) => `${claim.namespace} ${claim.name} ${claim.storage_class} ${claim.phase}`}
            searchPlaceholder="Filter claims…"
            emptyMessage="No claims in scope"
            pageSize={20}
          />
        </Card>
      </div>

      <Card title="Network policies" subtitle="L3/L4 rules that can block object-store or inter-node traffic" tight>
        <DataTable
          rows={snapshot.network_policies}
          columns={netpolColumns}
          rowKey={(policy) => `${policy.namespace}/${policy.name}`}
          searchText={(policy) => `${policy.namespace} ${policy.name} ${policy.pod_selector}`}
          searchPlaceholder="Filter policies…"
          emptyMessage="No NetworkPolicies in the collected namespaces"
        />
      </Card>
    </>
  );
}
