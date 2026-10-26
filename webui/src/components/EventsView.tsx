import { useState } from "react";
import { Badge, Card, DataTable, Empty } from "./ui";
import type { Column } from "./ui";
import { formatTime, relativeTime } from "../format";
import type { EventRow, Snapshot } from "../types";

export function EventsView({ snapshot }: { snapshot: Snapshot }) {
  const [type, setType] = useState("all");
  const [namespace, setNamespace] = useState("all");
  const namespaces = [...new Set(snapshot.events.map((event) => event.namespace))].sort();

  const rows = snapshot.events.filter(
    (event) => (type === "all" || event.type === type) && (namespace === "all" || event.namespace === namespace),
  );
  const warnings = snapshot.events.filter((event) => event.type === "Warning").length;

  const columns: Column<EventRow>[] = [
    {
      key: "last",
      label: "Last seen",
      render: (event) => (
        <span className="nowrap" title={formatTime(event.last_seen)}>
          {relativeTime(event.last_seen)}
        </span>
      ),
      sortValue: (event) => event.last_seen ?? "",
      width: "10%",
    },
    {
      key: "type",
      label: "Type",
      render: (event) => <Badge tone={event.type === "Warning" ? "warn" : "info"}>{event.type}</Badge>,
      sortValue: (event) => event.type,
      width: "9%",
    },
    {
      key: "reason",
      label: "Reason",
      render: (event) => <span className="mono" style={{ fontSize: 11.5 }}>{event.reason}</span>,
      sortValue: (event) => event.reason,
      width: "14%",
    },
    {
      key: "object",
      label: "Object",
      render: (event) => (
        <div style={{ display: "grid", gap: 1 }}>
          <span className="mono" style={{ fontSize: 11 }}>{event.object}</span>
          <span className="muted" style={{ fontSize: 10.5 }}>{event.namespace}</span>
        </div>
      ),
      sortValue: (event) => event.object,
      width: "22%",
    },
    {
      key: "count",
      label: "Count",
      render: (event) => <span className="mono" style={{ fontSize: 11.5 }}>{event.count}</span>,
      align: "right",
      sortValue: (event) => event.count,
      width: "7%",
    },
    {
      key: "message",
      label: "Message",
      render: (event) => (
        <span style={{ fontSize: 11.5, color: "var(--text-soft)", display: "block", maxWidth: 620 }}>
          {event.message}
        </span>
      ),
      sortable: false,
    },
  ];

  return (
    <>
      <div className="toolbar">
        <div className="pill-group" role="group" aria-label="Event type">
          {[
            ["all", "All"],
            ["Warning", "Warnings"],
            ["Normal", "Normal"],
          ].map(([value, label]) => (
            <button key={value} type="button" className={type === value ? "active" : ""} onClick={() => setType(value)}>
              {label}
            </button>
          ))}
        </div>
        <select value={namespace} onChange={(event) => setNamespace(event.target.value)} aria-label="Filter event namespace">
          <option value="all">All namespaces</option>
          {namespaces.map((name) => (
            <option key={name} value={name}>
              {name}
            </option>
          ))}
        </select>
        <span className="chip">
          <b>{rows.length}</b> events
        </span>
        <span className="chip">
          <b>{warnings}</b> warnings in scope
        </span>
      </div>

      <Card title="Kubernetes events" subtitle="Newest first, as returned by the API server" tight>
        {rows.length === 0 ? (
          <Empty glyph="◔">No events matched this filter. Kubernetes expires events after about an hour by default.</Empty>
        ) : (
          <DataTable
            rows={[...rows].sort((a, b) => (b.last_seen ?? "").localeCompare(a.last_seen ?? ""))}
            columns={columns}
            rowKey={(event) => `${event.namespace}/${event.object}/${event.reason}/${event.last_seen}`}
            searchText={(event) => `${event.namespace} ${event.object} ${event.reason} ${event.message}`}
            searchPlaceholder="Search event messages…"
            emptyMessage="No events"
            pageSize={50}
          />
        )}
      </Card>
    </>
  );
}
