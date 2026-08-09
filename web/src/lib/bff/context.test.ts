import { describe, it, expect, beforeEach, afterEach, vi } from "vitest";
vi.mock("server-only", () => ({}));
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { bffLog } from "./context";

const logFile = path.join(os.tmpdir(), `bfflog-${process.pid}.jsonl`);

beforeEach(() => {
  process.env.COMBEE_BFF_LOG_FILE = logFile;
  try {
    fs.unlinkSync(logFile);
  } catch {
    /* */
  }
});
afterEach(() => {
  try {
    fs.unlinkSync(logFile);
  } catch {
    /* */
  }
});

describe("bffLog", () => {
  it("输出单行 JSON,公共字段齐全", () => {
    bffLog("INFO", { operation: "auth.register", tenant_id: "t1", cell_id: "c1" });
    const raw = fs.readFileSync(logFile, "utf8").trim();
    const parsed = JSON.parse(raw);
    expect(parsed.service).toBe("combee-bff");
    expect(parsed.operation).toBe("auth.register");
    expect(parsed.tenant_id).toBe("t1");
    expect(parsed.cell_id).toBe("c1");
    expect(parsed.timestamp).toBeTruthy();
    expect(parsed.level).toBe("INFO");
  });

  it("ERROR 级带 error_code / request_id / latency_ms", () => {
    bffLog("ERROR", { operation: "sql.query", error_code: "SQL_TIMEOUT", request_id: "req_123", latency_ms: 5001 });
    const parsed = JSON.parse(fs.readFileSync(logFile, "utf8").trim());
    expect(parsed.error_code).toBe("SQL_TIMEOUT");
    expect(parsed.request_id).toBe("req_123");
    expect(parsed.latency_ms).toBe(5001);
  });

  it("敏感字段名(api_key/password/session/voucher)整条丢弃", () => {
    bffLog("INFO", { operation: "auth.login", api_key: "cmb_sk_secret" });
    bffLog("INFO", { operation: "auth.login", password: "hunter2" });
    bffLog("INFO", { operation: "auth.login", session: "abc" });
    bffLog("INFO", { operation: "voucher.redeem", voucher: "CMB-X" });
    expect(fs.existsSync(logFile)).toBe(false);
  });

  it("非敏感同结构字段正常输出(不误杀)", () => {
    bffLog("INFO", { operation: "cell.create", cell_id: "c1", status: 201 });
    const parsed = JSON.parse(fs.readFileSync(logFile, "utf8").trim());
    expect(parsed.cell_id).toBe("c1");
    expect(parsed.status).toBe(201);
  });

  it("operation 缺省时 message 为 operation", () => {
    bffLog("WARN", { operation: "quota.exceeded" });
    const parsed = JSON.parse(fs.readFileSync(logFile, "utf8").trim());
    expect(parsed.message ?? parsed.operation).toBe("quota.exceeded");
  });
});
