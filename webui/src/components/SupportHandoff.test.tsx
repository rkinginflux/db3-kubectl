import { beforeEach, expect, test, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { DEFAULT_SEVERITY, SUPPORT_PORTAL_URL, SupportHandoff, supportTemplate } from "./SupportHandoff";
import type { Snapshot } from "../types";

const snapshot = {
  generated_at: "2026-10-28T00:30:00Z",
  tool_version: "0.1.0",
  context: "prod-west",
  namespace_filter: null,
  cluster: { server: "https://k8s.example:6443", version: "v1.31.4", platform: "linux/amd64" },
} as unknown as Snapshot;

const baseProps = {
  snapshot,
  pod: "influxdb3-0",
  container: "influxdb3",
  tail: 500,
  logs: "2026-10-28T00:00:00Z ERROR write failed\n2026-10-28T00:00:01Z WARN retry scheduled\n",
  redacted: true,
};

const writeText = vi.fn().mockResolvedValue(undefined);
const fetchSpy = vi.fn();

beforeEach(() => {
  writeText.mockClear();
  fetchSpy.mockClear();
  Object.defineProperty(navigator, "clipboard", { value: { writeText }, configurable: true });
  vi.stubGlobal("fetch", fetchSpy);
});

test("template puts triage metadata before the log body", () => {
  const text = supportTemplate(baseProps);
  expect(text).toContain("https://k8s.example:6443");
  expect(text).toContain("Kubernetes v1.31.4");
  expect(text).toContain("prod-west");
  expect(text).toContain("influxdb3-0 / influxdb3");
  expect(text).toContain("last 500 lines");
  expect(text).toContain("kubectl-db3 0.1.0");
  expect(text).toContain("known admin token removed");
  // The log body comes last, after the triage header.
  expect(text.indexOf("ERROR write failed")).toBeGreaterThan(text.indexOf("--- pod logs ---"));
});

test("template states when no redaction matched, so the customer is warned", () => {
  const text = supportTemplate({ ...baseProps, redacted: false });
  expect(text).toContain("no known token patterns found");
  expect(text).toContain("review before sharing");
});

test("copying writes exactly the text that is displayed, nothing more", async () => {
  render(<SupportHandoff {...baseProps} />);
  await userEvent.click(screen.getByRole("button", { name: /copy for support/i }));
  expect(writeText).toHaveBeenCalledTimes(1);
  expect(writeText).toHaveBeenCalledWith(supportTemplate(baseProps));
});

test("severity defaults to Sev3", () => {
  expect(DEFAULT_SEVERITY).toBe("Sev3");
  expect(supportTemplate(baseProps)).toContain("Severity:    Sev3");
  expect(supportTemplate(baseProps)).not.toContain("Sev1");
});

test("a promoted severity is what gets copied", async () => {
  render(<SupportHandoff {...baseProps} />);
  await userEvent.selectOptions(screen.getByLabelText(/case severity/i), "Sev1");
  await userEvent.click(screen.getByRole("button", { name: /copy for support/i }));
  expect(writeText).toHaveBeenCalledWith(expect.stringContaining("Severity:    Sev1"));
  expect(writeText).not.toHaveBeenCalledWith(expect.stringContaining("Severity:    Sev3"));
});

test("the Sev1 phone-call requirement is stated in the UI", () => {
  render(<SupportHandoff {...baseProps} />);
  expect(screen.getByText(/Sev1 also has to be phoned in/i)).toBeInTheDocument();
});

test("the handoff performs no network request of its own", async () => {
  render(<SupportHandoff {...baseProps} />);
  await userEvent.click(screen.getByRole("button", { name: /copy for support/i }));
  // The console uploads nothing: copying must not reach the network.
  expect(fetchSpy).not.toHaveBeenCalled();
});

test("the portal link is the documented support URL and opens safely", () => {
  render(<SupportHandoff {...baseProps} />);
  const link = screen.getByRole("link", { name: /open influxdata support/i });
  expect(SUPPORT_PORTAL_URL).toBe("https://support.influxdata.com");
  expect(link).toHaveAttribute("href", "https://support.influxdata.com");
  expect(link).toHaveAttribute("target", "_blank");
  expect(link).toHaveAttribute("rel", "noopener noreferrer");
});

test("a clipboard failure asks the customer to copy manually instead of claiming success", async () => {
  writeText.mockRejectedValueOnce(new Error("denied"));
  render(<SupportHandoff {...baseProps} />);
  await userEvent.click(screen.getByRole("button", { name: /copy for support/i }));
  expect(await screen.findByRole("button", { name: /copy failed/i })).toBeInTheDocument();
});
