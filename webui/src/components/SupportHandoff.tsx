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
 * The text a customer pastes into a ticket: triage metadata first, so support
 * can route the case before reading the log body.
 */
export function supportTemplate({
  snapshot,
  pod,
  container,
  tail,
  logs,
  redacted,
}: SupportHandoffProps): string {
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

  const text = useMemo(
    () => supportTemplate({ snapshot, pod, container, tail, logs, redacted }),
    [snapshot, pod, container, tail, logs, redacted],
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
      </div>
      <p className="muted" style={{ margin: 0, fontSize: 11.5 }}>
        Copies these logs plus a short header for the case. Nothing is uploaded by this console: paste it into a new
        or existing ticket at <code>support.influxdata.com</code>. Review the text before sharing.
      </p>
    </div>
  );
}
