/* Landing 国际化:en / zh 双语文案。
 * 所有展示文案集中于此;组件经 locale 取 dict,不硬编码 UI 文案。 */

export const locales = ["en", "zh"] as const;
export type Locale = (typeof locales)[number];
export const defaultLocale: Locale = "zh";

export interface Dict {
  meta: { title: string; description: string };
  nav: { product: string; benchmarks: string; code: string; alpha: string; cta: string };
  hero: {
    badge: string; titleA: string; titleB: string; titleC: string;
    subtitle: string; ctaPrimary: string; ctaSecondary: string;
    stats: [string, string][];
  };
  screens: { label: string; title: string; body: string; captions: string[] };
  features: { label: string; title: string; items: { title: string; body: string; tag: string }[] };
  bench: {
    label: string; title: string; body: string;
    big: { value: string; label: string }[];
    tableHeaders: string[]; rows: { cells: string; active: string; note: string }[];
  };
  code: {
    label: string; title: string;
    singleEngineTitle: string; singleEngineBody: string; tsTitle: string; httpTitle: string;
  };
  alpha: {
    label: string; title: string; body: string; cta: string; backToTop: string;
    tiers: { name: string; price: string; tag: string; highlight: boolean; points: string[]; cta: string }[];
  };
  waitlist: {
    label: string;
    placeholder: string;
    submit: string;
    success: string;
    invalid: string;
    error: string;
    hint: string;
  };
  footer: { tagline: string; sub: string; docs: string; icp: string };
}

export const dict: Record<Locale, Dict> = {
  en: {
    meta: {
      title: "Combee — One app, one Cell. SQL + KV included.",
      description:
        "Combee is the database of your app — a logical Cell per application, with SQL and KV built in. No database instances to provision, no connection pools to babysit. Scale to 1M logical Cells on a single node.",
    },
    nav: {
      product: "Product",
      benchmarks: "Benchmarks",
      code: "Code",
      alpha: "Alpha",
      cta: "Request access",
    },
    hero: {
      badge: "private alpha — invite only",
      titleA: "One app, one Cell.",
      titleB: "SQL + KV",
      titleC: "included.",
      subtitle:
        "No database instances to provision. No connection pools to babysit. A Cell is the database of your app — created with one API call, serving SQL and KV on the same engine.",
      ctaPrimary: "Start with 1,000 free credits",
      ctaSecondary: "See 1M-Cell benchmarks",
      stats: [
        ["1M", "logical Cells / node"],
        ["64µs", "p99 @ 1M × 5k active"],
        ["100%", "cache hit rate"],
        ["10µs", "KV hot GET p50"],
      ] as [string, string][],
    },
    screens: {
      label: "the console",
      title: "Every Cell, fully visible",
      body: "Combee Cloud is the control plane for your Cells — SQL workspace, KV explorer, backups, replication, usage metering and credits in one dark console.",
      captions: [
        "overview — all cells at a glance",
        "cells — one app, one cell",
        "sql — query right inside the console",
        "usage — metered per tenant, per cell",
      ],
    },
    features: {
      label: "built for builders",
      title: "The database of your app — everything included",
      items: [
        {
          title: "SQL + KV, one engine",
          body: "Every Cell serves both SQL (SQLite-class) and key-value with TTL — no sidecars, no second datastore to reconcile.",
          tag: "core",
        },
        {
          title: "Created in one call",
          body: "POST /v1/databases and your Cell exists. 20k logical Cells create in ~15ms. No migrations, no capacity planning.",
          tag: "api",
        },
        {
          title: "TTL & counters built in",
          body: "expire, incr, mget, ttl — the primitives every session store needs, without bolt-on libraries.",
          tag: "kv",
        },
        {
          title: "Backups to object storage",
          body: "Snapshot + WAL-incremental to S3/MinIO, restore on demand. A destroyed node comes back.",
          tag: "durability",
        },
        {
          title: "Replica + auto-failover",
          body: "One replica per Cell with generation fencing against split-brain. Failover promotes in seconds.",
          tag: "reliability",
        },
        {
          title: "Usage metered from day one",
          body: "Per-tenant, per-cell metrics — requests, bytes, storage — flowing into a credits ledger with pricing rules.",
          tag: "business",
        },
      ],
    },
    bench: {
      label: "capacity benchmark",
      title: "1M logical Cells. One process.",
      body: "1M logical Cells ≠ 1M connections. An Active DB Manager keeps a bounded pool of SQLite handles with LRU eviction and idle sleep — measured in a 4-CPU / 8-GB container.",
      big: [
        { value: "1M", label: "logical Cells on one node" },
        { value: "64µs", label: "p99 latency @ 1M × 5k active" },
        { value: "100%", label: "cache hit rate (4+8 container)" },
        { value: "35µs", label: "KV hot GET p99" },
        { value: "41µs", label: "SQL p99" },
        { value: "15ms", label: "20k Cells create" },
      ],
      tableHeaders: ["total cells", "active cells", "note"],
      rows: [
        { cells: "10k", active: "32 · 100 · 500", note: "single node, resident" },
        { cells: "100k", active: "32 · 100 · 500 · 1k", note: "single node, LRU-managed" },
        { cells: "1M", active: "32 · 100 · 500 · 1k · 5k", note: "4+8 container — p99 64µs, hit rate 100%" },
      ],
    },
    code: {
      label: "developer experience",
      title: "Ship on day one. Not on day thirty.",
      singleEngineTitle: "single engine, no copies",
      singleEngineBody:
        "Your SQL tables and your KV namespace live in the same Cell. One backup, one replica, one failure domain — one API.",
      tsTitle: "typescript — @combee/sdk",
      httpTitle: "http — plain REST",
    },
    alpha: {
      label: "closed alpha",
      title: "The database you'll never have to think about",
      body: "Combee is in private alpha with a small number of builders. Invites are invite-only and come with 1,000 Alpha Credits — enough to build and measure before the public beta opens.",
      cta: "Request an invite",
      backToTop: "Back to top",
      tiers: [
        {
          name: "Private Alpha",
          price: "invite",
          tag: "now",
          highlight: true,
          points: [
            "1,000 Alpha Credits to start",
            "Invite code = voucher (single-use)",
            "Cells, SQL, KV, backups, replication",
            "Usage metering + credits ledger",
          ],
          cta: "Request access",
        },
        {
          name: "Public Beta",
          price: "soon",
          tag: "next",
          highlight: false,
          points: [
            "Self-serve signup",
            "Usage-based pricing",
            "Regional placement",
            "Docs & quickstart",
          ],
          cta: "Join the waitlist",
        },
      ],
    },
    waitlist: {
      label: "Email",
      placeholder: "you@example.com",
      submit: "Join waitlist",
      success: "You're on the list! We'll notify you when Public Beta opens.",
      invalid: "Please enter a valid email",
      error: "Submission failed, please try again later",
      hint: "Be the first to know when Public Beta opens",
    },
    footer: {
      tagline: "One app, one Cell.",
      sub: "sql + kv included · no database instances",
      docs: "Documentation",
      icp: "冀ICP备2024088698号-2",
    },
  },

  zh: {
    meta: {
      title: "Combee — 一个应用,一个 Cell。SQL + KV 齐备。",
      description:
        "Combee 是你的应用数据库——每个应用一个逻辑 Cell,内置 SQL 与 KV。无需准备数据库实例,无需维护连接池,单节点即可扩展到 100 万个逻辑 Cell。",
    },
    nav: {
      product: "产品",
      benchmarks: "性能基准",
      code: "代码示例",
      alpha: "Alpha",
      cta: "申请访问",
    },
    hero: {
      badge: "私密 Alpha — 仅限邀请",
      titleA: "一个应用,一个 Cell。",
      titleB: "SQL + KV",
      titleC: "齐备。",
      subtitle:
        "无需准备数据库实例,无需维护连接池。Cell 就是你的应用数据库——一次 API 调用即可创建,同一引擎同时提供 SQL 与 KV。",
      ctaPrimary: "领取 1,000 免费 Credits",
      ctaSecondary: "查看 1M Cell 基准",
      stats: [
        ["1M", "单节点逻辑 Cell"],
        ["64µs", "1M × 5k active 下 p99"],
        ["100%", "缓存命中率"],
        ["10µs", "KV 热读 p50"],
      ] as [string, string][],
    },
    screens: {
      label: "控制台",
      title: "每一个 Cell,尽在掌握",
      body: "Combee Cloud 是你的 Cell 控制平面——SQL 工作台、KV 浏览器、备份、复制、用量计量与 Credits,集中在一个深色控制台。",
      captions: [
        "概览 — 所有 Cell 一览",
        "Cell — 一个应用,一个 Cell",
        "SQL — 在控制台直接查询",
        "用量 — 按租户、按 Cell 计量",
      ],
    },
    features: {
      label: "为开发者而生",
      title: "你的应用数据库 — 一切内置",
      items: [
        {
          title: "SQL + KV,同一引擎",
          body: "每个 Cell 同时提供 SQL(SQLite 级)与带 TTL 的 KV——无需边车、无需第二个数据存储来对齐。",
          tag: "核心",
        },
        {
          title: "一次调用即可创建",
          body: "POST /v1/databases,你的 Cell 就绪。2 万个逻辑 Cell 创建约 15ms。无需迁移、无需容量规划。",
          tag: "api",
        },
        {
          title: "内置 TTL 与计数器",
          body: "expire、incr、mget、ttl——会话存储所需的一切原语,无需额外库。",
          tag: "kv",
        },
        {
          title: "备份到对象存储",
          body: "快照 + WAL 增量到 S3/MinIO,按需恢复。节点销毁也能回来。",
          tag: "持久",
        },
        {
          title: "副本 + 自动故障转移",
          body: "每个 Cell 一个副本,generation fencing 防脑裂。故障转移秒级提升。",
          tag: "可靠",
        },
        {
          title: "从第一天开始计量",
          body: "按租户、按 Cell 的指标——请求、字节、存储——流入带定价规则的 Credits 账本。",
          tag: "商业",
        },
      ],
    },
    bench: {
      label: "容量基准",
      title: "100 万个逻辑 Cell。一个进程。",
      body: "100 万逻辑 Cell ≠ 100 万个连接。Active DB Manager 通过 LRU 逐出与空闲休眠维持有界 SQLite 连接池——在 4 核 / 8GB 容器中实测。",
      big: [
        { value: "1M", label: "单节点逻辑 Cell" },
        { value: "64µs", label: "1M × 5k active 下 p99" },
        { value: "100%", label: "缓存命中率(4+8 容器)" },
        { value: "35µs", label: "KV 热读 p99" },
        { value: "41µs", label: "SQL p99" },
        { value: "15ms", label: "2 万 Cell 创建" },
      ],
      tableHeaders: ["总 Cell 数", "活跃 Cell", "说明"],
      rows: [
        { cells: "10k", active: "32 · 100 · 500", note: "单节点,常驻" },
        { cells: "100k", active: "32 · 100 · 500 · 1k", note: "单节点,LRU 管理" },
        { cells: "1M", active: "32 · 100 · 500 · 1k · 5k", note: "4+8 容器 — p99 64µs,命中率 100%" },
      ],
    },
    code: {
      label: "开发者体验",
      title: "第一天就能上线。不是第三十天。",
      singleEngineTitle: "单一引擎,无副本漂移",
      singleEngineBody:
        "你的 SQL 表与 KV 命名空间住在同一个 Cell。一份备份、一个副本、一个故障域——一个 API。",
      tsTitle: "typescript — @combee/sdk",
      httpTitle: "http — 纯 REST",
    },
    alpha: {
      label: "封闭 Alpha",
      title: "一款你永远不必操心的数据库",
      body: "Combee 正与少数开发者进行私密 Alpha。采用邀请制,邀请即含 1,000 Alpha Credits——足以在公开 Beta 前完成构建与度量。",
      cta: "申请邀请",
      backToTop: "返回顶部",
      tiers: [
        {
          name: "私密 Alpha",
          price: "邀请制",
          tag: "now",
          highlight: true,
          points: [
            "赠送 1,000 Alpha Credits",
            "邀请码 = voucher(一次性)",
            "Cell、SQL、KV、备份、复制",
            "用量计量 + Credits 账本",
          ],
          cta: "申请访问",
        },
        {
          name: "公开 Beta",
          price: "即将开放",
          tag: "next",
          highlight: false,
          points: [
            "自助注册",
            "按用量计费",
            "区域部署",
            "文档与快速上手",
          ],
          cta: "加入候补名单",
        },
      ],
    },
    waitlist: {
      label: "邮箱地址",
      placeholder: "you@example.com",
      submit: "加入候补",
      success: "已登记!Public Beta 开放时会第一时间通知你。",
      invalid: "请输入有效的邮箱地址",
      error: "提交失败,请稍后重试",
      hint: "Public Beta 开放时第一时间通知",
    },
    footer: {
      tagline: "一个应用,一个 Cell。",
      sub: "sql + kv 齐备 · 无需数据库实例",
      docs: "文档",
      icp: "冀ICP备2024088698号-2",
    },
  },
};

export function getDict(locale: Locale): Dict {
  return dict[locale];
}
