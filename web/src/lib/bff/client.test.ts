import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { bffFetch } from "./client";

// jsdom 下 location.href 赋值不导航,记录调用
const origLocation = window.location;

beforeEach(() => {
  vi.stubGlobal("fetch", vi.fn());
  vi.spyOn(window, "location", "get").mockReturnValue({ ...origLocation, href: "", pathname: "/zh/overview" } as Location);
});
afterEach(() => {
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
});

describe("bffFetch 401 处理(不误跳登录)", () => {
  it("401 且会话有效 → 抛错,不跳登录", async () => {
    const fetchMock = vi.mocked(fetch);
    fetchMock
      .mockResolvedValueOnce(new Response(JSON.stringify({ code: "unauthorized" }), { status: 401 })) // 业务请求 401
      .mockResolvedValueOnce(new Response(JSON.stringify({ authenticated: true }), { status: 200 })); // session 检查
    await expect(bffFetch("/v1/databases/x/kv")).rejects.toThrow("HTTP 401");
    expect(window.location.href).toBe("");
  });

  it("401 且会话失效 → 跳登录(带语言前缀)", async () => {
    const fetchMock = vi.mocked(fetch);
    fetchMock
      .mockResolvedValueOnce(new Response(JSON.stringify({ code: "unauthorized" }), { status: 401 }))
      .mockResolvedValueOnce(new Response(JSON.stringify({ authenticated: false }), { status: 200 }));
    await expect(bffFetch("/v1/databases/x/kv")).rejects.toThrow("not authenticated");
    expect(window.location.href).toBe("/zh/login");
  });

  it("401 且 session 接口失败 → 视为会话失效跳登录", async () => {
    const fetchMock = vi.mocked(fetch);
    fetchMock
      .mockResolvedValueOnce(new Response(JSON.stringify({ code: "unauthorized" }), { status: 401 }))
      .mockRejectedValueOnce(new Error("network"));
    await expect(bffFetch("/v1/databases/x/kv")).rejects.toThrow("not authenticated");
    expect(window.location.href).toBe("/zh/login");
  });

  it("非 401 错误原样抛错,不跳登录", async () => {
    const fetchMock = vi.mocked(fetch);
    fetchMock.mockResolvedValueOnce(new Response(JSON.stringify({ error: "database not found" }), { status: 404 }));
    await expect(bffFetch("/v1/databases/x/kv")).rejects.toThrow("database not found");
    expect(window.location.href).toBe("");
  });

  it("200 正常返回数据", async () => {
    const fetchMock = vi.mocked(fetch);
    fetchMock.mockResolvedValueOnce(new Response(JSON.stringify({ keys: ["a"] }), { status: 200 }));
    const data = await bffFetch<{ keys: string[] }>("/v1/databases/x/kv");
    expect(data.keys).toEqual(["a"]);
    expect(window.location.href).toBe("");
  });
});
