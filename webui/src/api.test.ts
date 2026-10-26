import { beforeEach, expect, test, vi } from "vitest";
import { fetchHealth, fetchLogs, fetchSnapshot, runQuery } from "./api";

beforeEach(() => {
  document.head.innerHTML = '<meta name="db3-session" content="test-session">';
  vi.stubGlobal("fetch", vi.fn().mockResolvedValue({ ok: true, json: async () => ({ ok: true }) }));
});
for (const [name, run] of [
  ["snapshot", () => fetchSnapshot({ namespace: null, refresh: true })],
  ["logs", () => fetchLogs("db3", "pod-0", null, 50)],
  ["health", () => fetchHealth()],
  [
    "query",
    () =>
      runQuery({
        namespace: "db3",
        pod: "pod-0",
        container: null,
        database: "mydb",
        language: "sql",
        statement: "SELECT 1",
      }),
  ],
] as const) {
  test(`${name} sends session only in request header`, async () => {
    await run();
    const [url, options] = vi.mocked(fetch).mock.calls[0];
    expect(url).not.toContain("test-session");
    expect(options?.headers).toMatchObject({ "X-DB3-Session": "test-session" });
    expect(options?.cache).toBe("no-store");
  });
}
test("fails closed before sending API requests without a bootstrap session", async () => {
  document.head.innerHTML = "";
  await expect(fetchHealth()).rejects.toThrow(/session/i);
  expect(fetch).not.toHaveBeenCalled();
});
test("surfaces HTTP failures", async () => {
  vi.mocked(fetch).mockResolvedValueOnce({ ok: false, status: 401, text: async () => "session expired" } as Response);
  await expect(fetchHealth()).rejects.toThrow("session expired");
});
