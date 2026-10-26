import { execFileSync } from "node:child_process";
import { readFileSync, readdirSync } from "node:fs";
import { createHash } from "node:crypto";
import assert from "node:assert/strict";
const before = readFileSync("dist/index.html");
for (let pass = 0; pass < 2; pass++) {
  execFileSync(process.execPath, ["node_modules/vite/bin/vite.js", "build"], { stdio: "inherit" });
  assert.deepEqual(readdirSync("dist"), ["index.html"], "runtime must be a single embedded HTML file");
  assert.deepEqual(readFileSync("dist/index.html"), before, "dist must match source and rebuild byte-for-byte");
}
const html = before.toString();
assert(html.includes('__DB3_SESSION__'));
assert(!/<script[^>]+src=/i.test(html), "no runtime external scripts");
for (const name of ["LICENSE.md", "THIRD_PARTY_NOTICES.md"]) {
  const text = readFileSync(`../${name}`, "utf8").replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;");
  assert(html.includes(text), `${name} must be preserved in the embedded legal template`);
}
console.log(`Reproducible singlefile SHA-256 ${createHash("sha256").update(before).digest("hex")}`);
