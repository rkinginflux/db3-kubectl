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
| `--address` | `127.0.0.1` | Bind address. Only loopback IPs or `localhost`; remote and unspecified addresses are rejected. |
| `-n, --namespace` | all | Restrict collection to one namespace. |
| `--context` | current | kubeconfig context to read. |
| `--refresh` | `10` | Seconds a snapshot stays cached before a rebuild. |
| `--timeout` | `60` | Per-collector timeout in seconds. |
| `--include-secret-metadata` | off | Opt in to Secret names/type/key names; values are discarded, but Kubernetes requires full Secret read permission. |
| `--no-secrets` | default behavior | Explicitly prohibit direct and Helm storage Secret reads; conflicts with both opt-ins. |
| `--helm` | off | Explicitly allow Helm release collection, which may read/decode Secret-backed storage using operator credentials; independent of `--include-secret-metadata`. |
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
| `helm` | `helm list -A -o json` (only with `--helm`) |
| `configmaps` | `kubectl get configmaps -n <db3 namespace> -o json` |
| `secrets` | `kubectl get secrets -n <db3 namespace> -o json` (only with `--include-secret-metadata`) |

Configuration is scoped to namespaces that actually contain InfluxDB 3 pods.
At most 16 namespaces are processed sequentially, one parsed raw result at a
time. ConfigMaps and opt-in Secret metadata share 500 rows and a 1 MiB
serialized-row budget (measured after value masking, including delimiters). Any
namespace, row, byte or per-resource truncation keeps a visible partial/error
configuration status while retaining bounded returned data.

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

Role desired counts use only observed pod owner kind/name plus namespace,
never same-role workload labels. Verified distinct owners are summed once;
with multiple owners the singular workload field remains null. Missing or
ambiguous owner chains (including ReplicaSet-to-Deployment) leave workload and
desired null, displayed as unknown. A role with no observed pods is not inferred.

## HTTP API

The console preserves its four API endpoints plus the app shell (GET/HEAD;
other methods return 405). All API requests, including health and manifest,
require `X-DB3-Session`. A cryptorandom 256-bit per-run session is bootstrapped
in the app shell's `db3-session` meta tag, never in a URL or terminal log.
The frontend supplies the header automatically. Reload after restarting the
command. Host must equal the printed bound authority, including the actual
port chosen for `--port 0`; an Origin, when supplied, must match that exact
HTTP origin. There is no CORS grant. This is local-browser protection, not a
multi-user authentication system: another local process can fetch the shell.

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
- All inline environment values, valueFrom references, displayed command
  arguments, and ConfigMap values are `[redacted]`. Opaque values are hidden
  even when the variable/key name looks harmless. Safe derived database mode,
  cluster ID, object-store, and license-presence observations are computed
  before display sanitization, preserving database details without raw values.
- Raw collector stderr is discarded, not forwarded to errors or the logs API.
  Errors are generic status/timeout/limit messages, never arbitrary client text.
- If `INFLUXDB3_TOKEN` is exported, exact, JSON-escaped, and UTF-16-escaped
  occurrences are redacted from captured log output.
- By default neither Secret objects nor Helm releases are requested: Helm is not
  executed at all. With `--include-secret-metadata`,
  full Secret objects reach kubectl and process memory before values are
  discarded: Kubernetes RBAC has no metadata-only Secret permission. Names
  and key names are deliberately exposed only by this explicit opt-in.
  Independently, `--helm` explicitly permits Helm to read/decode its storage
  Secrets (including stored values) under your credentials, though those values
  are not returned in the console snapshot. `--no-secrets` conflicts with both
  opt-ins; `--include-secret-metadata --helm` can be used together.
- Application logs may contain unknown credentials that cannot be identified
  automatically. Known exported `INFLUXDB3_TOKEN` is redacted from snapshot and
  log payloads; treat all displayed logs/cluster metadata as private.

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

There are no write/exec verbs and no Secret grants in the base manifest.
Opt-in `--helm` may need separately reviewed storage Secret reads; a failed Helm panel
is not a reason to grant broad Secret access. Review any separately scoped
permission before opting in. The command never automatically applies RBAC.

## Frontend

`webui/` is a Vite + React + TypeScript app. `vite-plugin-singlefile` inlines
all assets into `webui/dist/index.html`, which `src/webui.rs` embeds with
`include_str!`. The built file is committed so `cargo build` works with no Node
toolchain; rebuild it after changing anything under `webui/src`:

```sh
cd webui
# Node 22.12+ (22.x or 24+), never needed at runtime
npm ci
npm test
npm run typecheck
npm run build
npm run check:reproducible
npm audit
cd ..
cargo build --locked --release
```

Views: Overview, Topology (nodes → workloads → services, edges derived from pod
scheduling and selectors), InfluxDB 3, Kubernetes, Helm, Config & secrets,
Events, Logs. The theme toggle (or `t`) switches light/dark; dark is the default
and the choice is stored in `localStorage`. `r` refreshes. Routes are hashes
(`#topology`), so a bookmark survives a reload.

The server adds a cryptorandom CSP nonce to the singlefile's inline scripts;
`script-src` permits that nonce, not `unsafe-inline`. Style `unsafe-inline`
is intentionally retained for the existing React inline styles and bundled
CSS. Responses are no-store, deny framing, use nosniff and no-referrer, and
never grant CORS. The complete license and third-party notices are embedded
in the HTML legal template; no CDN or Node runtime is needed.

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

**Helm releases missing.** Collection is disabled by default; the Helm view says
"disabled; enable --helm", not that no releases exist. Opt in only after
reviewing Helm storage Secret reads. With `--helm`, a missing client or denied
read is a collector failure, not proof the cluster has no releases.

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
- Session/authority checks protect against remote web origins, not hostile
  local processes or malicious browser extensions. Non-loopback binds are
  rejected rather than supported as an anonymous remote service.
- Resource arrays are capped at 500 entries and service matching at 50,000
  comparisons with visible partial/truncation findings. The browser topology
  caps each input column at 80, edges at 500, and relationship/selector work at
  20,000. Its partial notice includes unmapped ownership. Only observed
  namespace + owner kind/name links are used, not same-role guesses;
  ReplicaSet-to-Deployment ancestry is not collected.
- A missing role means not observed in this scope, not proof it is absent.
- WebUI collectors have a 4 MiB stdout limit, timeout and cancellation checks.
  Their process groups are killed before reaping direct children, including
  successful leaders with background descendants. SIGINT and SIGTERM cancel
  active collection and join request workers before exit. HTTP sockets have a
  two-second idle I/O timeout on the supported Unix hosts (not a total header
  wall-clock deadline for clients that continually send bytes). At most 16
  application request workers run concurrently. Body-bearing framing is rejected
  with 400 and connection close before body-reader creation: positive/malformed
  Content-Length and any Transfer-Encoding are forbidden (zero length is fine).
  Request/header lines are limited to 8 KiB, the header section to 32 KiB and
  header fields to 64 through the opt-in vendored `db3-read-only` feature. Support bundle capture uses
  its original implementation and is not changed by these WebUI limits.
