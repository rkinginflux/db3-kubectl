import { expect, test } from "vitest";
import { readFileSync } from "node:fs";
const css = readFileSync(`${process.cwd()}/src/styles.css`, "utf8");
function luminance(rgb: number[]) {
  return rgb.map(n => n / 255).map(n => n <= 0.04045 ? n / 12.92 : ((n + 0.055) / 1.055) ** 2.4).reduce((sum, n, i) => sum + n * [0.2126, 0.7152, 0.0722][i], 0);
}
test("light success badges have WCAG AA text contrast on their tinted background", () => {
  const light = css.split(':root[data-theme="light"]')[1].split("}")[0];
  const color = light.match(/--ok: #([\da-f]{6})/)![1].match(/../g)!.map(n => parseInt(n, 16));
  // Parent observed actual role-card badge background #e8f2ed.
  expect((luminance([232, 242, 237]) + 0.05) / (luminance(color) + 0.05)).toBeGreaterThanOrEqual(4.5);
});
test("tablet cards stack key-value rows rather than fragmenting URLs", () => {
  const tablet = css.split("@media (max-width: 980px)")[1].split("@media")[0];
  expect(tablet).toMatch(/\.kv\s*\{[^}]*grid-template-columns:\s*1fr/);
});

test("narrow layouts stack key-value rows and retain touch-sized theme/menu controls", () => {
  expect(css).toMatch(/@media\s*\(max-width:\s*600px\)[\s\S]*\.kv\s*\{[^}]*grid-template-columns:\s*1fr/);
  expect(css).toMatch(/min-height:\s*44px/);
});
