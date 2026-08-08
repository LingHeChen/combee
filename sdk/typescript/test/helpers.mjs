//! Contract test 基础设施:启动真实 Combee API Server(dev 模式),等待就绪。
//! 需要已构建的二进制(target/debug/combee-api-server);缺失时自动 cargo build。

import { spawn, execSync } from "node:child_process";
import path from "node:path";
import { fileURLToPath } from "node:url";

const here = path.dirname(fileURLToPath(import.meta.url));
const repoRoot = path.resolve(here, "../../..");
const PORT = 18091;

export const BASE_URL = `http://127.0.0.1:${PORT}`;

export async function startServer() {
  const bin = path.join(repoRoot, "target/debug/combee-api-server");
  // 总是增量 build(避免用过期二进制跑 contract tests)
  execSync("cargo build -p combee-api-server", { cwd: repoRoot, stdio: "inherit" });
  const child = spawn(bin, [], {
    env: {
      ...process.env,
      COMBEE_BIND_ADDR: `127.0.0.1:${PORT}`,
      COMBEE_DATA_DIR: path.join(repoRoot, "target/.sdk-test-data"),
      COMBEE_AUTH: "off",
      COMBEE_USAGE_FLUSH_INTERVAL_SECS: "1",
    },
    stdio: "ignore",
  });
  // 等待 ready
  const deadline = Date.now() + 30_000;
  for (;;) {
    try {
      const r = await fetch(`${BASE_URL}/v1/databases`);
      if (r.status === 200) break;
    } catch {
      /* retry */
    }
    if (Date.now() > deadline) {
      child.kill("SIGKILL");
      throw new Error("server did not become ready");
    }
    await new Promise((r) => setTimeout(r, 200));
  }
  return child;
}
