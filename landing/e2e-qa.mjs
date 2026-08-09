/* Combee Landing E2E(playwright chromium headless)— 中英双语 + 响应式
 * 前置:npm run build → out/;脚本自动起静态服务。
 * 覆盖:根路径重定向、/en 英文、/zh 中文、语言切换、localStorage 偏好、
 *       截图/特性/bench/高亮/定价/锚点/移动端/零 console error。 */
import { chromium } from "playwright";
import { spawn } from "node:child_process";
import fs from "node:fs";
import path from "node:path";

const PORT = 3299;
const BASE = `http://127.0.0.1:${PORT}`;
const OUT = path.resolve("out");
const CHROME = "/Users/zhouhaixin/projects/personal/combee/web/node_modules/playwright-core/.local-browsers/chromium_headless_shell-1234/chrome-headless-shell-mac-arm64/chrome-headless-shell";

function startServer() {
  return spawn("python3", ["-m", "http.server", String(PORT), "--directory", OUT], { stdio: "ignore" });
}

async function waitReady(url, tries = 40) {
  for (let i = 0; i < tries; i++) {
    try { if ((await fetch(url)).ok) return true; } catch {}
    await new Promise((r) => setTimeout(r, 250));
  }
  return false;
}

const results = [];
async function check(name, cond) {
  results.push({ name, pass: !!cond });
  console.log(`${cond ? "PASS" : "FAIL"} ${name}`);
}

let server, browser;
try {
  if (!fs.existsSync(path.join(OUT, "en", "index.html")) || !fs.existsSync(path.join(OUT, "zh", "index.html"))) {
    console.error("out/ 缺少 en/zh 页面,先 npm run build");
    process.exit(1);
  }
  server = startServer();
  if (!(await waitReady(`${BASE}/`))) { console.error("静态服务未就绪"); process.exit(1); }

  browser = await chromium.launch({ headless: true, executablePath: CHROME, args: ["--no-sandbox"] });
  const page = await browser.newPage({ viewport: { width: 1440, height: 900 } });
  const consoleErrors = [];
  page.on("console", (m) => m.type() === "error" && consoleErrors.push(m.text()));

  /* ---- 1. 根路径重定向:默认中文(不跟随浏览器语言) ---- */
  await page.goto(BASE, { waitUntil: "load" });
  await page.waitForURL(/\/zh\/?$/, { timeout: 8000 });
  await check("根路径默认重定向到中文 /zh", /\/zh\/?$/.test(page.url()));

  /* ---- 2. /en 英文内容 ---- */
  await page.goto(`${BASE}/en`, { waitUntil: "load" });
  await page.waitForTimeout(800);
  await check("en:hero 标语", (await page.locator("h1").innerText()).includes("One app, one Cell."));
  await check("en:nav CTA", await page.getByTestId("nav-cta").isVisible());
  await check("en:hero CTA", (await page.getByTestId("hero-cta").innerText()).includes("1,000 free credits"));
  const imgs = await page.locator('[data-testid="screens"] img').count();
  await check(`en:screens 4 截图 (${imgs})`, imgs === 4);
  const features = await page.locator('[data-testid="features"] > div').count();
  await check(`en:features 6 卡 (${features})`, features === 6);
  const big = await page.locator('[data-testid="bench-big"] > div').count();
  await check(`en:bench 大数字 6 (${big})`, big === 6);
  const rows = await page.locator('[data-testid="bench-table"] tbody tr').count();
  await check(`en:bench 表 3 行 (${rows})`, rows === 3);
  await page.locator("#code").scrollIntoViewIfNeeded();
  await page.waitForTimeout(1200);
  const tokens = await page.locator('[data-testid="code-showcase"] .token').count();
  await check(`en:代码高亮 token (${tokens})`, tokens > 3);
  const tiers = await page.locator('[data-testid="tiers"] > div').count();
  await check(`en:tiers 2 卡 (${tiers})`, tiers === 2);
  await check("en:tiers 含 Private Alpha", (await page.locator('[data-testid="tiers"]').innerText()).includes("Private Alpha"));

  /* ---- 3. 语言切换:en → zh ---- */
  await page.getByTestId("lang-zh").click();
  await page.waitForURL(/\/zh\/?$/, { timeout: 8000 });
  await page.waitForTimeout(800);
  await check("zh:URL 切到 /zh", /\/zh\/?$/.test(page.url()));
  const zhH1 = await page.locator("h1").innerText();
  await check("zh:hero 中文标语", zhH1.includes("一个应用") && zhH1.includes("SQL + KV"));
  await check("zh:nav 产品", (await page.locator("header").innerText()).includes("产品"));
  await check("zh:features 中文", (await page.locator('[data-testid="features"]').innerText()).includes("同一引擎"));
  await check("zh:bench 表中文", (await page.locator('[data-testid="bench-table"]').innerText()).includes("总 Cell 数"));
  await check("zh:tiers 中文", (await page.locator('[data-testid="tiers"]').innerText()).includes("私密 Alpha"));
  await check("zh:截图 4 张", (await page.locator('[data-testid="screens"] img').count()) === 4);

  /* ---- 4. zh → en 切回 ---- */
  await page.getByTestId("lang-en").click();
  await page.waitForURL(/\/en\/?$/, { timeout: 8000 });
  await page.waitForTimeout(600);
  await check("切回 en:hero 英文", (await page.locator("h1").innerText()).includes("One app"));

  /* ---- 5. localStorage 偏好:combee-locale=zh → 根路径去 /zh ---- */
  await page.goto(`${BASE}/en`, { waitUntil: "load" });
  await page.evaluate(() => localStorage.setItem("combee-locale", "zh"));
  await page.goto(BASE, { waitUntil: "load" });
  await page.waitForURL(/\/zh\/?$/, { timeout: 8000 });
  await check("localStorage 偏好生效 → /zh", /\/zh\/?$/.test(page.url()));

  /* ---- 5b. localStorage 明确选 en → /en ---- */
  await page.evaluate(() => localStorage.setItem("combee-locale", "en"));
  await page.goto(BASE, { waitUntil: "load" });
  await page.waitForURL(/\/en\/?$/, { timeout: 8000 });
  await check("localStorage=en 时跳英文 /en", /\/en\/?$/.test(page.url()));
  await page.evaluate(() => localStorage.removeItem("combee-locale"));

  /* ---- 6. 锚点导航(en)---- */
  await page.goto(`${BASE}/en`, { waitUntil: "load" });
  await page.waitForTimeout(500);
  await page.locator('a[href="#benchmarks"]').first().click();
  await page.waitForTimeout(900);
  await check("锚点跳转 #benchmarks", page.url().includes("#benchmarks"));

  /* ---- 7. 截图 ---- */
  fs.mkdirSync("/tmp/landing-shots", { recursive: true });
  await page.evaluate(() => window.scrollTo(0, 0));
  await page.waitForTimeout(300);
  await page.screenshot({ path: "/tmp/landing-shots/en-hero.png" });
  await page.evaluate(() => window.scrollTo(0, document.body.scrollHeight));
  await page.waitForTimeout(400);
  await page.screenshot({ path: "/tmp/landing-shots/en-full.png", fullPage: true });
  await page.goto(`${BASE}/zh`, { waitUntil: "load" });
  await page.waitForTimeout(600);
  await page.screenshot({ path: "/tmp/landing-shots/zh-hero.png" });

  /* ---- 8. 移动端 375px 无横向溢出(zh)---- */
  const mobile = await browser.newPage({ viewport: { width: 375, height: 812 } });
  await mobile.goto(`${BASE}/zh`, { waitUntil: "load" });
  await mobile.waitForTimeout(800);
  const overflow = await mobile.evaluate(
    () => document.documentElement.scrollWidth - document.documentElement.clientWidth,
  );
  await check(`zh 移动端无横向溢出 (${overflow}px)`, overflow <= 2);
  await mobile.screenshot({ path: "/tmp/landing-shots/zh-mobile.png" });

  await check("零 console error", consoleErrors.length === 0);
} catch (e) {
  console.error("E2E 异常:", e.message);
  results.push({ name: "e2e 执行", pass: false });
} finally {
  if (browser) await browser.close();
  if (server) server.kill();
}

const failed = results.filter((r) => !r.pass);
console.log(`\n=== ${results.length - failed.length}/${results.length} passed ===`);
if (failed.length) {
  failed.forEach((f) => console.log("  FAILED:", f.name));
  process.exit(1);
}
