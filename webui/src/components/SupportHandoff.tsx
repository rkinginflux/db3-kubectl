import { useMemo, useState } from "react";
import type { Snapshot } from "../types";

/**
 * Documented support entry point (InfluxData Support Policy §1.3.1). The portal
 * covers both new and existing tickets, so no case id has to be known up front
 * and no Salesforce credentials are ever held by this plugin.
 */
export const SUPPORT_PORTAL_URL = "https://support.influxdata.com";

export interface SupportHandoffProps {
  snapshot: Snapshot;
  pod: string;
  container: string | null;
  tail: number;
  logs: string;
  redacted: boolean;
}

/**
 * Salesforce case severities. Sev3 is the default because the console is used
 * for routine troubleshooting; a genuine outage is promoted by the customer.
 * Sev1 must also be phoned in (InfluxData Support Policy §1.3.4), which is why
 * the UI says so rather than implying the ticket alone is enough.
 */
export const SEVERITIES = ["Sev1", "Sev2", "Sev3", "Sev4"] as const;
export const DEFAULT_SEVERITY = "Sev3";
export type Severity = (typeof SEVERITIES)[number];

export function supportTemplate(
  { snapshot, pod, container, tail, logs, redacted }: SupportHandoffProps,
  severity: Severity = DEFAULT_SEVERITY,
): string {
  const cluster = snapshot.cluster ?? { server: "", version: "", platform: "" };
  const header = [
    "InfluxDB 3 Enterprise support request",
    "",
    `Cluster:     ${cluster.server || "unknown"} (Kubernetes ${cluster.version || "version unknown"})`,
    `Context:     ${snapshot.context || "current kubeconfig context"}`,
    `Pod:         ${pod}${container ? ` / ${container}` : ""}`,
    `Log window:  last ${tail} lines`,
    `Collected:   ${snapshot.generated_at}`,
    `Tool:        kubectl-db3 ${snapshot.tool_version}`,
    `Redaction:   ${redacted ? "known admin token removed" : "no known token patterns found (review before sharing)"}`,
    "",
    `Severity:    ${severity}`,
    "",
    "What is happening:",
    "",
    "When it started:",
    "",
    "What changed:",
    "",
    "--- pod logs ---",
  ].join("\n");
  const body = logs.replace(/\s+$/, "") || "(no log output)";
  return `${header}\n${body}\n`;
}

/**
 * Copy-for-support affordance. The console itself uploads nothing: this copies
 * the already-redacted log text to the clipboard and the customer pastes it into
 * a case. The only navigation is a plain link the customer clicks.
 */
export function SupportHandoff(props: SupportHandoffProps) {
  const { logs, pod, container, tail, redacted, snapshot } = props;
  const [state, setState] = useState<"idle" | "copied" | "failed">("idle");
  const [severity, setSeverity] = useState<Severity>(DEFAULT_SEVERITY);

  const text = useMemo(
    () => supportTemplate({ snapshot, pod, container, tail, logs, redacted }, severity),
    [snapshot, pod, container, tail, logs, redacted, severity],
  );

  async function copyForSupport() {
    try {
      await navigator.clipboard.writeText(text);
      setState("copied");
    } catch {
      setState("failed");
    }
    window.setTimeout(() => setState("idle"), 2400);
  }

  const label =
    state === "copied" ? "✓ Copied for support" : state === "failed" ? "Copy failed — select the text" : "Copy for support";

  return (
    <div className="stack" style={{ gap: 8 }}>
      <div className="row">
        <button type="button" className="btn primary small" onClick={copyForSupport}>
          {label}
        </button>
        <a className="btn ghost small" href={SUPPORT_PORTAL_URL} target="_blank" rel="noopener noreferrer">
          Open InfluxData support ↗
        </a>
        <label className="row" style={{ gap: 6, fontSize: 11.5 }}>
          <span className="muted">Severity</span>
          <select
            value={severity}
            onChange={(event) => setSeverity(event.target.value as Severity)}
            aria-label="Case severity"
          >
            {SEVERITIES.map((value) => (
              <option key={value} value={value}>
                {value}
              </option>
            ))}
          </select>
        </label>
      </div>
      <p className="muted" style={{ margin: 0, fontSize: 11.5 }}>
        Copies these logs plus a short header for the case. Nothing is uploaded by this console: paste it into a new
        or existing ticket at <code>support.influxdata.com</code> (Sev1 also has to be phoned in). Review the text
        before sharing.
      </p>
    </div>
  );
}
