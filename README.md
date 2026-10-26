# db3-kubectl

Independent, community-maintained Rust kubectl plugin for use with the InfluxDB® 3 Enterprise database.[^trademark] The compiled plugin requires no Python runtime. It collects support bundles and logs, and serves a temporary read-only console for the Kubernetes environment around a deployment. Nothing is uploaded, and neither command changes database configuration or Kubernetes resources.

This project is not an official product of InfluxData. See [trademark usage and review notes](TRADEMARKS.md) for attribution, contributor guidance, and permission-review limitations.

## Requirements

- kubectl on the administrator's PATH and a compiled `kubectl-db3` binary for your platform (Linux/macOS). Rust/Cargo is needed to build from source; Python 3 is needed only for the optional compatibility-test harness. Node.js 22.12+ (22.x or 24+) is needed only to rebuild/test the embedded console frontend.
- For JSON support collection: a kubeconfig with permission to create `pods/exec` sessions, `sh` and `influxdb3` on PATH in the selected container, and a server reachable by that container's CLI (normally http://127.0.0.1:8181). The container's host/TLS environment is honored. The plugin does not port-forward or discover credentials.
- For log collection: permission to list pods and read `pods/log` in the selected namespace. No database CLI or admin token is required.
- For the environment console: read access to nodes, pods, services, workloads, EndpointSlices, storage objects, events, and ConfigMaps. Secret collection is off by default; `--include-secret-metadata` opts into names/keys and needs separately reviewed Secret permissions. Helm is disabled unless `--helm` explicitly allows Helm storage Secret reads under your credentials; this is independent of `--include-secret-metadata`. `--no-secrets` conflicts with both opt-ins.

## Build and enable locally

Clone the source and enter the project directory:

```sh
git clone https://github.com/rkinginflux/db3-kubectl.git
cd db3-kubectl
```

Build and install the executable in the project directory:

```sh
cargo build --locked --release
install -m 755 target/release/kubectl-db3 ./kubectl-db3.new
mv -f ./kubectl-db3.new ./kubectl-db3
```

The project-root executable is the compiled Rust binary, not a Python script. Re-run these commands after changing Rust source. The temporary filename plus rename avoids partially overwriting an executable another process is using.

The console frontend is already built and committed as a single embedded HTML file, so no Node.js step is required to build the plugin. After changing anything under `webui/`, rebuild it first:

```sh
cd webui && npm ci && npm run build && cd ..
```

```sh
export PATH="$HOME/db3-kubectl:$PATH"
kubectl db3 --help
kubectl db3 support --help
```

This PATH change affects only the current shell; add it to your shell profile yourself if desired. The executable is `kubectl-db3`; kubectl discovers it as `kubectl db3`. Alternatively, place a symlink to the built executable in a directory already on your PATH, such as `~/.local/bin`.

## Collect database role logs

Collect logs across querier, processor, ingester, and compactor pods:

```sh
kubectl db3 support logs -n influxdb3
kubectl db3 support logs all -n influxdb3
```

These are equivalent and use the selector `app.kubernetes.io/component in (querier,processor,ingester,compactor)`. All matching replicas and all their containers are included, including pods belonging to StatefulSets. Selection is by pod labels, not Kubernetes Service names or StatefulSet-name guesses; it also works for matching pods managed by other controllers. The selector convention must match your deployment labels. The manifest lists the requested roles and actual collected pods; `complete` means all selected pods succeeded, not that every role has a deployed replica.

You can also select any one role:

```sh
kubectl db3 support logs querier -n influxdb3
kubectl db3 support logs processor -n influxdb3
kubectl db3 support logs ingester -n influxdb3
kubectl db3 support logs compactor -n influxdb3
```

Each role command discovers all pods matching `app.kubernetes.io/component=ROLE` in the namespace and takes a log snapshot from all containers in each pod. It does not require a pod name, run the database CLI, or call `kubectl exec`. The default label is a convention, not a verified fact about your deployment; no pod-name substring matching or fallback to all namespace pods is performed.

If your chart uses a different role label, inspect labels and override the selector:

```sh
kubectl get pods -n influxdb3 --show-labels
kubectl db3 support logs processor -n influxdb3 --selector 'role=processor'
kubectl db3 support logs all -n influxdb3 --selector 'role in (querier,processor,ingester,compactor)'
```

The selector examples are also placeholders: use the actual label from your deployment. `--selector` replaces the default role selector, so include the desired roles in your custom selector. A selector matching no pods is an error, not a successful empty collection. If multiple DB3 installations share the namespace, restrict the selector further with your installation label.

Logs include timestamps and pod/container prefixes. By default, the most recent 10,000 lines per container are collected; `--tail -1` requests all logs still available through the Kubernetes log API. This does not retrieve older rotated logs or previous container instances. `--context`, `--timeout`, and `--output` are supported:

```sh
kubectl db3 support logs processor -n influxdb3 --tail -1 --output ./processor-logs
```

The private output directory contains one `<pod>.log` and `<pod>.stderr.txt` per pod, `discovery.stderr.txt`, and `manifest.json`. The manifest records the selector, role, namespace/context options, tail limit, discovery status, and each pod's outcome. A failed/timed-out pod does not prevent collecting the rest. Exit status is 1 for discovery errors, no matches, or any log failure; 0 means all selected pods were collected. Default output directories use `db3-support-logs-<UTC timestamp>-<suffix>` in the current directory. Files are not uploaded. Known exported INFLUXDB3_TOKEN occurrences are redacted, but other secrets in application logs may remain; review before sharing. Log output is buffered in memory, so use a bounded `--tail` for large deployments.

## Collect support data

Explicitly select the pod to avoid collecting from the wrong installation:

```sh
kubectl db3 support --pod influxdb3-0 --namespace influxdb3
```

For a multi-container pod or a different kubeconfig context:

```sh
kubectl db3 support --pod influxdb3-0 --namespace influxdb3 \
  --container influxdb3 --context my-cluster --output ./support-bundle
```

Namespace/context default to kubectl's current settings. Put plugin options after `support` as shown.

To use the bare command:

```sh
export INFLUXDB3_POD=influxdb3-0
kubectl db3 support
```

That uses the namespace in your current kubectl context; add `--namespace` when needed.

### Admin-token authentication

If authentication is enabled, provide an admin token with access to all three operations:

```sh
export INFLUXDB3_TOKEN=apiv3_xxxxxxxxx
kubectl db3 support --pod influxdb3-0 --namespace influxdb3
unset INFLUXDB3_TOKEN
```

The plugin executes these read-only CLI commands inside the selected pod:

```sh
influxdb3 show nodes --format json
influxdb3 show license info --format json
influxdb3 show databases --format json
```

When `INFLUXDB3_TOKEN` is nonempty, each command receives `--token "$token"`. This mapping is explicit: the database CLI's native environment variable can differ (`INFLUXDB3_AUTH_TOKEN` in the local Enterprise source), so simply forwarding `INFLUXDB3_TOKEN` would not suffice.

The token travels over kubectl exec stdin, not in the local kubectl command arguments or the Kubernetes exec command URL. The Rust implementation prepares stdin in an anonymous, owner-only temporary file to avoid blocked pipe writes; credential bytes can therefore touch local temporary storage until that handle closes. Captured stdout/stderr are also temporarily file-backed before being read for redaction. The token is not written intentionally to the bundle; exact plaintext and JSON-escaped occurrences are redacted from captured command output. However, the remote database CLI process receives `--token`, so privileged observers inside the pod can inspect its arguments. A privileged process on the administrator's machine can also inspect the exported environment. This is not a guarantee against all credential disclosure or an exhaustive sanitizer of server output. With no plugin token, no `--token` argument is added; the container CLI's own authentication environment/configuration still applies.

Do not paste real tokens into issue reports. Review all bundle contents before sharing: node names, database names, license information, and diagnostics can be sensitive. No automatic upload occurs.

## Bundle contents

The default output is a unique `db3-support-<UTC timestamp>-<suffix>/` directory in the current working directory. `--output` must name a new directory whose parent already exists; existing files are never overwritten.

- `nodes.json`
- `license-info.json`
- `databases.json`
- A separate `<name>.stderr.txt` for each command.
- `manifest.json`: selected target options, command names, return codes, output filenames, per-command errors, and `complete` status. Null namespace/context/container means the corresponding kubectl default was used.

Failed commands retain stdout as `<name>.stdout.txt` instead of claiming valid JSON. The remaining commands still run. Timeout defaults to 60 seconds per command; override with `--timeout SECONDS`. Output directories are created with owner-only permissions (0700), files with 0600 (a more restrictive umask can further restrict them).

JSON collection exit codes: 0 = all three returned valid JSON; 1 = partial collection or local execution/filesystem failure; 2 = usage error. A local filesystem failure or interruption may leave a partial directory without a manifest. A timeout terminates the local kubectl process; it does not guarantee the remote process was terminated.

## Environment console

`kubectl db3 webui` starts a temporary, read-only console for the Kubernetes
environment that hosts an InfluxDB 3 deployment: topology, workloads, Services
and endpoints, storage, Helm releases, ConfigMaps and secret key names, events,
and on-demand log tails.

```sh
kubectl db3 webui
# URL      http://127.0.0.1:9191
# Stop     Ctrl+C (the port is released when this process exits)
```

It runs only while the command runs. Ctrl+C stops the accept loop, exits 0, and
frees the port; no Deployment, Service, or background process is created, and
nothing is written to the cluster or uploaded. The default port is 9191 and can
be changed:

```sh
kubectl db3 webui --port 9300                 # another port
kubectl db3 webui --helm                    # opt in to Helm storage Secret reads
kubectl db3 webui -n influxdb3 --context lab  # narrow scope
kubectl db3 webui --no-secrets                # never read Secret objects
```

`--refresh` sets the snapshot cache lifetime (default 10s), `--timeout` the
per-collector timeout (default 60s), and `--port 0` picks a free port and prints
it. Each panel is an independent `kubectl` call; a denied or failed collector
degrades that panel and is listed under **Collectors** instead of breaking the
page.

The console only needs read access. For a reviewable least-privilege identity,
install the shipped manifest once per cluster:

```sh
kubectl db3 webui --manifest | kubectl apply -f -
# or: kubectl apply -f deploy/webui/rbac.yaml
```

It creates the `db3-webui` namespace, ServiceAccount, and a
`db3-webui-readonly` ClusterRole/ClusterRoleBinding with get/list/watch on the
objects listed above, `pods/log` get, and no write verbs. An administrator
kubeconfig does not require it; it exists so the permission surface is explicit
and usable as `--as=system:serviceaccount:db3-webui:db3-webui`.

Secret **values** never enter a snapshot: `data`/`stringData` are dropped while
the object is converted, so only namespace, name, type, and key names remain.
All inline environment values, references, displayed command arguments, and
ConfigMap values are replaced with `[redacted]`, and exported
admin tokens are redacted from log output. `kubectl` must still receive Secret
objects from the API server to list their keys, because RBAC has no
metadata-only verb for them. Neither direct Secret collection nor Helm runs by
default. `--include-secret-metadata` allows direct Secret reads; independently,
`--helm` allows Helm to read/decode Secret-backed release storage, including
stored values. `--no-secrets` explicitly prevents either opt-in. The base RBAC
never grants Secret reads.

The UI is a React single-page app embedded in the binary (no Node runtime and no
network fetch at run time). Dark is the default theme; the header toggle (or `t`)
switches to light and remembers the choice. `r` refreshes, and routes are hashes
such as `#topology`. See [docs/webui.md](docs/webui.md) for the API, the full
collector list, role detection rules, threat model, and troubleshooting.

## Tests

```sh
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test --locked
cargo test --locked --release
cargo doc --locked --no-deps
cargo build --locked --release
DB3_BINARY="$PWD/target/release/kubectl-db3" python3 -m unittest discover -s tests -v
```

Rust tests cover native behavior. The retained Python black-box compatibility suite invokes the compiled binary directly; Python is test tooling only. Without DB3_BINARY it tests the project-root executable. Tests use temporary executable kubectl/database-CLI substitutes and never contact a cluster. They cover collection, command construction, token transport/redaction, actual remote-shell execution against a substitute CLI, partial failures, invalid JSON, timeout, private permissions, target configuration, and output preservation. The console suite starts the real binary against fake `kubectl`/`helm` programs and asserts the embedded app is served, snapshots derive cluster/role/finding data, secret values never appear in the payload, argument injection is rejected, a total collector failure still returns a snapshot, and SIGINT stops the listener and releases the port. Real kubectl plugin dispatch can also be checked safely with `kubectl db3 support --help` and `kubectl db3 webui --help`.

Live-cluster collection is not part of the automated tests. The collector does not upload support data and the console does not upload snapshots. Source repository: https://github.com/rkinginflux/db3-kubectl.

## License

This project is released under the [MIT License](LICENSE.md). The license covers this project's source code; it does not grant rights to any third-party trademark, and it does not replace the attribution and permission notes in [TRADEMARKS.md](TRADEMARKS.md). Third-party components embedded in the compiled binary are attributed in [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md).

## License

This project is released under the [MIT License](LICENSE.md). The license covers this project's source code; it does not grant rights to any third-party trademark, and it does not replace the attribution and permission notes in [TRADEMARKS.md](TRADEMARKS.md). Third-party components embedded in the compiled binary are attributed in [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md).

[^trademark]: InfluxDB® is a trademark registered by InfluxData, which is not affiliated with, and does not endorse, this project.
