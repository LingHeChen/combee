import { describe, it, expect } from "vitest";
import { dict, locales, defaultLocale, getDict } from "./i18n";

type UnknownRecord = Record<string, unknown>;

/** 递归收集 key 路径,用于双语结构一致性比对 */
function keyPaths(obj: unknown, prefix = ""): string[] {
  if (Array.isArray(obj)) {
    return obj.length
      ? keyPaths(obj[0], `${prefix}[]`)
      : [`${prefix}[]`];
  }
  if (obj !== null && typeof obj === "object") {
    return Object.entries(obj as UnknownRecord).flatMap(([k, v]) =>
      keyPaths(v, prefix ? `${prefix}.${k}` : k),
    );
  }
  return [prefix];
}

describe("i18n 字典", () => {
  it("en / zh 结构完全一致(递归 key 集合相等)", () => {
    const enKeys = keyPaths(dict.en).sort();
    const zhKeys = keyPaths(dict.zh).sort();
    expect(zhKeys).toEqual(enKeys);
    expect(enKeys.length).toBeGreaterThan(40);
  });

  it("locales 含 en/zh,默认中文", () => {
    expect(locales).toEqual(["en", "zh"]);
    expect(defaultLocale).toBe("zh");
    expect(getDict("en").meta.title).toContain("One app, one Cell.");
    expect(getDict("zh").meta.title).toContain("一个应用");
  });

  it("hero 文案双语齐全,统计 4 项一致", () => {
    expect(dict.en.hero.stats).toHaveLength(4);
    expect(dict.zh.hero.stats).toHaveLength(4);
    expect(dict.zh.hero.titleB).toBe("SQL + KV");
    expect(dict.zh.hero.ctaPrimary).toContain("Credits");
  });

  it("features/bench/tiers 数量双语一致", () => {
    expect(dict.zh.features.items.length).toBe(dict.en.features.items.length);
    expect(dict.zh.bench.big.length).toBe(dict.en.bench.big.length);
    expect(dict.zh.bench.rows.length).toBe(dict.en.bench.rows.length);
    expect(dict.zh.alpha.tiers.length).toBe(dict.en.alpha.tiers.length);
  });

  it("benchmark 数值字段双语一致(数据不因语言变化)", () => {
    for (let i = 0; i < dict.en.bench.big.length; i++) {
      expect(dict.zh.bench.big[i].value).toBe(dict.en.bench.big[i].value);
    }
    expect(dict.zh.bench.big.some((b) => b.value === "64µs")).toBe(true);
  });

  it("每个语言恰好一个 highlight 档位", () => {
    const enHl = dict.en.alpha.tiers.filter((t) => t.highlight).length;
    const zhHl = dict.zh.alpha.tiers.filter((t) => t.highlight).length;
    expect(enHl).toBe(1);
    expect(zhHl).toBe(1);
  });

  it("无空文案(不允许漏翻译)", () => {
    const check = (obj: unknown, path = "") => {
      if (Array.isArray(obj)) return obj.forEach((v, i) => check(v, `${path}[${i}]`));
      if (obj !== null && typeof obj === "object") {
        return Object.entries(obj as UnknownRecord).forEach(([k, v]) => check(v, `${path}.${k}`));
      }
      if (typeof obj === "string" && obj.trim() === "") {
        throw new Error(`empty string at ${path}`);
      }
    };
    expect(() => check(dict.en)).not.toThrow();
    expect(() => check(dict.zh)).not.toThrow();
  });
});
