import { useMemo, useState } from "react";
import type { ReactNode } from "react";
import { classNames } from "../format";

export type Tone = "ok" | "warn" | "danger" | "info" | "neutral" | "accent";

export function Badge({ tone = "neutral", children, title }: { tone?: Tone; children: ReactNode; title?: string }) {
  return (
    <span className={`badge ${tone}`} title={title}>
      {children}
    </span>
  );
}

export function Dot({ tone = "neutral" }: { tone?: Tone }) {
  return <span className={`dot ${tone}`} aria-hidden="true" />;
}

export function Card({
  title,
  subtitle,
  actions,
  children,
  tight,
  className,
}: {
  title?: ReactNode;
  subtitle?: ReactNode;
  actions?: ReactNode;
  children: ReactNode;
  tight?: boolean;
  className?: string;
}) {
  return (
    <section className={classNames("card", className)}>
      {(title || actions) && (
        <header className="card-head">
          <div style={{ display: "grid", gap: 1, minWidth: 0 }}>
            {title && <h2>{title}</h2>}
            {subtitle && <span className="sub">{subtitle}</span>}
          </div>
          <div style={{ flex: 1 }} />
          {actions}
        </header>
      )}
      <div className={classNames("card-body", tight && "tight")}>{children}</div>
    </section>
  );
}

export function Stat({
  label,
  value,
  note,
  accent,
}: {
  label: string;
  value: ReactNode;
  note?: ReactNode;
  accent?: "accent" | "ok" | "warn" | "danger";
}) {
  return (
    <div className={classNames("card stat", accent)}>
      <span className="stat-label">{label}</span>
      <span className="stat-value">{value}</span>
      {note && <span className="stat-note">{note}</span>}
    </div>
  );
}

export function Empty({ children, glyph = "◌" }: { children: ReactNode; glyph?: string }) {
  return (
    <div className="empty">
      <span className="glyph" aria-hidden="true">
        {glyph}
      </span>
      <span>{children}</span>
    </div>
  );
}

export function Meter({ ready, desired }: { ready: number; desired: number }) {
  const percent = desired > 0 ? Math.min(100, Math.round((ready / desired) * 100)) : ready > 0 ? 100 : 0;
  const tone = percent === 100 ? "ok" : percent === 0 ? "danger" : "warn";
  return (
    <div className={`meter ${tone === "ok" ? "" : tone}`} role="img" aria-label={`${ready} of ${desired} ready`}>
      <span style={{ width: `${percent}%` }} />
    </div>
  );
}

export function CopyButton({ value, label = "Copy" }: { value: string; label?: string }) {
  const [copied, setCopied] = useState(false);
  return (
    <button
      type="button"
      className="btn ghost small"
      onClick={async () => {
        try {
          await navigator.clipboard.writeText(value);
          setCopied(true);
          window.setTimeout(() => setCopied(false), 1200);
        } catch {
          setCopied(false);
        }
      }}
      title={`Copy ${label.toLowerCase()}`}
    >
      {copied ? "✓ copied" : label}
    </button>
  );
}

export interface Column<T> {
  key: string;
  label: string;
  render: (row: T) => ReactNode;
  sortValue?: (row: T) => string | number;
  align?: "left" | "right";
  mono?: boolean;
  width?: string;
  sortable?: boolean;
}

export function DataTable<T>({
  rows,
  columns,
  initialSort,
  searchText,
  searchPlaceholder = "Filter…",
  emptyMessage = "Nothing to show",
  pageSize = 60,
  rowKey,
}: {
  rows: T[];
  columns: Column<T>[];
  initialSort?: { key: string; direction: "asc" | "desc" };
  searchText?: (row: T) => string;
  searchPlaceholder?: string;
  emptyMessage?: ReactNode;
  pageSize?: number;
  rowKey: (row: T) => string;
}) {
  const [query, setQuery] = useState("");
  const [sort, setSort] = useState(initialSort ?? null);
  const [limit, setLimit] = useState(pageSize);

  const filtered = useMemo(() => {
    if (!query.trim() || !searchText) return rows;
    const needle = query.trim().toLowerCase();
    return rows.filter((row) => searchText(row).toLowerCase().includes(needle));
  }, [rows, query, searchText]);

  const sorted = useMemo(() => {
    if (!sort) return filtered;
    const column = columns.find((item) => item.key === sort.key);
    if (!column?.sortValue) return filtered;
    const factor = sort.direction === "asc" ? 1 : -1;
    return [...filtered].sort((a, b) => {
      const left = column.sortValue!(a);
      const right = column.sortValue!(b);
      if (typeof left === "number" && typeof right === "number") return (left - right) * factor;
      return String(left).localeCompare(String(right), undefined, { numeric: true }) * factor;
    });
  }, [filtered, sort, columns]);

  const visible = sorted.slice(0, limit);

  return (
    <div>
      {searchText && (
        <div className="toolbar" style={{ padding: "10px 12px", borderBottom: "1px solid var(--border)" }}>
          <input
            type="search"
            value={query}
            placeholder={searchPlaceholder}
            onChange={(event) => {
              setQuery(event.target.value);
              setLimit(pageSize);
            }}
            style={{ minWidth: 220 }}
            aria-label={searchPlaceholder}
          />
          <span className="muted" style={{ fontSize: 11.5 }}>
            {filtered.length} of {rows.length}
          </span>
        </div>
      )}
      {sorted.length === 0 ? (
        <Empty>{emptyMessage}</Empty>
      ) : (
        <div className="table-wrap">
          <table className="data">
            <thead>
              <tr>
                {columns.map((column) => {
                  const sortable = column.sortable !== false && Boolean(column.sortValue);
                  const active = sort?.key === column.key;
                  return (
                    <th
                      key={column.key}
                      className={classNames(!sortable && "nosort")}
                      style={{ width: column.width, textAlign: column.align === "right" ? "right" : "left" }}
                      onClick={() => {
                        if (!sortable) return;
                        setSort((current) => {
                          if (current?.key !== column.key) return { key: column.key, direction: "asc" };
                          return { key: column.key, direction: current.direction === "asc" ? "desc" : "asc" };
                        });
                      }}
                      aria-sort={active ? (sort?.direction === "asc" ? "ascending" : "descending") : "none"}
                    >
                      {column.label}
                      {active && <span className="arrow">{sort?.direction === "asc" ? "▲" : "▼"}</span>}
                    </th>
                  );
                })}
              </tr>
            </thead>
            <tbody>
              {visible.map((row) => (
                <tr key={rowKey(row)}>
                  {columns.map((column) => (
                    <td
                      key={column.key}
                      className={classNames(column.mono && "mono", column.align === "right" && "num")}
                      style={{ textAlign: column.align === "right" ? "right" : "left" }}
                    >
                      {column.render(row)}
                    </td>
                  ))}
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}
      {sorted.length > visible.length && (
        <div style={{ padding: 10, textAlign: "center" }}>
          <button className="btn small" type="button" onClick={() => setLimit((current) => current + pageSize)}>
            Show {Math.min(pageSize, sorted.length - visible.length)} more ({sorted.length - visible.length} hidden)
          </button>
        </div>
      )}
    </div>
  );
}
