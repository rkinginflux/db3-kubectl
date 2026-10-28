import { useMemo, useState } from "react";
import { Badge, Card, Empty } from "./ui";
import { fetchInfo } from "../api";
import type { InfoResponse, Snapshot } from "../types";

/** The two views the panel shows, in the order the CLI is run. */
const PANELS = [
  { key: "nodes", command: "influxdb3 show nodes --format json", label: "Nodes" },
  { key: "license", command: "influxdb3 show license info --format json", label: "License" },
] as const;

function pretty(value: unknown): string {
  if (value === null || value === undefined) return "(no output)";
  return JSON.stringify(value, null, 2);
}

/**
 * Renders `influxdb3 show nodes` and `show license info` as pretty JSON.
 *
 * These run inside the selected pod through `pods/exec`, so the whole panel is
 * gated behind the console's `--info` opt-in. When it is off the section still
 * appears — with the reason and the flag to add — because a section that silently
 * vanishes reads as a collection failure.
 */
export function BasicInformation({ snapshot }: { snapshot: Snapshot }) {
  const pods = useMemo(() => {
    const database = snapshot.pods.filter((pod) => pod.component);
    const source = database.length ? database : snapshot.pods;
    return [...source]
      .sort((a, b) => Number(b.ready) - Number(a.ready) || `${a.namespace}/${a.name}`.localeCompare(`${b.namespace}/${b.name}`))
      .map((pod) => ({ key: `${pod.namespace}/${pod.name}`, pod }));
  }, [snapshot.pods]);

  const [podKey, setPodKey] = useState(pods[0]?.key ?? "");
  const [result, setResult] = useState<InfoResponse | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const enabled = snapshot.info_enabled === true;
  const selected = pods.find((entry) => entry.key === podKey)?.pod;

  async function load() {
    if (!selected) return;
    setLoading(true);
    setError(null);
    try {
      setResult(
        await fetchInfo({
          namespace: selected.namespace,
          pod: selected.name,
          container: selected.containers[0]?.name ?? null,
        }),
      );
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : String(cause));
      setResult(null);
    } finally {
      setLoading(false);
    }
  }

  const subtitle = "influxdb3 show nodes and show license info, read through the database CLI";

  if (!enabled) {
    return (
      <Card title="Basic Information" subtitle={subtitle}>
        <Empty glyph="⌗">
          Off by default. Restart the console with <code>--info</code> to read nodes and license information; that runs{" "}
          <code>influxdb3 show nodes</code> and <code>influxdb3 show license info</code> inside the selected pod through{" "}
          <code>pods/exec</code>, and needs <code>INFLUXDB3_TOKEN</code>.
        </Empty>
      </Card>
    );
  }

  return (
    <Card title="Basic Information" subtitle={subtitle}>
      <div className="stack">
        <div className="toolbar">
          <select value={podKey} onChange={(event) => setPodKey(event.target.value)} aria-label="Select pod for basic information">
            {pods.length === 0 && <option value="">No pods available</option>}
            {pods.map((entry) => (
              <option key={entry.key} value={entry.key}>
                {entry.key}
                {entry.pod.component ? ` · ${entry.pod.component}` : ""}
                {entry.pod.ready ? "" : " · not ready"}
              </option>
            ))}
          </select>
          <button type="button" className="btn primary" onClick={load} disabled={!selected || loading}>
            {loading ? <span className="spin">◌</span> : "Read"}
          </button>
          {result && <Badge tone="neutral">{result.pod}</Badge>}
        </div>

        {error && <div className="error-banner">{error}</div>}
        {result?.errors &&
          Object.entries(result.errors).map(([command, detail]) => (
            <div key={command} className="error-banner">
              {command}: {detail}
            </div>
          ))}

        {result ? (
          PANELS.map((panel) => (
            <div key={panel.key} className="stack" style={{ gap: 4 }}>
              <span className="muted mono" style={{ fontSize: 11.5 }}>
                {panel.command}
              </span>
              <pre className="logs">{pretty(result[panel.key])}</pre>
            </div>
          ))
        ) : (
          <Empty glyph="{}">Choose a pod and read its node and license information.</Empty>
        )}

        <p className="muted" style={{ margin: 0, fontSize: 11.5 }}>
          Read with <code>kubectl exec</code> into the selected pod and never written to disk. The admin token travels on
          stdin, not in the command line; known token patterns are redacted from the output.
        </p>
      </div>
    </Card>
  );
}
