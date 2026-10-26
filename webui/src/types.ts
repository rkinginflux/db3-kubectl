export type Severity = "critical" | "warning" | "info";

export interface NodeRow {
  name: string;
  ready: boolean;
  roles: string[];
  version: string;
  os_image: string;
  kernel: string;
  cpu_capacity: string;
  mem_capacity_bytes: number;
  cpu_allocatable: string;
  mem_allocatable_bytes: number;
  pods_capacity: number;
  pods_used: number;
  zone: string;
  internal_ip: string;
  age: string;
  taints: string[];
}

export interface NamespaceRow {
  name: string;
  phase: string;
  age: string;
}

export interface EnvRow {
  key: string;
  value: string | null;
  redacted: boolean;
}

export interface ContainerRow {
  name: string;
  image: string;
  ready: boolean;
  restarts: number;
  state: string;
  reason: string | null;
  env: EnvRow[];
  commands: string[];
}

export interface PodRow {
  namespace: string;
  name: string;
  node: string | null;
  phase: string;
  ready: boolean;
  restarts: number;
  start_time: string | null;
  age: string;
  component: string | null;
  cluster_id: string | null;
  inferred_component: boolean;
  images: string[];
  containers: ContainerRow[];
  owner_kind: string | null;
  owner_name: string | null;
  labels: Record<string, string>;
  mode: string | null;
}

export interface WorkloadRow {
  namespace: string;
  kind: string;
  name: string;
  desired: number;
  ready: number;
  updated: number;
  component: string | null;
  cluster_id: string | null;
  images: string[];
  age: string;
}

export interface ServicePort {
  name: string;
  port: number | null;
  target_port: string;
  protocol: string;
}

export interface EndpointRow {
  address: string;
  ready: boolean;
  target: string;
}

export interface ServiceRow {
  namespace: string;
  name: string;
  type: string;
  cluster_ip: string;
  external_ips: string[];
  ports: ServicePort[];
  selector: Record<string, string>;
  endpoints: EndpointRow[];
  component: string | null;
  cluster_id: string | null;
}

export interface StorageClassRow {
  name: string;
  provisioner: string;
  reclaim_policy: string;
  binding_mode: string;
  is_default: boolean;
  allow_expansion: boolean;
}

export interface VolumeRow {
  name: string;
  capacity_bytes: number;
  storage_class: string;
  status: string;
  claim: string;
  reclaim_policy: string;
}

export interface ClaimRow {
  namespace: string;
  name: string;
  storage_class: string;
  capacity_bytes: number;
  status: string;
  volume: string;
  phase: string;
}

export interface HelmReleaseRow {
  name: string;
  namespace: string;
  chart: string;
  app_version: string;
  status: string;
  revision: string;
  updated: string;
}

export interface KeyRow {
  key: string;
  bytes: number;
}

export interface ConfigMapRow {
  namespace: string;
  name: string;
  keys: KeyRow[];
  data: Record<string, string>;
  age: string;
}

export interface SecretRow {
  namespace: string;
  name: string;
  type: string;
  keys: string[];
  age: string;
}

export interface NetworkPolicyRow {
  namespace: string;
  name: string;
  pod_selector: string;
  policy_types: string[];
  ingress_rules: number;
  egress_rules: number;
}

export interface EventRow {
  namespace: string;
  type: string;
  reason: string;
  object: string;
  message: string;
  count: number;
  last_seen: string | null;
}

export interface RolePodRow {
  name: string;
  node: string | null;
  ready: boolean;
  restarts: number;
  mode: string | null;
}

export interface RoleState {
  role: string;
  desired: number | null;
  ready: number;
  pods: RolePodRow[];
  workload: string | null;
  mode: string | null;
}

export interface LicenseRow {
  type: string | null;
  email: string | null;
  token_present: boolean;
}

export interface Db3ClusterRow {
  key: string;
  namespace: string;
  cluster_id: string;
  object_store: string | null;
  roles: RoleState[];
  services: string[];
  license: LicenseRow | null;
  helm_releases: string[];
  pod_count: number;
}

export interface IssueRow {
  severity: Severity;
  title: string;
  detail: string;
  namespace: string | null;
}

export interface CollectorRow {
  name: string;
  ok: boolean;
  error: string | null;
  duration_ms: number;
  count: number;
}

export interface ClusterInfo {
  server: string;
  version: string;
  platform: string;
}

export interface Snapshot {
  generated_at: string;
  tool_version: string;
  context: string | null;
  namespace_filter: string | null;
  cluster: ClusterInfo;
  nodes: NodeRow[];
  namespaces: NamespaceRow[];
  pods: PodRow[];
  workloads: WorkloadRow[];
  services: ServiceRow[];
  storage: {
    classes: StorageClassRow[];
    volumes: VolumeRow[];
    claims: ClaimRow[];
  };
  helm: HelmReleaseRow[];
  helm_enabled?: boolean;
  configmaps: ConfigMapRow[];
  secrets: SecretRow[];
  secret_values_exposed: boolean;
  network_policies: NetworkPolicyRow[];
  events: EventRow[];
  db3: Db3ClusterRow[];
  issues: IssueRow[];
  collectors: CollectorRow[];
}

export interface LogsResponse {
  pod: string;
  namespace: string;
  container: string | null;
  tail: number;
  redacted: boolean;
  error: string | null;
  lines: string;
}
