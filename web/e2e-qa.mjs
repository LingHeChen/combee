// E2E QA(BFF 模式):前置起真实 Combee + Next(BFF),走登录流程验证全栈。
// 用法:node e2e-qa.mjs [--keep]  (自动起 Combee:18090 + Next:3100)
import { chromium } from "playwright";
import { spawn, execSync } from "node:child_process";
import path from "node:path";
import fs from "node:fs";
import { fileURLToPath } from "node:url";

const here = path.dirname(fileURLToPath(import.meta.url));
const repoRoot = path.resolve(here, "..");
const COMBEE_PORT = 18091;
const NEXT_PORT = 3300;
const COMBEE_BASE = `http://127.0.0.1:${COMBEE_PORT}`;
const BASE = `http://127.0.0.1:${NEXT_PORT}`;
const OUT = "/tmp/web-shots";
fs.mkdirSync(OUT, { recursive: true });

const children = [];
async function waitHttp(url, timeoutMs = 40_000) {
  const deadline = Date.now() + timeoutMs;
  for (;;) {
    try {
      const r = await fetch(url);
      if (r.status < 500) return;
    } catch {
      /* retry */
    }
    if (Date.now() > deadline) throw new Error(`timeout waiting ${url}`);
    await new Promise((r) => setTimeout(r, 400));
  }
}

async function ensureCombee() {
  try {
    await waitHttp(`${COMBEE_BASE}/v1/databases`, 2000);
    return;
  } catch {
    /* not running -> spawn */
  }
  execSync("cargo build -p combee-api-server", { cwd: repoRoot, stdio: "inherit" });
  const dir = path.join(repoRoot, "target/.e2e-bff-data");
  fs.rmSync(dir, { recursive: true, force: true });
  const child = spawn(path.join(repoRoot, "target/debug/combee-api-server"), [], {
    env: {
      ...process.env,
      COMBEE_BIND_ADDR: `127.0.0.1:${COMBEE_PORT}`,
      COMBEE_DATA_DIR: dir,
      COMBEE_AUTH: "off",
      COMBEE_USAGE_FLUSH_INTERVAL_SECS: "1",
      COMBEE_ADMIN_TOKEN: "e2e-admin",
    },
    stdio: "ignore",
  });
  children.push(child);
  await waitHttp(`${COMBEE_BASE}/v1/databases`);
}

async function ensureNext() {
  try {
    await waitHttp(`${BASE}/zh/login`, 2000);
    return;
  } catch {
    /* spawn */
  }
  execSync("npm run build", { cwd: here, env: { ...process.env, COMBEE_API_URL: COMBEE_BASE }, stdio: "inherit" });
  const child = spawn("npx", ["next", "start", "-p", String(NEXT_PORT)], {
    cwd: here,
    env: { ...process.env, COMBEE_API_URL: COMBEE_BASE },
    stdio: "ignore",
  });
  children.push(child);
  await waitHttp(`${BASE}/zh/login`);
}

await ensureCombee();
await ensureNext();

// Closed Alpha:生成一个邀请码(1000 Credits)供注册使用
async function generateInvite() {
  const res = await fetch(`${COMBEE_BASE}/admin/vouchers/generate`, {
    method: "POST",
    headers: { "content-type": "application/json", "x-admin-token": "e2e-admin" },
    body: JSON.stringify({ amount_units: 1_000_000_000, count: 1, campaign: "e2e" }),
  });
  const text = await res.text();
  if (!res.ok) throw new Error(`invite gen failed ${res.status}: ${text}`);
  const data = JSON.parse(text);
  if (!data.codes?.length) throw new Error(`invite gen empty: ${text.slice(0, 200)}`);
  return data.codes[0].code;
}
const INVITE_CODE = await generateInvite();

const results = [];
const ok = (name, cond, extra = "") => {
  results.push({ name, pass: Boolean(cond) });
  console.log(`${cond ? "PASS" : "FAIL"}  ${name}${extra ? ` — ${extra}` : ""}`);
};

const browser = await chromium.launch({
  executablePath: path.join(here, "node_modules/playwright-core/.local-browsers/chromium_headless_shell-1234/chrome-headless-shell-mac-arm64/chrome-headless-shell"),
});
const ctx = await browser.newContext({ viewport: { width: 1440, height: 900 }, permissions: ["clipboard-write"] });
const page = await ctx.newPage();
const consoleErrors = [];
page.on("console", (m) => { if (m.type() === "error") { consoleErrors.push(m.text()); if (m.text().includes("405")) console.log("405-CONSOLE:", m.text().slice(0, 160)); } });
page.on("response", (r) => { if (r.status() === 405) console.log("405-URL:", r.url()); });
page.on("pageerror", (e) => consoleErrors.push(String(e)));

const TS = Date.now();
const USERNAME = `qa-${TS}`;
const PASSWORD = "password123";

// ---- 0. 未登录跳转 ----
await page.goto(`${BASE}/overview`, { waitUntil: "networkidle" });
ok("unauthenticated redirects to /zh/login", page.url().includes("/zh/login"), page.url());

// ---- 1. 注册(用户名+密码;自动登录)----
await page.goto(`${BASE}/register`, { waitUntil: "networkidle" });
await page.getByTestId("register-username").fill(USERNAME);
await page.getByTestId("register-password").fill(PASSWORD);
await page.getByTestId("register-confirm").fill(PASSWORD);
await page.getByTestId("register-access-code").fill(INVITE_CODE);
await page.getByTestId("register-submit").click();
// 注册完成页:一次性 API key 展示(不再直接跳 Dashboard)
await page.waitForSelector('[data-testid="api-key-value"]', { timeout: 12_000 });
ok("register shows api key once", await page.getByTestId("api-key-value").isVisible());
const apiKeyText = await page.getByTestId("api-key-value").innerText();
ok("api key format cmb_sk_", apiKeyText.startsWith("cmb_sk_"), apiKeyText.slice(0, 12));
await page.getByTestId("copy-api-key").click();
await page.getByTestId("go-dashboard").click();
await page.waitForURL("**/zh/overview", { timeout: 10_000 });
ok("register navigates to /zh/overview", page.url().includes("/zh/overview"));
// 注册成功 → 用户初始 Credits(1000 Alpha Credits)
await page.goto(`${BASE}/credits`, { waitUntil: "networkidle" });
await page.waitForSelector('[data-testid="balance-card"]', { timeout: 10_000 });
const balanceText = await page.getByTestId("balance-card").innerText();
ok("invite credits granted (1000)", balanceText.includes("1000.00"), balanceText.slice(0, 60));

// ---- 2. Overview(BFF 聚合真实 Combee 数据)----
await page.goto(`${BASE}/overview`, { waitUntil: "networkidle" });
await page.waitForSelector("h2", { timeout: 10_000 }).catch(() => undefined);
const ovBody = (await page.locator("body").innerText().catch(() => "")).slice(0, 140).replace(/\n/g, " ");
ok("overview loads (zh)", await page.locator("h2", { hasText: "概览" }).first().isVisible(), ovBody);
ok("stat cards render", await page.getByText("请求数").first().isVisible());
ok("recent cells table", await page.getByTestId("recent-cells-table").isVisible());
await page.screenshot({ path: `${OUT}/bff-overview.png` });

// ---- 3. Cells(BFF 代理)----
await page.goto(`${BASE}/cells`, { waitUntil: "networkidle" });
ok("cells page renders", await page.getByTestId("cells-search").isVisible());
ok("cells grid renders (live data)", (await page.getByTestId("cell-card").count()) >= 1);
await page.screenshot({ path: `${OUT}/bff-cells.png` });

// ---- 4. 创建 Cell(BFF 代理写)----
await page.goto(`${BASE}/cells/new`, { waitUntil: "networkidle" });
await page.getByTestId("cell-name-input").fill("e2e-cell");
await page.getByTestId("create-cell-submit").click();
try {
  await page.waitForURL(/\/cells\/[0-9a-f-]{36}$/, { timeout: 8_000 });
} catch {
  /* fall through to report url */
}
ok("create cell navigates to detail", /\/cells\/[0-9a-f-]{36}$/.test(page.url()), page.url() + " | " + (await page.locator("body").innerText()).slice(0, 120).replace(/\n/g, " "));

// ---- 5. Cell 详情(SQL 高亮等)----
await page.waitForSelector('[data-testid="cell-tabs"]', { timeout: 10_000 });
ok("cell detail tabs", (await page.getByRole("tab").count()) === 7);
// KV 浏览:set 一个 key → 浏览模式能看到
await page.getByRole("tab", { name: "KV" }).click();
await page.waitForTimeout(600);
await page.getByTestId("kv-mode-operate").click();
await page.getByTestId("kv-key").fill("e2e:probe");
await page.getByTestId("kv-value").fill("hello");
await page.getByTestId("kv-set").click();
await page.waitForSelector('[data-testid="kv-result"]', { timeout: 6000 });
await page.getByTestId("kv-mode-browse").click();
await page.waitForTimeout(800);
const kvBrowseText = await page.getByTestId("kv-browse").innerText();
ok("kv browse lists key", kvBrowseText.includes("e2e:probe"), kvBrowseText.slice(0, 60));
await page.getByRole("tab", { name: "SQL" }).click();
ok("sql editor highlighting", (await page.locator(".sql-overlay .token.keyword").count()) > 0);
// 真实 SQL 执行:建表 → 插入 → 查询
const editor = page.getByTestId("sql-editor").locator("textarea");
await editor.fill("CREATE TABLE qa_t (id INTEGER PRIMARY KEY, name TEXT)");
await page.getByTestId("sql-run").click();
await page.waitForSelector('[data-testid="sql-result"]', { timeout: 8000 });
await editor.fill("INSERT INTO qa_t (name) VALUES ('a'), ('b')");
await page.getByTestId("sql-run").click();
await page.waitForTimeout(800);
await editor.fill("SELECT * FROM qa_t");
await page.getByTestId("sql-run").click();
await page.waitForSelector('[data-testid="sql-result"]', { timeout: 8000 });
const sqlText = await page.getByTestId("sql-result").innerText();
ok("sql real result rows", sqlText.includes("2 行") || sqlText.includes("2 rows"), sqlText.slice(0, 60));
await page.screenshot({ path: `${OUT}/bff-cell.png` });

// ---- 6. Usage / Credits(BFF 代理 + 聚合)----
await page.goto(`${BASE}/usage`, { waitUntil: "networkidle" });
ok("usage page renders", await page.locator("h2", { hasText: "用量" }).first().isVisible());
await page.goto(`${BASE}/credits`, { waitUntil: "networkidle" });
ok("credits balance card", await page.getByTestId("balance-card").isVisible());
await page.screenshot({ path: `${OUT}/bff-credits.png` });

// ---- 7. Account(Profile / Onboarding / Snippets / Activity)----
await page.goto(`${BASE}/account`, { waitUntil: "networkidle" });
ok("profile card", await page.getByTestId("profile-card").isVisible());
await page.getByTestId("profile-display-name").fill("QA Tester");
await page.getByTestId("profile-save").click();
await page.waitForSelector('[data-testid="profile-saved"]', { timeout: 5000 });
ok("profile saved", await page.getByTestId("profile-saved").isVisible());
await page.getByRole("tab", { name: "新手引导" }).click();
ok("onboarding steps", (await page.getByTestId("onboarding-step").count()) === 3);
await page.getByRole("tab", { name: "SQL 片段" }).click();
await page.getByTestId("snippet-title").fill("qa snippet");
await page.getByTestId("snippet-sql").fill("SELECT 1");
await page.getByTestId("snippet-add").click();
await page.waitForSelector('[data-testid="snippet-row"]', { timeout: 5000 });
ok("snippet saved", (await page.getByTestId("snippet-row").count()) >= 1);
await page.getByRole("tab", { name: "最近活动" }).click();
ok("activity cards", await page.getByTestId("recent-card").isVisible() && await page.getByTestId("history-card").isVisible());
await page.screenshot({ path: `${OUT}/account.png` });

// ---- 7.1 Session 端点(BFF)----
const session = await page.evaluate(() => fetch("/api/bff/auth/session").then((r) => r.json()));
ok("session endpoint authenticated", session.authenticated === true);

// ---- 7.2 语言切换:zh → en → zh + 根路径默认中文 ----
await page.goto(`${BASE}/`, { waitUntil: "domcontentloaded" });
await page.waitForTimeout(1500);
ok("root path defaults to /zh", /\/zh\/?$/.test(page.url()), page.url());
await page.goto(`${BASE}/zh/overview`, { waitUntil: "domcontentloaded" });
await page.waitForTimeout(800);
await page.getByTestId("lang-en").click();
await page.waitForURL("**/en/overview", { timeout: 8000 });
ok("lang switch to EN", page.url().includes("/en/overview"));
ok("EN overview title", await page.locator("h2", { hasText: "Overview" }).first().isVisible());
ok("EN nav label", await page.getByText("API Keys", { exact: true }).first().isVisible());
await page.getByTestId("lang-zh").click();
await page.waitForURL("**/zh/overview", { timeout: 8000 });
ok("lang switch back to zh", page.url().includes("/zh/overview"));

// ---- 8. Logout → re-login(用户名密码)----
const logout = await page.evaluate(() => fetch("/api/bff/auth/logout", { method: "POST" }).then((r) => r.json()));
ok("logout ok", logout.ok === true);
await page.goto(`${BASE}/overview`, { waitUntil: "networkidle" });
ok("after logout redirects to /zh/login", page.url().includes("/zh/login"));
await page.getByTestId("login-username").fill(USERNAME);
await page.getByTestId("login-password").fill(PASSWORD);
await page.getByTestId("login-submit").click();
await page.waitForURL("**/overview", { timeout: 10_000 });
ok("re-login with username+password", page.url().includes("/zh/overview"));

ok("no console errors", consoleErrors.length === 0, consoleErrors.slice(0, 3).join(" | ") || "clean");

await browser.close();
children.forEach((c) => c.kill("SIGKILL"));
const failed = results.filter((r) => !r.pass).length;
console.log(`\nE2E(BFF): ${results.length - failed}/${results.length} passed`);
process.exit(failed === 0 ? 0 : 1);
