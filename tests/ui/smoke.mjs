// Interface smoke test: a hub serving the built interface, the fixture
// project, every view opened in headless Chromium at laptop size in both
// themes, no errors in the console, a screenshot of each view.
//
//   node tests/ui/smoke.mjs [out-dir]
// Needs: target/release/gamma, app/ui/dist, and Chromium (CHROMIUM=/path or
// `npx playwright install chromium`).
import { chromium } from "../../app/ui/node_modules/playwright-core/index.mjs";
import { spawn } from "node:child_process";
import { mkdtempSync, readFileSync, existsSync, mkdirSync, cpSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";

const root = resolve(new URL("../..", import.meta.url).pathname);
const out = resolve(process.argv[2] || join(root, "target", "ui-smoke"));
mkdirSync(out, { recursive: true });
const work = mkdtempSync(join(tmpdir(), "gamma-ui-"));
// GAMMA_UI_PROJECT: another (built) project to capture instead of the fixture
const src = process.env.GAMMA_UI_PROJECT || join(root, "tests/fixtures/wire");
const project = join(work, "project");
cpSync(src, project, { recursive: true, filter: (p) => !p.includes("/.gamma/local") && !p.includes("/results/") });
const env = { ...process.env, GAMMA_RUNTIME_DIR: join(work, "rt"), GAMMA_DATA_DIR: join(work, "data"), GAMMA_CONFIG_DIR: join(work, "cfg") };
const gamma = join(root, "target/release/gamma");
const tHub = Date.now();
const hub = spawn(gamma, ["hub", "--port", "0", "--ui", join(root, "app/ui/dist")], { env, stdio: "ignore" });
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
let info;
for (let i = 0; i < 100 && !info; i++) { await sleep(100); if (existsSync(join(work, "rt/hub.json"))) info = JSON.parse(readFileSync(join(work, "rt/hub.json"), "utf8")); }
if (!info) throw new Error("hub did not start");
const hubStart = Date.now() - tHub;
const base = `http://127.0.0.1:${info.port}`;
const api = (p, body) => fetch(base + p, { method: body ? "POST" : "GET", headers: { Authorization: `Bearer ${info.token}`, "Content-Type": "application/json" }, body: body ? JSON.stringify(body) : undefined }).then((r) => r.json());
const opened = await api("/api/open", { path: project });
// one finished run so the results view has data
const sims = await api(`/api/p/${opened.id}/sims`);
const job = await api(`/api/p/${opened.id}/sims/${sims[0].name}/run`, { device: "cpu", preview: !!process.env.GAMMA_UI_PREVIEW });
for (let i = 0; i < 600; i++) { const j = await api(`/api/jobs/${job.id}`); if (["done", "failed"].includes(j.status)) { if (j.status !== "done") throw new Error(j.error); break; } await sleep(300); }

await api(`/api/p/${opened.id}/checkpoints`, { message: "First run of the fixture" });
await api(`/api/p/${opened.id}/comments`, { text: "Transmission looks as expected for a straight wire.", object: "monitor:wire/through" });

const browser = await chromium.launch({ executablePath: process.env.CHROMIUM || undefined, args: ["--use-gl=angle", "--use-angle=swiftshader", "--enable-unsafe-swiftshader"] });
const errors = [];
const timings = [];
let failed = false;
for (const theme of ["dark", "light"]) {
  const page = await browser.newPage({ viewport: { width: 1440, height: 900 }, deviceScaleFactor: 2 });
  page.on("console", (m) => { if (m.type() === "error") errors.push(`${theme}: ${m.text()}`); });
  page.on("pageerror", (e) => errors.push(`${theme}: ${e.message}`));
  page.on("response", (r) => { if (r.status() >= 400) errors.push(`${theme}: ${r.status()} ${r.url().replace(/token=[0-9a-f]+/, "token=…")}`); });
  await page.addInitScript((t) => { localStorage.setItem("gamma-theme", t); localStorage.setItem("gamma-tab", "layout"); }, theme);
  const tLoad = Date.now();
  await page.goto(`${base}/?token=${info.token}&project=${encodeURIComponent(project)}`);
  await page.waitForSelector("nav.tree .row", { timeout: 15000 });
  await page.waitForFunction(() => document.querySelector("#layout-meta")?.textContent, null, { timeout: 15000 }).catch(() => {});
  timings.push(`${theme}: project open and layout drawn ${Date.now() - tLoad} ms`);
  for (const [key, name] of [["1", "layout"], ["2", "3d"], ["3", "setup"], ["4", "run"], ["5", "results"], ["6", "circuit"], ["7", "history"]]) {
    await page.keyboard.press(key);
    await sleep(name === "setup" || name === "results" ? 2500 : 1200);
    const sel = await page.getAttribute(`nav.tabs a[data-tab="${name}"]`, "aria-selected");
    if (sel !== "true") { failed = true; console.log(`FAIL ${theme} ${name}: tab not selected`); }
    await page.screenshot({ path: join(out, `${name}-${theme}.png`) });
  }
  await page.click("header.top >> text=Settings");
  await sleep(600);
  await page.screenshot({ path: join(out, `settings-${theme}.png`) });
  await page.keyboard.press("Escape");
  await page.click(".modal", { position: { x: 5, y: 5 } }).catch(() => {});
  await sleep(200);
  // select a port from the tree and check the inspector follows
  await page.keyboard.press("1");
  await page.click("nav.tree >> text=o1");
  await sleep(300);
  const ref = await page.textContent(".inspector .ref");
  if (ref?.trim() !== "port:o1") { failed = true; console.log(`FAIL ${theme}: inspector shows ${ref}`); }
  await page.close();
}
await browser.close();
hub.kill();
const real = errors.filter((e) => !/favicon/.test(e));
if (real.length) { failed = true; console.log("console errors:\n" + real.join("\n")); }
console.log(`hub start ${hubStart} ms; ${timings.join("; ")}`);
console.log(failed ? "UI SMOKE TEST FAILED" : `UI smoke test passed; screenshots in ${out}`);
process.exit(failed ? 1 : 0);
