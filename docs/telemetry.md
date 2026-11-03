# Performance Telemetry And Migration Evidence

Related enhancement: <https://github.com/influxdata/db3-kubectl/issues/4>.

## Scope And Dependencies

This opt-in integration consumes an existing, trusted contract-1.0 telemetry
provider. It does not deploy Prometheus, a Kubernetes collector, an exporter,
or an InfluxDB database for telemetry. The native server requires curl when the
integration is enabled. Python is needed only for the optional reference client
and the separately reviewed provider prototype, not to run the console.

The provider/collector is separate from this patch. A source-only provider
reference is supplied separately in the maintainer handoff, not silently enabled
or installed in the cluster. It requires deployment-specific configuration,
existing Prometheus targets and explicitly approved Kubernetes read/exec access.
Do not point this console at a generic Prometheus URL: it expects the bounded
JSON contract described below, not Prometheus's native API.

## Enable The View

Have the provider generate an owner-only credential file containing `token`,
`auth_type` (`bearer`), `port` (9091) and `expires_at_unix` (future Unix seconds).
The token must be 32-128 URL-safe alphanumeric, underscore or hyphen characters.
Never substitute a database operator token. Do not check credentials into Git.

```sh
chmod 600 /private/path/telemetry-access.json
kubectl db3 webui --context example-context -n influxdb3 --port 9193 \
  --telemetry-access /private/path/telemetry-access.json
```

Open `http://127.0.0.1:9193/#telemetry`. Use the exact bound authority, not an
interchangeable `localhost` alias. The provider must be reachable at
`http://127.0.0.1:9091`. If it runs elsewhere, arrange a private, authenticated
transport/forward separately. The native bridge deliberately ignores the
credential file's URL for transport and refuses non-loopback console binding.
Neither `--query`, `--info` nor `--helm` is required for telemetry. Those existing
features retain their own independent opt-ins and permission requirements.

## Views

- Performance: node CPU/memory, database working set, execution reservation/limit,
  newly observed execution OOMs, compaction queue, filesystem capacity, per-pod
  resources, last/current container exits and selectable history.
- Running configuration: allowlisted observed startup inputs, image help defaults,
  declared environment, unrecognized names and drift. These are distinct forms
  of evidence, not a claim to introspect every effective engine setting.
- Signals and sources: metric IDs, units, quality and scrape freshness. Missing,
  stale or non-finite observations remain unknown instead of becoming zero.

The download icon exports the sanitized snapshot. Even without credentials,
topology and startup settings are private operational data; review before sharing.

## Provider Contract

The [snapshot JSON Schema](telemetry/snapshot.schema.json) is Draft 2020-12;
the [metric catalog](telemetry/metric-catalog.json) lists the 41 supported lab
prototype IDs/units without deployment-specific queries. Providers may mark
optional metrics unavailable rather than inventing observations. Source job/role
labels must match the documented mapping in `webui/src/telemetry.ts`.

Every provider endpoint requires Bearer authorization. The console uses only:

| Endpoint | Purpose |
| --- | --- |
| `GET /v1/snapshot` | Cached configuration, metrics, container status and quality |
| `GET /v1/history?metric=ID&start=SECONDS&end=SECONDS&step=SECONDS` | Fixed-catalog history |

External clients may also read `/v1/config`, `/v1/metrics`, `/v1/catalog`,
`/v1/schema` and `/v1/health` when implemented by the provider. They must use
their backend, not embed credentials in browser JavaScript or URLs.

Contract version is the string `1.0`. `config` and `metrics` may be null during
startup. HTTP 200 means the API responded, not that all data is usable. Each
section has `observed_at`, `fresh`, `max_age_seconds` and `status`. Each metric
has `id`, `unit`, `status` and labeled samples with numeric-or-null `value` and
`evaluated_at`. Evaluation time is not the underlying scrape time; also inspect
source `last_scrape`, health and age. Enforce freshness between polls.

The prototype samples metrics every 30 seconds and configuration every five
minutes, with 90-second/900-second freshness limits. It caches current reads;
another client must not trigger new exec or scrape operations. These settings
describe that provider, not universal guarantees made by the console. A config
fingerprint excludes volatile timestamps/readiness but includes process identity.

History is limited to catalog IDs, 24-hour ranges, step >=15 seconds, <=2000
intervals, <=64 series, <=50,000 points and <=4 MiB upstream responses. The
native bridge also bounds curl time and response capture. The provider enforces
one active history query and a two-second minimum gap, returning 429 when busy.
The UI makes one delayed retry. No gap interpolation is applied. Available
history depends on the existing Prometheus retention and size cap; 24 hours
was the lab's retention, not a new TSDB installed by this feature.

## Execution Memory Is Not Pod Memory

`query_datafusion_query_execution_ooms_total` counts execution allocation
failures. `datafusion_mem_pool_bytes` must preserve both `state=reserved` and
`state=limit` for each source, including history. Utilization is reserved/limit,
never their sum. Do not add pool bytes to pod working-set memory as though they
were separate allocations. Compactor admission/input budget is another scope.

The headline pool excludes dedicated WebUI nodes; the table retains them.
Ambiguous/multiple reservation or limit series for a pod become unknown.
The raw history legend retains state, with dashed limit lines.

New execution OOMs are compared with a baseline keyed by pod UID and container
ID, established when the view receives valid observations. A page reload or
leaving/re-entering the view can establish another baseline. The reset icon
changes only the browser baseline, not the engine counter. This is NOT the
Windows migration run baseline. Counter decreases, replaced/missing processes
or unavailable observations must not be reported as zero new failures.

The independent `metrics.container_health` channel includes pod/container
identity, readiness, restarts, current state and nullable current/last termination
records. Records include reason, `oom_killed`, exit code, signal and start/end
timestamps, but no raw messages. No retained record does not prove no historic
OOM occurred. Exit code 137 alone does not establish an OOM kill.

An observed import failure in the Enterprise 3.12.1 lab increased the engine
counter from 0 to 39 without a container restart. Its execution pool was about
307.2 MiB, compactor input budget 512 MiB and pod limit 1536 MiB. The error was
`Memory Exhausted while Sorting (DiskManager is disabled)`. This illustrates
why increasing pod memory alone is not an established remedy. Sampled gauges
may miss brief peaks, and a refused allocation may never become reserved.

## Migration And Other Data Operations

Migration/backfill tooling can correlate import IDs, upload latency, SQL latency,
execution OOM changes and compaction pressure with a configuration fingerprint.
The same evidence helps ingestion/load tests, replay jobs, bulk loads, query
investigations and capacity/tuning comparisons. It does not prove that a single
writer caused a cluster-wide change when other clients share the cluster.

Poll from the writer's backend in a bounded background task, outside upload
and journal locks. Use timeouts/backoff and keep telemetry credentials separate
from database credentials. Establish the writer's own per-run OOM baseline.
Store health on every observation, not only when the config fingerprint changes.

The [reference client](../examples/telemetry/fetch_snapshot.py) fetches a snapshot
without following redirects or HTTP proxies and refuses to overwrite evidence.
It accepts loopback or private Tailscale addresses on 9091; remote HTTP is only
appropriate within that authenticated encrypted path. Other deployments need a
reviewed secure transport, not a relaxed URL check. Protect Windows credentials
with ACLs; do not print authorization headers.

```sh
python3 examples/telemetry/fetch_snapshot.py \
  --access /private/path/telemetry-access.json --output new-snapshot.json
```

The optional [SQLite evidence schema](../examples/telemetry/windows-evidence.sql)
is for a SEPARATE writer-side file, never the target InfluxDB database. Retain
run IDs, config fingerprints, metric quality/labels, source freshness, container
status and writer measurements. Choose bounded retention and batch evidence
writes. It does not replace a migration journal or delete history automatically.

Begin with observe/advisory mode. Automatic control is not implemented here.
A later controller should combine existing writer/user ceilings with cluster
pressure, use cooldown/hysteresis, and avoid increasing concurrency when required
signals are missing. Pausing should stop new admissions while tracking accepted
asynchronous imports; do not replay an accepted upload after a timeout. Audit
every automatic recommendation/action and its supporting observation.

## Security And Limitations

The provider is a trusted sanitization boundary. The console does not turn
arbitrary upstream JSON into safe data simply by proxying it. Providers must
exclude credentials, licenses/emails, raw command lines/logs and unallowlisted
environment values before responding. The native bridge checks contract version
and context/namespace and redacts its own known credential; external clients
should additionally validate the full schema and intended cluster IDs.

The console keeps the provider credential server-side, sends it over curl stdin
and retains its existing per-process browser session guard. Curl redirects,
ambient curl configuration and proxies are disabled. Source responses and
timeouts are bounded. No new Kubernetes permissions, public listener or
automatic database tuning is added by this patch. Provider exec, if separately
approved, is restricted to fixed allowlisted environment reads and image help.

Monitoring has nonzero CPU/memory/I/O cost. Node/PVC metrics may describe a shared
VM/filesystem rather than a physical host or database quota. Never sum shared
filesystem usage across PVCs. An exporter being up does not prove a cached
directory scan is current. Idle latency histograms may be NaN and should remain
explicitly non-finite, not zero or automatically unhealthy.

## References

- [Enterprise bulk-import API](https://docs.influxdata.com/influxdb3/enterprise/api/bulk-import/)
- [Enterprise configuration options](https://docs.influxdata.com/influxdb3/enterprise/reference/config-options/)
- [Prometheus HTTP API](https://prometheus.io/docs/prometheus/latest/querying/api/)
- [Kubernetes container states](https://kubernetes.io/docs/concepts/workloads/pods/pod-lifecycle/#container-states)
