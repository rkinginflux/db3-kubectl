import { useCallback, useEffect, useRef, useState } from "react";
import { fetchSnapshot } from "../api";
import { useTheme } from "../theme";
import { relativeTime } from "../format";
import { ConfigView } from "./ConfigView";
import { Db3View } from "./Db3View";
import { EventsView } from "./EventsView";
import { HelmView } from "./HelmView";
import { KubernetesView } from "./KubernetesView";
import { LogsView } from "./LogsView";
import { Overview } from "./Overview";
import { Topology } from "./Topology";
import type { Snapshot } from "../types";

type ViewKey = "overview" | "topology" | "db3" | "kubernetes" | "helm" | "config" | "events" | "logs";

interface NavItem {
  key: ViewKey;
  label: string;
  icon: string;
  group: string;
}

const NAV: NavItem[] = [
  { key: "overview", label: "Overview", icon: "◈", group: "Environment" },
  { key: "topology", label: "Topology", icon: "⬡", group: "Environment" },
  { key: "db3", label: "InfluxDB 3", icon: "◍", group: "Environment" },
  { key: "kubernetes", label: "Kubernetes", icon: "☸", group: "Environment" },
  { key: "helm", label: "Helm", icon: "⎈", group: "Configuration" },
  { key: "config", label: "Config & secrets", icon: "▤", group: "Configuration" },
  { key: "events", label: "Events", icon: "◔", group: "Operations" },
  { key: "logs", label: "Logs", icon: "≋", group: "Operations" },
];

const TITLES: Record<ViewKey, { title: string; subtitle: string }> = {
  overview: { title: "Overview", subtitle: "Readiness, findings and cluster identity" },
  topology: { title: "Topology", subtitle: "Nodes, database workloads and service routing" },
  db3: { title: "InfluxDB 3", subtitle: "Clusters, roles, pods and licensing" },
  kubernetes: { title: "Kubernetes", subtitle: "Nodes, workloads, services, storage and policies" },
  helm: { title: "Helm", subtitle: "Releases, chart pins and revision state" },
  config: { title: "Config & secrets", subtitle: "ConfigMaps, opt-in Secret metadata, and masked container environment" },
  events: { title: "Events", subtitle: "Recent warnings and lifecycle messages" },
  logs: { title: "Logs", subtitle: "Bounded on-demand pod log tails" },
};

export function App() {
  const { theme, toggle } = useTheme();
  const [view, setView] = useState<ViewKey>(() => {
    const hash = window.location.hash.replace("#", "");
    return (NAV.find((item) => item.key === hash)?.key ?? "overview") as ViewKey;
  });
  const [snapshot, setSnapshot] = useState<Snapshot | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);
  const [refreshing, setRefreshing] = useState(false);
  const [intervalSeconds, setIntervalSeconds] = useState(30);
  const [navOpen, setNavOpen] = useState(false);
  const inFlight = useRef(false);

  useEffect(() => {
    window.location.hash = view;
  }, [view]);

  useEffect(() => {
    function onHashChange() {
      const hash = window.location.hash.replace("#", "");
      const next = NAV.find((item) => item.key === hash);
      if (next) setView(next.key);
    }
    window.addEventListener("hashchange", onHashChange);
    return () => window.removeEventListener("hashchange", onHashChange);
  }, []);

  const load = useCallback(
    async (force: boolean) => {
      if (inFlight.current) return;
      inFlight.current = true;
      if (!snapshot) setLoading(true);
      else setRefreshing(true);
      setError(null);
      try {
        const next = await fetchSnapshot({ namespace: null, refresh: force });
        setSnapshot(next);
      } catch (cause) {
        setError(cause instanceof Error ? cause.message : String(cause));
      } finally {
        inFlight.current = false;
        setLoading(false);
        setRefreshing(false);
      }
    },
    [snapshot],
  );

  useEffect(() => {
    void load(true);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  useEffect(() => {
    if (!intervalSeconds) return;
    const timer = window.setInterval(() => void load(false), intervalSeconds * 1000);
    return () => window.clearInterval(timer);
  }, [intervalSeconds, load]);

  useEffect(() => {
    function onKey(event: KeyboardEvent) {
      if (event.key === "Escape") setNavOpen(false);
      const target = event.target as HTMLElement | null;
      const typing = target && ["INPUT", "SELECT", "TEXTAREA"].includes(target.tagName);
      if (typing) return;
      if (event.key === "t" && !event.metaKey && !event.ctrlKey) toggle();
      if (event.key === "r" && !event.metaKey && !event.ctrlKey) void load(true);
    }
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [load, toggle]);

  const groups = [...new Set(NAV.map((item) => item.group))];
  const meta = TITLES[view];

  return (
    <div className="shell">
      <aside className={`sidebar ${navOpen ? "open" : ""}`} aria-label="Sections">
        <div className="brand">
          <div className="brand-mark">db3</div>
          <div style={{ display: "grid" }}>
            <span className="brand-title">Environment Console</span>
            <span className="brand-sub">kubectl db3 webui</span>
          </div>
        </div>

        {groups.map((group) => (
          <div className="nav-group" key={group}>
            <span className="nav-label">{group}</span>
            {NAV.filter((item) => item.group === group).map((item) => (
              <button
                key={item.key}
                type="button"
                className={`nav-item ${view === item.key ? "active" : ""}`}
                onClick={() => {
                  setView(item.key);
                  setNavOpen(false);
                }}
                aria-current={view === item.key ? "page" : undefined}
              >
                <span className="nav-icon" aria-hidden="true">
                  {item.icon}
                </span>
                {item.label}
                {item.key === "events" && snapshot && snapshot.events.filter((event) => event.type === "Warning").length > 0 && (
                  <span className="nav-count">{snapshot.events.filter((event) => event.type === "Warning").length}</span>
                )}
                {item.key === "db3" && snapshot && snapshot.db3.length > 0 && <span className="nav-count">{snapshot.db3.length}</span>}
              </button>
            ))}
          </div>
        ))}

        <div className="sidebar-footer">
          <div className="legend-row">
            <span className="dot ok" /> healthy
            <span className="dot warn" style={{ marginLeft: 8 }} /> degraded
            <span className="dot danger" style={{ marginLeft: 8 }} /> failing
          </div>
          <span className="muted" style={{ fontSize: 11, lineHeight: 1.5 }}>
            Local read-only server. Closing this command stops the console.
          </span>
        </div>
      </aside>

      <div className="main">
        <header className="topbar">
          <button
            type="button"
            className="btn ghost small"
            onClick={() => setNavOpen((open) => !open)}
            aria-label="Toggle navigation"
            style={{ display: "none" }}
            id="nav-toggle"
          >
            ☰
          </button>
          <div style={{ display: "grid", gap: 1, minWidth: 0 }}>
            <h1>{meta.title}</h1>
            <span className="crumb">{meta.subtitle}</span>
          </div>
          <div className="topbar-spacer" />
          <div className="controls">
            {snapshot && (
              <>
                <span className="chip mono" title="kubectl context">
                  {snapshot.context ?? "default context"}
                </span>
                {snapshot.namespace_filter && (
                  <span className="chip" title="Collected scope">
                    ns {snapshot.namespace_filter}
                  </span>
                )}
                {snapshot.db3.length > 0 && (
                  <span className="chip" title="InfluxDB 3 pods in the snapshot">
                    <b>{snapshot.db3.reduce((sum, cluster) => sum + cluster.pod_count, 0)}</b> db3 pods
                  </span>
                )}
                <span className="chip" title={snapshot.generated_at}>
                  updated {relativeTime(snapshot.generated_at)}
                </span>
              </>
            )}
            <select
              value={intervalSeconds}
              onChange={(event) => setIntervalSeconds(Number(event.target.value))}
              aria-label="Auto refresh interval"
              title="Auto refresh"
            >
              <option value={0}>Auto off</option>
              <option value={15}>Every 15s</option>
              <option value={30}>Every 30s</option>
              <option value={60}>Every 60s</option>
            </select>
            <button type="button" className="btn" onClick={() => void load(true)} disabled={refreshing}>
              {refreshing ? <span className="spin">◌</span> : "↻"} Refresh
            </button>
            <button
              type="button"
              className="btn ghost"
              onClick={toggle}
              aria-label={`Switch to ${theme === "dark" ? "light" : "dark"} theme`}
              title={`Switch to ${theme === "dark" ? "light" : "dark"} theme (t)`}
            >
              {theme === "dark" ? "☀" : "☾"}
            </button>
          </div>
        </header>

        <main className="content content-wide">
          {error && (
            <div className="error-banner">
              {snapshot ? "Disconnected — cached observations are stale. " : "Could not read the local console. "}
              {error}. Check that the webui command is still running and kubectl can reach the cluster, then refresh.
            </div>
          )}

          {loading && !snapshot && (
            <>
              <div className="grid stats">
                {Array.from({ length: 6 }).map((_, index) => (
                  <div className="skeleton" key={index} style={{ height: 96 }} />
                ))}
              </div>
              <div className="skeleton" style={{ height: 320 }} />
            </>
          )}

          {snapshot && view === "overview" && <Overview snapshot={snapshot} />}
          {snapshot && view === "topology" && <Topology snapshot={snapshot} />}
          {snapshot && view === "db3" && <Db3View snapshot={snapshot} />}
          {snapshot && view === "kubernetes" && <KubernetesView snapshot={snapshot} />}
          {snapshot && view === "helm" && <HelmView snapshot={snapshot} />}
          {snapshot && view === "config" && <ConfigView snapshot={snapshot} />}
          {snapshot && view === "events" && <EventsView snapshot={snapshot} />}
          {snapshot && view === "logs" && <LogsView snapshot={snapshot} />}

          {!loading && !snapshot && !error && <div className="empty">No snapshot available.</div>}
          <footer className="muted" style={{ fontSize: 11, lineHeight: 1.6 }}>
            <p>InfluxDB® is a registered trademark of InfluxData Inc.</p>
            <p>Independent community tool; not endorsed by or affiliated with InfluxData.</p>
          </footer>
        </main>
      </div>
    </div>
  );
}
