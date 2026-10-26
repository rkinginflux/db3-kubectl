# Environment console (`kubectl db3 webui`)

`kubectl db3 webui` starts a read-only console for the Kubernetes environment
around an InfluxDB 3 deployment: topology, workloads, configuration, Helm
releases, events, and bounded pod logs. It is a local process, not a deployed
service.

```
kubectl db3 webui
# db3 webui 0.1.0 — read-only environment console
#   URL      http://127.0.0.1:9191
#   Context  kubectl default
#   Scope    all namespaces
#   Cache    10s per snapshot
#   Stop     Ctrl+C (the port is released when this process exits)
```

Open the printed URL. Press Ctrl+C in the terminal to stop the console; the
listening socket closes with the process, so nothing keeps serving.

## Lifecycle and footprint

- The server exists only for the lifetime of the command. It installs no
  Deployment, Service, or DaemonSet, and it writes nothing to the cluster.
- Ctrl+C (SIGINT) is handled explicitly: the accept loop stops, the process
  exits 0, and the port is released. There is no detached daemon and no
  background listener after exit.
- Nothing is uploaded. Snapshots are cached in memory only; log tails are
  streamed into the response and never written to disk.
- The binary embeds the entire frontend (HTML, CSS, JS) as one asset, so the
  console works offline and needs no Node runtime.

## Flags

| Flag | Default | Meaning |
| --- | --- | --- |
| `--port` | `9191` | TCP port. `0` lets the OS choose; the resolved port is printed. |
| `--address` | `127.0.0.1` | Bind address. Non-loopback values print a warning. |
| `-n, --namespace` | all | Restrict collection to one namespace. |
| `--context` | current | kubeconfig context to read. |
| `--refresh` | `10` | Seconds a snapshot stays cached before a rebuild. |
| `--timeout` | `60` | Per-collector timeout in seconds. |
| `--no-secrets` | off | Never request Secret objects at all. |
| `--manifest` | off | Print the read-only RBAC manifest and exit. |

The browser can request a forced rebuild (`/api/snapshot?refresh=1`), but the
server rate-limits that to one rebuild every two seconds. The UI's auto-refresh
uses the cache instead of forcing, so a 30-second browser interval does not
mean a rebuild every 30 seconds on a slow API server.

## What is collected

Each panel is its own `kubectl` call with its own timeout. One failure degrades
one panel and is reported in the Overview under **Collectors**; it never aborts
the snapshot.

| Collector | Command |
| --- | --- |
| `kubectl-version` | `kubectl version -o json` |
| `kubeconfig` | `kubectl config view --minify -o json` |
| `nodes` | `kubectl get nodes -o json` |
| `namespaces` | `kubectl get namespaces -o json` |
| `pods` | `kubectl get pods [-A] -o json` |
| `workloads` | `kubectl get statefulsets,deployments,daemonsets [-A] -o json` |
| `services` | `kubectl get services [-A] -o json` |
| `endpointslices` | `kubectl get endpointslices.discovery.k8s.io [-A] -o json` |
| `storageclasses` | `kubectl get storageclasses -o json` |
| `persistentvolumes` | `kubectl get persistentvolumes -o json` |
| `persistentvolumeclaims` | `kubectl get persistentvolumeclaims [-A] -o json` |
| `networkpolicies` | `kubectl get networkpolicies [-A] -o json` |
| `events` | `kubectl get events [-A] -o json` |
| `helm` | `helm list -A -o json` |
| `configmaps` | `kubectl get configmaps -n <db3 namespace> -o json` |
| `secrets` | `kubectl get secrets -n <db3 namespace> -o json` (`--no-secrets` skips) |

Configuration is scoped to namespaces that actually contain InfluxDB 3 pods.
That keeps a snapshot small on large clusters and avoids reading unrelated
application configuration.

### How InfluxDB 3 objects are recognised

Pods qualify when one of their images contains `influxdb`. The role comes from
`app.kubernetes.io/component`, `influxdb3/component`, `component`, `role`, or
`app`, normalised to `ingester`, `querier`, `compactor`, or `processor`. If no
label matches, the pod name is inspected and the row is marked with `~` in the
UI (`inferred_component: true` in JSON) so you can tell a convention from a
guess.

The cluster id comes from a `cluster-id`-style label, then `--cluster-id`, then
`INFLUXDB3_CLUSTER_ID`, then `app.kubernetes.io/instance`, then the namespace.
Mode comes from `--mode` or `INFLUXDB3_ENTERPRISE_MODE`/`INFLUXDB3_MODE`. Object
store comes from `--object-store=`. License presence is derived from
`*LICENSE*` environment names (never their secret values) plus
`INFLUXDB3_LICENSE_TYPE`/`_EMAIL` when they are literal values.

Workload desired counts come from the owning StatefulSet/Deployment/DaemonSet,
so a scaled-down role is visible even when no pod exists to count.

## HTTP API

The console serves four JSON endpoints plus the app shell. All are GET, bound
to the same origin; there is no CORS grant and no auth token, which is why the
default bind is loopback.

| Path | Purpose |
| --- | --- |
| `/` | Embedded single-page app (also served for unknown non-API paths). |
| `/api/health` | `{"ok":true,"version":"…"}` |
| `/api/snapshot[?refresh=1]` | The whole environment snapshot. |
| `/api/logs?namespace=&pod=&container=&tail=` | Bounded log tail, token-redacted. |
| `/api/manifest` | The RBAC manifest as `text/yaml`. |

`/api/logs` validates namespace, pod, and container names against
`[A-Za-z0-9._-]`, rejects a leading `-`, clamps `tail` to 1–20000, and passes
arguments as an argv array (never a shell string). Unknown `/api/*` paths
return JSON 404 instead of the app shell.

## Data handling

- Secret `data`/`stringData` is dropped as each Secret object is converted; the
  snapshot carries only namespace, name, type, key names, and age.
  `secret_values_exposed` is false in every payload.
- Environment values whose names look like credentials (`TOKEN`, `SECRET`,
  `PASSWORD`, `CREDENTIAL`, `PRIVATE_KEY`, `ACCESS_KEY`, `API_KEY`, `JWT`,
  `LICENSE`, `AUTH`) are replaced with `[redacted]`. A small allowlist keeps
  operational values visible (`INFLUXDB3_MODE`, `INFLUXDB3_OBJECT_STORE`,
  `INFLUXDB3_BUCKET`, `INFLUXDB3_LICENSE_TYPE`, and similar).
- `valueFrom` references become `<secret name/key>` or `<configMap name/key>`
  so you can see provenance without the value.
- ConfigMap values matching the same credential patterns are replaced with
  `[redacted]`; other values are truncated to 2000 characters.
- If `INFLUXDB3_TOKEN` is exported, exact, JSON-escaped, and UTF-16-escaped
  occurrences are redacted from captured log output.
- The collector still asks the API server for Secret objects (RBAC has no
  metadata-only verb). Their values reach the local `kubectl` process and this
  process's memory before being discarded; they are never serialised, logged,
  or cached. Use `--no-secrets` if that is not acceptable.

## The RBAC manifest

For an administrator workstation the console reads through your kubeconfig and
needs no cluster object. The manifest exists so the permission surface is
explicit and reusable:

```sh
kubectl db3 webui --manifest | kubectl apply -f -
# or
kubectl apply -f deploy/webui/rbac.yaml
```

It creates:

- namespace `db3-webui`
- ServiceAccount `db3-webui` (token not auto-mounted)
- ClusterRole `db3-webui-readonly` — get/list/watch on the objects above, plus
  `pods/log` get and `influxdata.com` custom resources
- ClusterRoleBinding `db3-webui-readonly`

To read through that identity instead of your personal kubeconfig:

```sh
kubectl --as=system:serviceaccount:db3-webui:db3-webui get pods -A
```

There are no write verbs. The Secret grant is the one to review before using
this outside a lab, because RBAC cannot restrict it to metadata.

## Frontend

`webui/` is a Vite + React + TypeScript app. `vite-plugin-singlefile` inlines
all assets into `webui/dist/index.html`, which `src/webui.rs` embeds with
`include_str!`. The built file is committed so `cargo build` works with no Node
toolchain; rebuild it after changing anything under `webui/src`:

```sh
cd webui
npm ci
npm run build
cd ..
cargo build --locked --release
```

Views: Overview, Topology (nodes → workloads → services, edges derived from pod
scheduling and selectors), InfluxDB 3, Kubernetes, Helm, Config & secrets,
Events, Logs. The theme toggle (or `t`) switches light/dark; dark is the default
and the choice is stored in `localStorage`. `r` refreshes. Routes are hashes
(`#topology`), so a bookmark survives a reload.

The page is served with `Content-Security-Policy` limited to inline scripts and
styles from the same origin, `X-Frame-Options: DENY`, `nosniff`, and
`no-referrer`. API responses are `no-store`.

## Troubleshooting

**Port already in use.** The process exits 1 with
`could not start the console on 127.0.0.1:9191 … choose another with --port`.
Pick another port; there is no orphan process to clean up.

**Every collector failed.** `kubectl` is missing from `PATH` or the context
cannot be reached. The Overview shows the exact error per collector; the UI
still renders.

**No InfluxDB 3 cluster.** The deployment may use different role labels. Check
`kubectl get pods -A --show-labels`; if the component label differs, the pod
name heuristics may still catch it (marked `~`). Otherwise the topology is
empty by design rather than guessing.

**Endpoints show zero ready.** `endpointslices.discovery.k8s.io` may be
forbidden by RBAC on older clusters. Check the Collectors panel for that
collector's error.

**Helm releases missing.** `helm` is an optional client-side dependency. If it
is absent, only that panel is empty; the collector error explains why.

**Config panel says "configuration collection was skipped".** No pod matched
the InfluxDB image check, so no namespace was considered a database namespace.
Use `-n <namespace>` if you want that namespace read anyway (it must still
contain a recognised database pod).

## Limitations

- InfluxDB 3 SQL/InfluxQL is not queried; this is an environment console, not a
  data browser. Use the database's own Web UI or CLI for queries.
- Custom resources are not expanded into a CR table; operator-managed
  deployments are visible through their pods, workloads, Services, and Helm
  release.
- Logs are a bounded tail from the Kubernetes log API. Rotated logs and
  previous container instances are not retrieved.
- The console reflects what RBAC allows. A denied collector is reported, not
  retried with broader permissions.
- There is no authentication. Keep the loopback default; binding to `0.0.0.0`
  exposes the snapshot to anyone who can reach the port.
