import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { aggregateOverview } from "./aggregate";
import type { BffSession } from "./auth";

// server-only 模块在 vitest 环境不可用,直接 mock 为空
vi.mock("server-only", () => ({}));

// mock combeeRequest(server 端请求)
const mockCombeeRequest = vi.fn();
vi.mock("@/lib/combee-client", () => ({
  combeeRequest: (path: string) => mockCombeeRequest(path),
}));
vi.mock("@/lib/bff/auth", async (importOriginal) => {
  const mod = (await importOriginal()) as Record<string, unknown>;
  return { ...mod, bffKey: () => "cmb_sk_admin" };
});

const session: BffSession = {
  username: "alice",
  api_key: "cmb_sk_alice",
  tenant_id: "tenant-alice",
  created_at: 0,
};

beforeEach(() => {
  mockCombeeRequest.mockReset();
});
afterEach(() => {
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
});

describe("BFF 权限: cells 归属过滤", () => {
  it("新用户只看到自己租户的 Cell,看不到平台/他人 Cell", async () => {
    // admin key 拉到全量:combee-bff(平台 DEFAULT)+ bob 的 cell + alice 的 cell
    mockCombeeRequest.mockImplementation((path: string) => {
      if (path === "/v1/databases?limit=1000") {
        return Promise.resolve([
          { id: "cell-combee-bff", tenant_id: "tenant-platform", state: "active", created_at: 1 },
          { id: "cell-bob", tenant_id: "tenant-bob", state: "active", created_at: 2 },
          { id: "cell-alice", tenant_id: "tenant-alice", state: "active", created_at: 3 },
        ]);
      }
      if (path === "/v1/usage/summary") return Promise.resolve({ request_count: 0, current_storage_bytes: 0 });
      if (path === "/v1/credits/balance") return Promise.resolve({ available: "1000" });
      return Promise.resolve(null);
    });

    const overview = await aggregateOverview(session);
    const ids = overview.recentCells.map((c) => c.id);
    expect(ids).toContain("cell-alice");
    expect(ids).not.toContain("cell-combee-bff"); // 平台 cell 必须对用户不可见
    expect(ids).not.toContain("cell-bob"); // 他人 cell 必须对用户不可见
    expect(overview.cellsTotal).toBe(1);
  });

  it("tenant_id 缺失(旧用户未回填)→ 列表为空,不泄露任何 cell", async () => {
    mockCombeeRequest.mockImplementation((path: string) => {
      if (path === "/v1/databases?limit=1000") {
        return Promise.resolve([{ id: "cell-x", tenant_id: "tenant-x", state: "active", created_at: 1 }]);
      }
      if (path === "/v1/usage/summary") return Promise.resolve({ request_count: 0, current_storage_bytes: 0 });
      if (path === "/v1/credits/balance") return Promise.resolve({ available: "0" });
      return Promise.resolve(null);
    });
    const legacy: BffSession = { ...session, tenant_id: null };
    const overview = await aggregateOverview(legacy);
    expect(overview.cellsTotal).toBe(0);
    expect(overview.recentCells).toHaveLength(0);
  });

  it("空列表(新注册未建 cell)→ 正常返回 0,不报错", async () => {
    mockCombeeRequest.mockImplementation((path: string) => {
      if (path === "/v1/databases?limit=1000") return Promise.resolve([]);
      if (path === "/v1/usage/summary") return Promise.resolve({ request_count: 0, current_storage_bytes: 0 });
      if (path === "/v1/credits/balance") return Promise.resolve({ available: "0" });
      return Promise.resolve(null);
    });
    const overview = await aggregateOverview(session);
    expect(overview.cellsTotal).toBe(0);
  });
});
