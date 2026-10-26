import { useMemo, useState } from "react";
import { Badge, Card, CopyButton, Empty } from "./ui";
import { runQuery } from "../api";
import type { QueryResponse, Snapshot } from "../types";

const STARTER = "SELECT * FROM home LIMIT 100";

function cell(value: unknown) {
  if (value === null || value === undefined) return <span className="muted">null</span>;
  if (typeof value === "object") return JSON.stringify(value);
  return String(value);
}

function toCsv(columns: string[], rows: Record<string, unknown>[]): string {
  const escape = (value: unknown) => {
    if (value === null || value === undefined) return "";
    const text = typeof value === "object" ? JSON.stringify(value) : String(value);
    return /[",\n]/.test(text) ? `"${text.replace(/"/g, '""')}"` : text;
  };
  return [columns.join(","), ...rows.map((row) => columns.map((column) => escape(row[column])).join(","))].join(
    "\n",
  );
}

export function QueryView({ snapshot }: { snapshot: Snapshot }) {
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
  const [container, setContainer] = useState("");
  const [database, setDatabase] = useState("");
  const [language, setLanguage] = useState<"sql" | "influxql">("sql");
  const [statement, setStatement] = useState(STARTER);
  const [result, setResult] = useState<QueryResponse | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const selectedPod = pods.find((pod) => `${pod.namespace}/${pod.name}` === podKey);
  const containers = selectedPod?.containers.map((item) => item.name) ?? [];
  const canRun = Boolean(selectedPod) && database.trim().length > 0 && statement.trim().length > 0;

  async function run() {
    if (!selectedPod || !canRun) return;
    setLoading(true);
    setError(null);
    try {
      const response = await runQuery({
        namespace: selectedPod.namespace,
        pod: selectedPod.name,
        container: container || containers[0] || null,
        database: database.trim(),
        language,
        statement,
      });
      setResult(response);
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : String(cause));
      setResult(null);
    } finally {
      setLoading(false);
    }
  }

  const columns = result?.columns ?? [];
  const rows = result?.rows ?? [];

  return (
    <Card
      title="Query terminal"
      subtitle="Single read-only SELECT or SHOW statements, run inside the pod with your own credentials"
    >
      <div className="stack">
        <div className="toolbar">
          <select
            value={podKey}
            onChange={(event) => {
              setPodKey(event.target.value);
              setContainer("");
              setResult(null);
            }}
            aria-label="Select pod to query through"
            style={{ maxWidth: 380 }}
          >
            {pods.length === 0 && <option value="">No pods available</option>}
            {pods.map((pod) => (
              <option key={`${pod.namespace}/${pod.name}`} value={`${pod.namespace}/${pod.name}`}>
                {pod.namespace}/{pod.name}
                {pod.component ? ` · ${pod.component}` : ""}
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
          <input
            type="text"
            value={database}
            onChange={(event) => setDatabase(event.target.value)}
            placeholder="database name"
            aria-label="Database name"
            spellCheck={false}
            style={{ maxWidth: 200 }}
          />
          <select
            value={language}
            onChange={(event) => setLanguage(event.target.value as "sql" | "influxql")}
            aria-label="Query language"
          >
            <option value="sql">SQL</option>
            <option value="influxql">InfluxQL</option>
          </select>
          <button type="button" className="btn primary" onClick={run} disabled={!canRun || loading}>
            {loading ? <span className="spin">◌</span> : "Run"}
          </button>
          {result && rows.length > 0 && (
            <CopyButton value={toCsv(columns, rows)} label="Copy CSV" />
          )}
        </div>

        <textarea
          className="query-editor"
          value={statement}
          onChange={(event) => setStatement(event.target.value)}
          onKeyDown={(event) => {
            if (event.key === "Enter" && (event.metaKey || event.ctrlKey)) {
              event.preventDefault();
              void run();
            }
          }}
          aria-label="Query statement"
          spellCheck={false}
          rows={4}
          placeholder="SELECT * FROM home LIMIT 100"
        />

        {error && <div className="error-banner">{error}</div>}
        {result?.error && <div className="error-banner">influxdb3: {result.error}</div>}

        {result ? (
          <>
            <div className="row" style={{ fontSize: 11.5 }}>
              <Badge tone="neutral">{result.database}</Badge>
              <Badge tone="neutral">{result.language === "sql" ? "SQL" : "InfluxQL"}</Badge>
              <span className="muted">
                {result.row_count === 1 ? "1 row" : `${result.row_count} rows`}
              </span>
              {result.truncated && <Badge tone="warn">truncated to {result.row_count}</Badge>}
              {result.redacted && <Badge tone="warn">token redacted</Badge>}
            </div>
            {columns.length > 0 ? (
              <div className="table-wrap">
                <table className="data">
                  <thead>
                    <tr>
                      {columns.map((column) => (
                        <th key={column} className="nosort">
                          {column}
                        </th>
                      ))}
                    </tr>
                  </thead>
                  <tbody>
                    {rows.map((row, index) => (
                      <tr key={index}>
                        {columns.map((column) => (
                          <td
                            key={column}
                            className={typeof row[column] === "number" ? "mono num" : "mono"}
                          >
                            {cell(row[column])}
                          </td>
                        ))}
                      </tr>
                    ))}
                  </tbody>
                </table>
              </div>
            ) : (
              <Empty glyph="⌗">
                {result.error ? "The statement did not return rows." : "The statement returned no rows."}
              </Empty>
            )}
          </>
        ) : (
          <Empty glyph="⌗">
            Enter a database name and a single read-only statement, then run it. Nothing is written to the
            cluster.
          </Empty>
        )}

        <p className="muted" style={{ margin: 0, fontSize: 11.5 }}>
          The console only accepts one <code>SELECT</code> or <code>SHOW</code> statement and refuses{" "}
          <code>INTO</code>, so it cannot modify data. It still needs <code>pods/exec</code> through your
          kubeconfig, and the admin token is passed to the pod on stdin, never in the argument list. Pair
          it with a read-scoped database token. Requires <code>--query</code>; the rest of the console
          stays read-only without it.
        </p>
      </div>
    </Card>
  );
}
