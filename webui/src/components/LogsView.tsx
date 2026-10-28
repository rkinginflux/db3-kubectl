import { useMemo, useState } from "react";
import { Badge, Card, CopyButton, Empty } from "./ui";
import { SupportHandoff } from "./SupportHandoff";
import { fetchLogs } from "../api";
import type { LogsResponse, Snapshot } from "../types";

export function LogsView({ snapshot }: { snapshot: Snapshot }) {
  const pods = useMemo(() => {
    const sorted = [...snapshot.pods].sort((a, b) => {
      const roleA = a.component ? 0 : 1;
      const roleB = b.component ? 0 : 1;
      if (roleA !== roleB) return roleA - roleB;
      return `${a.namespace}/${a.name}`.localeCompare(`${b.namespace}/${b.name}`);
    });
    return sorted;
  }, [snapshot.pods]);

  const [podKey, setPodKey] = useState(pods.length ? `${pods[0].namespace}/${pods[0].name}` : "");
  const [container, setContainer] = useState<string>("");
  const [tail, setTail] = useState(500);
  const [result, setResult] = useState<LogsResponse | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const selectedPod = pods.find((pod) => `${pod.namespace}/${pod.name}` === podKey);
  const containers = selectedPod?.containers.map((item) => item.name) ?? [];

  async function load() {
    if (!selectedPod) return;
    setLoading(true);
    setError(null);
    try {
      const response = await fetchLogs(
        selectedPod.namespace,
        selectedPod.name,
        container || containers[0] || null,
        tail,
      );
      setResult(response);
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : String(cause));
      setResult(null);
    } finally {
      setLoading(false);
    }
  }

  return (
    <>
      <Card title="Pod logs" subtitle="A bounded tail, read through the Kubernetes log API on demand">
        <div className="stack">
          <div className="toolbar">
            <select
              value={podKey}
              onChange={(event) => {
                setPodKey(event.target.value);
                setContainer("");
                setResult(null);
              }}
              aria-label="Select pod"
              style={{ maxWidth: 460 }}
            >
              {pods.length === 0 && <option value="">No pods available</option>}
              {pods.map((pod) => (
                <option key={`${pod.namespace}/${pod.name}`} value={`${pod.namespace}/${pod.name}`}>
                  {pod.namespace}/{pod.name}
                  {pod.component ? ` · ${pod.component}` : ""}
                  {pod.ready ? "" : " · not ready"}
                </option>
              ))}
            </select>
            <select value={container} onChange={(event) => setContainer(event.target.value)} aria-label="Select container">
              <option value="">{containers.length > 1 ? "All containers (first)" : "Default container"}</option>
              {containers.map((name) => (
                <option key={name} value={name}>
                  {name}
                </option>
              ))}
            </select>
            <select value={tail} onChange={(event) => setTail(Number(event.target.value))} aria-label="Tail size">
              {[100, 500, 2000, 10000].map((value) => (
                <option key={value} value={value}>
                  {value} lines
                </option>
              ))}
            </select>
            <button type="button" className="btn primary" onClick={load} disabled={!selectedPod || loading}>
              {loading ? <span className="spin">◌</span> : "Fetch"}
            </button>
            {result && result.lines && <CopyButton value={result.lines} label="Copy logs" />}
          </div>

          {error && <div className="error-banner">{error}</div>}
          {result?.error && <div className="error-banner">kubectl: {result.error}</div>}

          {result ? (
            <>
              <div className="row" style={{ fontSize: 11.5 }}>
                <Badge tone="neutral">{result.pod}</Badge>
                {result.container && <Badge tone="neutral">{result.container}</Badge>}
                <span className="muted">last {result.tail} lines</span>
                {result.redacted && <Badge tone="warn">token redacted</Badge>}
              </div>
              <pre className="logs">{result.lines || "(no log output)"}</pre>
              <SupportHandoff
                snapshot={snapshot}
                pod={result.pod}
                container={result.container}
                tail={result.tail}
                logs={result.lines}
                redacted={result.redacted}
              />
            </>
          ) : (
            <Empty glyph="≋">Choose a pod and fetch its most recent log lines.</Empty>
          )}

          <p className="muted" style={{ margin: 0, fontSize: 11.5 }}>
            Logs are read with <code>kubectl logs --tail</code> and never stored on disk. Known admin tokens are redacted from the
            response; other sensitive values written to application logs are not.
          </p>
        </div>
      </Card>
    </>
  );
}
