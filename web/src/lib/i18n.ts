/* Combee Cloud Console 国际化:en / zh(默认中文)。
 * 全部 UI 文案集中于此;组件经 useT()(client)或 getDict()(server)取 dict。 */

export const locales = ["zh", "en"] as const;
export type Locale = (typeof locales)[number];
export const defaultLocale: Locale = "zh";
export const COOKIE = "combee-locale";

export interface Dict {
  meta: { title: string; description: string };
  shell: {
    product: string;
    overview: string;
    cells: string;
    apiKeys: string;
    usage: string;
    credits: string;
    account: string;
    createCell: string;
    logOut: string;
    user: string;
  };
  welcome: {
    title: string;
    subtitle: string;
    steps: { title: string; desc: string }[];
    openDocs: string;
    continue: string;
    continueTo: string;
    ts: string;
    py: string;
  };
  login: {
    title: string;
    subtitle: string;
    username: string;
    password: string;
    apiKey: string;
    apiKeyPlaceholder: string;
    signIn: string;
    noAccount: string;
    createAccount: string;
    invalid: string;
  };
  register: {
    title: string;
    subtitle: string;
    username: string;
    password: string;
    confirm: string;
    accessCode: string;
    accessCodePlaceholder: string;
    accessCodeHint: string;
    signUp: string;
    haveAccount: string;
    signIn: string;
    errorPrefix: string;
    passwordMismatch: string;
    doneWelcome: string;
    doneSub: string;
    yourApiKey: string;
    onlyOnce: string;
    copy: string;
    copied: string;
    goToDashboard: string;
  };
  overview: {
    title: string;
    subtitle: string;
    totalLabel: string;
    statCells: string;
    statActive: string;
    statRequests: string;
    statStorage: string;
    statCredits: string;
    recentCells: string;
    recentCellsSub: string;
    viewAll: string;
    thCell: string;
    thStatus: string;
    thRegion: string;
    thStorage: string;
    thRequests: string;
    thLastActive: string;
    stateActive: string;
    stateIdle: string;
    stateReplicating: string;
    stateDegraded: string;
    empty: string;
    emptyCta: string;
  };
  cells: {
    title: string;
    subtitle: string;
    allStatus: string;
    allRegions: string;
    idLabel: string;
    noMatch: string;
    newCell: string;
    search: string;
    searchPlaceholder: string;
    active: string;
    idle: string;
    empty: string;
    emptyCta: string;
    createdAt: string;
  };
  cellNew: {
    title: string;
    subtitle: string;
    required: string;
    lazyHint: string;
    regionAuto: string;
    name: string;
    namePlaceholder: string;
    nameHint: string;
    ensureHint: string;
    create: string;
    creating: string;
    cancel: string;
    error: string;
    nameRequired: string;
  };
  cellDetail: {
    back: string;
    delete: string;
    deleteConfirm: string;
    deleting: string;
    connect: string;
    stateHealthy: string;
    rename: string;
    statStatus: string;
    statOperational: string;
    statAllSystems: string;
    statAttention: string;
    storagePct: string;
    lifecycle: string;
    created: string;
    lastActive: string;
    backupHealth: string;
    lastRun: string;
    snapshotWal: string;
    replicationHealth: string;
    readReplicas: string;
    advancedDiag: string;
    engine: string;
    engineDesc: string;
    kvKeys: string;
    sqlTables: string;
    keysSuffix: string;
    tablesSuffix: string;
    durability: string;
    durabilityNormal: string;
    tabs: { overview: string; sql: string; kv: string; backups: string; replication: string; usage: string; settings: string };
    notFound: string;
    backToCells: string;
    deleteCellConfirmTemplate: string;
  };
  connect: {
    title: string;
    subtitle: string;
    apiKeyLabel: string;
    copy: string;
    copied: string;
    ts: string;
    py: string;
    http: string;
  };
  apiKeys: {
    title: string;
    subtitle: string;
    create: string;
    creating: string;
    keyName: string;
    keyNamePlaceholder: string;
    created: string;
    createdOnce: string;
    copy: string;
    done: string;
    revoke: string;
    revokeConfirm: string;
    revoked: string;
    name: string;
    createdAt: string;
    status: string;
    active: string;
    empty: string;
    error: string;
    nameRequired: string;
  };
  usage: {
    title: string;
    subtitle: string;
    allCells: string;
    custom: string;
    cardRequests: string;
    cardBytesIn: string;
    cardSqlRw: string;
    cardKvRw: string;
    cardEgress: string;
    cardCreditsConsumed: string;
    cardNoteStable: string;
    cardNoteGrowth: string;
    cardNoteRunRate: string;
    chartTitle: string;
    chartRequests: string;
    chartSqlOps: string;
    chartKvOps: string;
    consumptionByCell: string;
    exportCsv: string;
    thCellId: string;
    thData: string;
    thCredits: string;
    currentStorage: string;
    totalRequests: string;
    totalBytesIn: string;
    totalBytesOut: string;
    totalStorage: string;
    period: string;
    last24h: string;
    last7d: string;
    last30d: string;
    requestsChart: string;
    byMetric: string;
    metric: string;
    value: string;
    empty: string;
    thRequests: string;
    thRegion: string;
    cardStorage: string;
  };
  credits: {
    title: string;
    subtitle: string;
    balance: string;
    available: string;
    redeem: string;
    redeemPlaceholder: string;
    redeeming: string;
    redeemed: string;
    error: string;
    codeRequired: string;
    buy: string;
    consumedMtd: string;
    estRemaining: string;
    monthlyAllowance: string;
    viewOlder: string;
    thDate: string;
    thDescription: string;
    thBalance: string;
    transactions: string;
    type: string;
    amount: string;
    createdAt: string;
    empty: string;
    grant: string;
    usage: string;
    redemption: string;
  };
  account: {
    title: string;
    subtitle: string;
    tabs: { profile: string; prefs: string; onboarding: string; snippets: string; activity: string };
    profile: {
      displayName: string;
      locale: string;
      timezone: string;
      save: string;
      saved: string;
      username: string;
      saveError: string;
      identity: string;
    };
    prefs: {
      defaultRange: string;
      defaultRegion: string;
      pageSize: string;
      save: string;
      saved: string;
      consolePrefs: string;
      prefsHint: string;
    };
    onboarding: {
      createdCell: string;
      createdKey: string;
      firstRequest: string;
      completedAt: string;
      notStarted: string;
      progress: string;
      completeMsg: string;
      pendingMsg: string;
    };
    snippets: {
      title: string;
      save: string;
      savedTitle: string;
      empty: string;
      emptyHint: string;
      delete: string;
      deleteConfirm: string;
    };
    activity: {
      recentCells: string;
      history: string;
      empty: string;
      historyHint: string;
      noRecent: string;
      noQueries: string;
    };
  };
  sql: {
    run: string;
    running: string;
    clear: string;
    placeholder: string;
    results: string;
    rows: string;
    error: string;
    noResult: string;
    saveSnippet: string;
    snippetTitle: string;
    snippetSaved: string;
    snippetPrompt: string;
  };
  kv: {
    key: string;
    value: string;
    ttl: string;
    ttlHint: string;
    browse: string;
    singleKey: string;
    scan: string;
    get: string;
    set: string;
    del: string;
    notFound: string;
    result: string;
    keyRequired: string;
  };
  backups: {
    title: string;
    create: string;
    creating: string;
    backupId: string;
    size: string;
    status: string;
    createdAt: string;
    restore: string;
    restoring: string;
    restoreConfirm: string;
    restored: string;
    empty: string;
    error: string;
    statusCompleted: string;
    statusFailed: string;
    statusPending: string;
    noticeArchived: string;
    showing: string;
    restoreConfirmTitle: string;
    restoreDesc: string;
    critical: string;
    destructiveWarn: string;
    confirmRestore: string;
  };
  replication: {
    title: string;
    role: string;
    primary: string;
    replica: string;
    peer: string;
    lag: string;
    failover: string;
    failoverConfirm: string;
    failoverDone: string;
    noReplica: string;
    error: string;
    primaryToSecondary: string;
    continuous: string;
    disabled: string;
    off: string;
    enabled: string;
    disabledLabel: string;
    secondary: string;
    lastSync: string;
    controls: string;
    disableReplica: string;
    enableReplica: string;
    advancedOps: string;
    manualFailoverDesc: string;
    typeToConfirm: string;
    initiateFailover: string;
  };
  cellUsage: {
    title: string;
    requests: string;
    bytesIn: string;
    bytesOut: string;
    storage: string;
    empty: string;
  };
  common: {
    loading: string;
    error: string;
    cancel: string;
    save: string;
    delete: string;
    empty: string;
    retry: string;
    unauthorized: string;
  };
}

export const dict: Record<Locale, Dict> = {
  zh: {
    meta: { title: "Combee Cloud", description: "一个应用,一个 Cell。SQL + KV 齐备。" },
    shell: {
      product: "Combee Cloud",
      overview: "概览",
      cells: "Cells",
      apiKeys: "API Keys",
      usage: "用量",
      credits: "Credits",
      account: "账户",
      createCell: "新建 Cell",
      logOut: "退出登录",
      user: "用户",
    },
    welcome: {
      title: "欢迎使用 Combee",
      subtitle: "创建你的第一个 Cell 并发出第一个请求,5 分钟内即可开始。",
      steps: [
        { title: "创建第一个 Cell", desc: "准备一个安全的环境。" },
        { title: "创建 API Key", desc: "认证你的应用。" },
        { title: "安装 SDK", desc: "把 Combee 加入你的项目。" },
        { title: "执行 SQL 或 KV 请求", desc: "安全地执行查询。" },
      ],
      openDocs: "打开文档",
      continue: "进入 Cell",
      continueTo: "继续",
      ts: "TypeScript",
      py: "Python",
    },
    login: {
      title: "登录 Combee Cloud",
      subtitle: "使用你的 API Key 登录(私密 Alpha 邀请制)。",
      username: "用户名",
      password: "密码",
      apiKey: "API Key",
      apiKeyPlaceholder: "cmb_sk_…",
      signIn: "登录",
      noAccount: "还没有账户?",
      createAccount: "创建账户",
      invalid: "登录失败,请检查 API Key",
    },
    register: {
      title: "创建账户",
      subtitle: "注册需要 Alpha 邀请码(注册即赠送 1,000 Credits)。",
      username: "用户名",
      password: "密码",
      confirm: "确认密码",
      accessCode: "Alpha 邀请码",
      accessCodePlaceholder: "CMB-XXXX-XXXX-XXXX",
      accessCodeHint: "邀请码 = voucher,一次性使用",
      signUp: "注册",
      haveAccount: "已有账户?",
      signIn: "登录",
      errorPrefix: "注册失败:",
      passwordMismatch: "两次输入的密码不一致",
      doneWelcome: "欢迎使用 Combee",
      doneSub: "你的默认 API Key 已自动生成,这是唯一一次明文展示。",
      yourApiKey: "你的 API Key",
      onlyOnce: "此 Key 仅显示一次,请立即保存。",
      copy: "复制",
      copied: "已复制",
      goToDashboard: "进入 Dashboard",
    },
    overview: {
      title: "概览",
      subtitle: "你的 Combee 实例一览",
      totalLabel: "总数",
      statCells: "Cell 总数",
      statActive: "活跃 Cell",
      statRequests: "请求数",
      statStorage: "存储",
      statCredits: "Credits 余额",
      recentCells: "最近使用",
      recentCellsSub: "最近创建或访问的 Cell。",
      viewAll: "查看全部",
      thCell: "Cell",
      thStatus: "状态",
      thRegion: "区域",
      thStorage: "存储",
      thRequests: "请求数",
      thLastActive: "最近活跃",
      stateActive: "活跃",
      stateIdle: "空闲",
      stateReplicating: "复制中",
      stateDegraded: "降级",
      empty: "还没有 Cell",
      emptyCta: "创建第一个 Cell",
    },
    cells: {
      title: "Cells",
      subtitle: "每个应用一个 Cell",
      allStatus: "全部状态",
      allRegions: "全部区域",
      idLabel: "ID",
      noMatch: "没有符合条件的 Cell。",
      newCell: "新建 Cell",
      search: "搜索",
      searchPlaceholder: "按名称或 ID 搜索…",
      active: "活跃",
      idle: "空闲",
      empty: "还没有 Cell",
      emptyCta: "创建第一个 Cell",
      createdAt: "创建时间",
    },
    cellNew: {
      title: "创建 Cell",
      subtitle: "一次调用,你的 Cell 就绪",
      required: "必填",
      lazyHint: "懒创建 — SQLite 文件在首次访问时生成。",
      regionAuto: "自动(最低延迟)",
      name: "名称",
      namePlaceholder: "例如 my-app",
      nameHint: "只用于展示,可用任意字符串",
      ensureHint: "必填;同名 Cell 幂等复用(ensure),不会重复创建。",
      create: "创建",
      creating: "创建中…",
      cancel: "取消",
      error: "创建失败",
      nameRequired: "请输入名称",
    },
    cellDetail: {
      back: "返回",
      delete: "删除 Cell",
      deleteConfirm: "确定删除该 Cell 吗?此操作不可撤销。",
      deleting: "删除中…",
      connect: "连接",
      stateHealthy: "健康",
      rename: "重命名",
      statStatus: "状态",
      statOperational: "运行正常",
      statAllSystems: "一切正常",
      statAttention: "需要注意",
      storagePct: "占分配容量",
      lifecycle: "生命周期",
      created: "创建时间",
      lastActive: "最近活跃",
      backupHealth: "备份健康",
      lastRun: "上次运行",
      snapshotWal: "快照 + WAL 增量",
      replicationHealth: "复制健康",
      readReplicas: "只读副本",
      advancedDiag: "高级诊断(内部)",
      engine: "引擎",
      engineDesc: "SQLite WAL · SQL + KV · 共享缓存",
      kvKeys: "KV 键数",
      sqlTables: "SQL 表数",
      keysSuffix: "个键",
      tablesSuffix: "张表",
      durability: "持久性",
      durabilityNormal: "normal (WAL fsync)",
      tabs: { overview: "概览", sql: "SQL", kv: "KV", backups: "备份", replication: "复制", usage: "用量", settings: "设置" },
      notFound: "Cell 不存在",
      backToCells: "返回 Cells",
      deleteCellConfirmTemplate: "确定删除 Cell {name}?其数据将被移除。",
    },
    connect: {
      title: "连接 Cell",
      subtitle: "用 SDK 或 REST 连接你的 Cell",
      apiKeyLabel: "API Key",
      copy: "复制",
      copied: "已复制",
      ts: "TypeScript",
      py: "Python",
      http: "HTTP",
    },
    apiKeys: {
      title: "API Keys",
      subtitle: "为你的应用创建密钥",
      create: "创建 Key",
      creating: "创建中…",
      keyName: "Key 名称",
      keyNamePlaceholder: "例如 production",
      created: "Key 创建成功",
      createdOnce: "密钥只显示一次,请立即保存。",
      copy: "复制",
      done: "完成",
      revoke: "撤销",
      revokeConfirm: "确定撤销该 Key 吗?使用它的应用将立即失效。",
      revoked: "已撤销",
      name: "名称",
      createdAt: "创建时间",
      status: "状态",
      active: "有效",
      empty: "还没有 API Key",
      error: "操作失败",
      nameRequired: "请输入名称",
    },
    usage: {
      title: "用量",
      subtitle: "按租户、按 Cell 计量",
      allCells: "全部 Cell",
      custom: "自定义",
      cardRequests: "请求数",
      cardBytesIn: "入站字节",
      cardSqlRw: "SQL 读/写",
      cardKvRw: "KV 读/写",
      cardEgress: "出站流量",
      cardCreditsConsumed: "Credits 消耗",
      cardNoteStable: "用量稳定",
      cardNoteGrowth: "较上月增长",
      cardNoteRunRate: "预估月消耗",
      chartTitle: "计算与数据操作",
      chartRequests: "请求",
      chartSqlOps: "SQL 操作",
      chartKvOps: "KV 操作",
      consumptionByCell: "按 Cell 消耗",
      exportCsv: "导出 CSV",
      thCellId: "Cell ID",
      thData: "数据 (GB)",
      thCredits: "Credits",
      thRequests: "请求数",
      thRegion: "区域",
      cardStorage: "存储",
      currentStorage: "当前存储",
      totalRequests: "请求总数",
      totalBytesIn: "入站字节",
      totalBytesOut: "出站字节",
      totalStorage: "存储字节",
      period: "时间范围",
      last24h: "最近 24 小时",
      last7d: "最近 7 天",
      last30d: "最近 30 天",
      requestsChart: "请求趋势",
      byMetric: "按指标",
      metric: "指标",
      value: "用量",
      empty: "暂无用量数据",
    },
    credits: {
      title: "Credits",
      subtitle: "预充值 + 按用量计费",
      balance: "余额",
      available: "可用",
      redeem: "兑换邀请码",
      redeemPlaceholder: "CMB-XXXX-XXXX-XXXX",
      redeeming: "兑换中…",
      redeemed: "兑换成功",
      error: "兑换失败",
      codeRequired: "请输入邀请码",
      buy: "购买 Credits",
      consumedMtd: "本月已消耗 (MTD)",
      estRemaining: "预估剩余用量",
      monthlyAllowance: "月额度",
      viewOlder: "查看更早记录",
      thDate: "日期",
      thDescription: "说明",
      thBalance: "余额",
      transactions: "账本明细",
      type: "类型",
      amount: "金额",
      createdAt: "时间",
      empty: "暂无交易记录",
      grant: "发放",
      usage: "用量",
      redemption: "兑换",
    },
    account: {
      title: "账户",
      subtitle: "个人资料、偏好、新手引导与你的活动——全部存储在 Combee。",
      tabs: { profile: "个人资料", prefs: "偏好设置", onboarding: "新手引导", snippets: "SQL 片段", activity: "最近活动" },
      profile: {
        displayName: "显示名称",
        locale: "界面语言",
        timezone: "时区",
        save: "保存",
        saved: "已保存",
        username: "用户名",
        saveError: "保存失败",
        identity: "身份信息",
      },
      prefs: {
        defaultRange: "默认时间范围",
        defaultRegion: "默认区域",
        pageSize: "表格每页行数",
        save: "保存",
        saved: "已保存",
        consolePrefs: "控制台偏好",
        prefsHint: "应用于表格、图表与默认 Cell 区域。",
      },
      onboarding: {
        createdCell: "已创建第一个 Cell",
        createdKey: "已创建 API Key",
        firstRequest: "已发出第一个请求",
        completedAt: "完成于",
        notStarted: "未开始",
        progress: "进度",
        completeMsg: "新手引导已完成 — 欢迎使用 Combee。",
        pendingMsg: "完成以上步骤以结束新手引导。",
      },
      snippets: {
        title: "保存的 SQL 片段",
        save: "保存片段",
        savedTitle: "片段标题",
        empty: "还没有保存的片段",
        emptyHint: "还没有片段 — 从 SQL 工作台保存一个。",
        delete: "删除",
        deleteConfirm: "确定删除该片段?",
      },
      activity: {
        recentCells: "最近访问的 Cells",
        history: "查询历史",
        empty: "暂无记录",
        historyHint: "仅截断的 SQL — 参数永不被存储。",
        noRecent: "暂无最近访问的 Cell。",
        noQueries: "暂无查询记录。",
      },
    },
    sql: {
      run: "运行",
      running: "运行中…",
      clear: "清空",
      placeholder: "输入 SQL…(如 SELECT 1)",
      results: "结果",
      rows: "行",
      error: "查询失败",
      noResult: "无结果(仅影响行数)",
      saveSnippet: "保存片段",
      snippetTitle: "片段标题",
      snippetSaved: "已保存片段",
      snippetPrompt: "输入片段标题:",
    },
    kv: {
      key: "Key",
      value: "Value",
      ttl: "TTL(秒)",
      ttlHint: "留空 = 永不过期",
      browse: "浏览",
      singleKey: "单 Key 操作",
      scan: "扫描",
      get: "读取",
      set: "写入",
      del: "删除",
      notFound: "Key 不存在",
      result: "结果",
      keyRequired: "请输入 Key",
    },
    backups: {
      title: "备份",
      create: "创建备份",
      noticeArchived: "增量备份已归档到对象存储。",
      showing: "显示",
      restoreConfirmTitle: "启动恢复流程?",
      restoreDesc: "即将把",
      critical: "警告",
      destructiveWarn: "这是破坏性操作。所有当前连接将被断开,此时间点之后写入的数据将永久丢失。",
      confirmRestore: "确认恢复",
      creating: "创建中…",
      backupId: "备份 ID",
      size: "大小",
      status: "状态",
      createdAt: "创建时间",
      restore: "恢复",
      restoring: "恢复中…",
      restoreConfirm: "确定从该备份恢复?当前数据将被覆盖。",
      restored: "恢复完成",
      empty: "暂无备份",
      error: "操作失败",
      statusCompleted: "完成",
      statusFailed: "失败",
      statusPending: "进行中",
    },
    replication: {
      title: "复制",
      role: "角色",
      primary: "主节点",
      replica: "副本",
      peer: "对端节点",
      lag: "复制延迟",
      failover: "故障转移",
      failoverConfirm: "确定提升副本为主节点?",
      failoverDone: "故障转移完成",
      noReplica: "暂无副本",
      error: "操作失败",
      primaryToSecondary: "主 → 副本同步",
      continuous: "连续逻辑复制运行中。",
      disabled: "该 Cell 的复制已禁用。",
      off: "关闭",
      enabled: "已启用",
      disabledLabel: "已禁用",
      secondary: "副本",
      lastSync: "最近同步",
      controls: "控制",
      disableReplica: "停用副本",
      enableReplica: "启用副本",
      advancedOps: "高级操作",
      manualFailoverDesc: "手动故障转移会将副本提升为主节点。此操作可能造成短暂中断。",
      typeToConfirm: "输入 Cell 名称确认",
      initiateFailover: "启动故障转移",
    },
    cellUsage: {
      title: "用量",
      requests: "请求",
      bytesIn: "入站字节",
      bytesOut: "出站字节",
      storage: "存储字节",
      empty: "暂无用量数据",
    },
    common: {
      loading: "加载中…",
      error: "出错了",
      cancel: "取消",
      save: "保存",
      delete: "删除",
      empty: "暂无数据",
      retry: "重试",
      unauthorized: "未登录",
    },
  },

  en: {
    meta: { title: "Combee Cloud", description: "One app, one Cell. SQL + KV included." },
    shell: {
      product: "Combee Cloud",
      overview: "Overview",
      cells: "Cells",
      apiKeys: "API Keys",
      usage: "Usage",
      credits: "Credits",
      account: "Account",
      createCell: "Create Cell",
      logOut: "Log out",
      user: "User",
    },
    welcome: {
      title: "Welcome to Combee",
      subtitle: "Create your first Cell and make your first request. You'll be up and running in under 5 minutes.",
      steps: [
        { title: "Create your first Cell", desc: "Provision a secure environment." },
        { title: "Create an API Key", desc: "Authenticate your application." },
        { title: "Install the SDK", desc: "Add Combee to your project." },
        { title: "Run a SQL or KV request", desc: "Execute queries securely." },
      ],
      openDocs: "Open Docs",
      continue: "Continue to Cell",
      continueTo: "Continue",
      ts: "TypeScript",
      py: "Python",
    },
    login: {
      title: "Sign in to Combee Cloud",
      subtitle: "Sign in with your API Key (private alpha, invite-only).",
      username: "Username",
      password: "Password",
      apiKey: "API Key",
      apiKeyPlaceholder: "cmb_sk_…",
      signIn: "Sign in",
      noAccount: "No account yet?",
      createAccount: "Create one",
      invalid: "Sign-in failed, please check your API key",
    },
    register: {
      title: "Create account",
      subtitle: "Registration requires an Alpha access code (1,000 credits included).",
      username: "Username",
      password: "Password",
      confirm: "Confirm password",
      accessCode: "Alpha access code",
      accessCodePlaceholder: "CMB-XXXX-XXXX-XXXX",
      accessCodeHint: "Invite code = voucher, single-use",
      signUp: "Sign up",
      haveAccount: "Already have an account?",
      signIn: "Sign in",
      errorPrefix: "Registration failed:",
      passwordMismatch: "Passwords do not match",
      doneWelcome: "Welcome to Combee",
      doneSub: "Your default API Key was created automatically — shown once, save it now.",
      yourApiKey: "Your API key",
      onlyOnce: "This key is shown only once.",
      copy: "Copy",
      copied: "Copied",
      goToDashboard: "Go to Dashboard",
    },
    overview: {
      title: "Overview",
      subtitle: "Your Combee instance at a glance",
      totalLabel: "Total",
      statCells: "Cells",
      statActive: "Active",
      statRequests: "Requests",
      statStorage: "Storage",
      statCredits: "Credits balance",
      recentCells: "Recent Cells",
      recentCellsSub: "Recently created or accessed data Cells.",
      viewAll: "View all",
      thCell: "Cell",
      thStatus: "Status",
      thRegion: "Region",
      thStorage: "Storage",
      thRequests: "Requests",
      thLastActive: "Last Active",
      stateActive: "Active",
      stateIdle: "Idle",
      stateReplicating: "Replicating",
      stateDegraded: "Degraded",
      empty: "No cells yet",
      emptyCta: "Create your first Cell",
    },
    cells: {
      title: "Cells",
      subtitle: "One app, one Cell",
      allStatus: "All Status",
      allRegions: "All Regions",
      idLabel: "ID",
      noMatch: "No cells match your filters.",
      newCell: "New Cell",
      search: "Search",
      searchPlaceholder: "Search by name or id…",
      active: "Active",
      idle: "Idle",
      empty: "No cells yet",
      emptyCta: "Create your first Cell",
      createdAt: "Created",
    },
    cellNew: {
      title: "Create Cell",
      subtitle: "One call, and your Cell is ready",
      required: "REQUIRED",
      lazyHint: "Lazy-created — the SQLite file appears on first access.",
      regionAuto: "Automatic (Lowest Latency)",
      name: "Name",
      namePlaceholder: "e.g. my-app",
      nameHint: "Display only, any string works",
      ensureHint: "Required; idempotent ensure — reusing the same name returns the existing Cell.",
      create: "Create",
      creating: "Creating…",
      cancel: "Cancel",
      error: "Failed to create",
      nameRequired: "Name is required",
    },
    cellDetail: {
      back: "Back",
      delete: "Delete Cell",
      deleteConfirm: "Delete this Cell? This cannot be undone.",
      deleting: "Deleting…",
      connect: "Connect",
      stateHealthy: "Healthy",
      rename: "Rename",
      statStatus: "Status",
      statOperational: "Operational",
      statAllSystems: "All systems go",
      statAttention: "Attention needed",
      storagePct: "% of allocated capacity",
      lifecycle: "Lifecycle",
      created: "Created",
      lastActive: "Last Active",
      backupHealth: "Backup Health",
      lastRun: "Last run",
      snapshotWal: "Snapshot + WAL incremental",
      replicationHealth: "Replication Health",
      readReplicas: "Read replicas",
      advancedDiag: "Advanced Diagnostics (Internal)",
      engine: "Engine",
      engineDesc: "SQLite WAL · SQL + KV · shared cache",
      kvKeys: "KV Keys",
      sqlTables: "SQL Tables",
      keysSuffix: "keys",
      tablesSuffix: "tables",
      durability: "Durability",
      durabilityNormal: "normal (WAL fsync)",
      tabs: { overview: "Overview", sql: "SQL", kv: "KV", backups: "Backups", replication: "Replication", usage: "Usage", settings: "Settings" },
      notFound: "Cell not found",
      backToCells: "Back to Cells",
      deleteCellConfirmTemplate: "Delete cell {name}? This removes its data.",
    },
    connect: {
      title: "Connect Cell",
      subtitle: "Connect your Cell with the SDK or REST",
      apiKeyLabel: "API Key",
      copy: "Copy",
      copied: "Copied",
      ts: "TypeScript",
      py: "Python",
      http: "HTTP",
    },
    apiKeys: {
      title: "API Keys",
      subtitle: "Keys for your applications",
      create: "Create Key",
      creating: "Creating…",
      keyName: "Key name",
      keyNamePlaceholder: "e.g. production",
      created: "Key created",
      createdOnce: "The key is shown only once — save it now.",
      copy: "Copy",
      done: "Done",
      revoke: "Revoke",
      revokeConfirm: "Revoke this key? Apps using it will fail immediately.",
      revoked: "Revoked",
      name: "Name",
      createdAt: "Created",
      status: "Status",
      active: "Active",
      empty: "No API keys yet",
      error: "Operation failed",
      nameRequired: "Name is required",
    },
    usage: {
      title: "Usage",
      subtitle: "Metered per tenant, per cell",
      allCells: "All Cells",
      custom: "Custom",
      cardRequests: "Requests",
      cardBytesIn: "Bytes in",
      cardSqlRw: "SQL R/W",
      cardKvRw: "KV R/W",
      cardEgress: "Egress",
      cardCreditsConsumed: "Credits Consumed",
      cardNoteStable: "Stable usage",
      cardNoteGrowth: "Growth vs last 30d",
      cardNoteRunRate: "Est. run rate",
      chartTitle: "Compute & Data Ops",
      chartRequests: "Requests",
      chartSqlOps: "SQL Ops",
      chartKvOps: "KV Ops",
      consumptionByCell: "Consumption by Cell",
      exportCsv: "Export CSV",
      thCellId: "Cell ID",
      thData: "Data (GB)",
      thCredits: "Credits",
      thRequests: "Requests",
      thRegion: "Region",
      cardStorage: "Storage",
      currentStorage: "Current storage",
      totalRequests: "Total requests",
      totalBytesIn: "Bytes in",
      totalBytesOut: "Bytes out",
      totalStorage: "Storage bytes",
      period: "Period",
      last24h: "Last 24 hours",
      last7d: "Last 7 days",
      last30d: "Last 30 days",
      requestsChart: "Requests over time",
      byMetric: "By metric",
      metric: "Metric",
      value: "Value",
      empty: "No usage data yet",
    },
    credits: {
      title: "Credits",
      subtitle: "Prepaid + usage-based billing",
      balance: "Balance",
      available: "Available",
      redeem: "Redeem invite code",
      redeemPlaceholder: "CMB-XXXX-XXXX-XXXX",
      redeeming: "Redeeming…",
      redeemed: "Redeemed",
      error: "Redemption failed",
      codeRequired: "Code is required",
      buy: "Buy Credits",
      consumedMtd: "Consumed (MTD)",
      estRemaining: "Est. Remaining Usage",
      monthlyAllowance: "of monthly allowance",
      viewOlder: "View Older Transactions",
      thDate: "Date",
      thDescription: "Description",
      thBalance: "Balance",
      transactions: "Ledger",
      type: "Type",
      amount: "Amount",
      createdAt: "Time",
      empty: "No transactions yet",
      grant: "Grant",
      usage: "Usage",
      redemption: "Redemption",
    },
    account: {
      title: "Account",
      subtitle: "Profile, preferences, onboarding and your activity — all stored in Combee.",
      tabs: { profile: "Profile", prefs: "Preferences", onboarding: "Onboarding", snippets: "Snippets", activity: "Activity" },
      profile: {
        displayName: "Display name",
        locale: "Language",
        timezone: "Timezone",
        save: "Save",
        saved: "Saved",
        username: "Username",
        saveError: "Failed to save",
        identity: "Identity",
      },
      prefs: {
        defaultRange: "Default time range",
        defaultRegion: "Default region",
        pageSize: "Table page size",
        save: "Save",
        saved: "Saved",
        consolePrefs: "Console Preferences",
        prefsHint: "Applies to tables, charts and the default cell region.",
      },
      onboarding: {
        createdCell: "Created first Cell",
        createdKey: "Created an API Key",
        firstRequest: "Ran first request",
        completedAt: "Completed at",
        notStarted: "Not started",
        progress: "Progress",
        completeMsg: "Onboarding complete — welcome to Combee.",
        pendingMsg: "Complete the steps above to finish onboarding.",
      },
      snippets: {
        title: "Saved SQL snippets",
        save: "Save snippet",
        savedTitle: "Snippet title",
        empty: "No saved snippets",
        emptyHint: "No snippets yet — save one from the SQL workspace.",
        delete: "Delete",
        deleteConfirm: "Delete this snippet?",
      },
      activity: {
        recentCells: "Recent Cells",
        history: "Query history",
        empty: "No records yet",
        historyHint: "Truncated SQL only — parameters never stored.",
        noRecent: "No recent cells.",
        noQueries: "No queries yet.",
      },
    },
    sql: {
      run: "Run",
      running: "Running…",
      clear: "Clear",
      placeholder: "Enter SQL… (e.g. SELECT 1)",
      results: "Results",
      rows: "rows",
      error: "Query failed",
      noResult: "No results (rows affected only)",
      saveSnippet: "Save snippet",
      snippetTitle: "Snippet title",
      snippetSaved: "Snippet saved",
      snippetPrompt: "Enter a title for this snippet:",
    },
    kv: {
      key: "Key",
      value: "Value",
      ttl: "TTL (s)",
      ttlHint: "Empty = no expiry",
      browse: "Browse",
      singleKey: "Single key",
      scan: "Scan",
      get: "Get",
      set: "Set",
      del: "Delete",
      notFound: "Key not found",
      result: "Result",
      keyRequired: "Key is required",
    },
    backups: {
      title: "Backups",
      create: "Create backup",
      noticeArchived: "Incremental backup archived to object storage.",
      showing: "Showing",
      restoreConfirmTitle: "Initiate Restore Procedure?",
      restoreDesc: "You are about to restore",
      critical: "CRITICAL",
      destructiveWarn: "This is a destructive action. All current connections will be dropped, and any data written after this timestamp will be permanently lost.",
      confirmRestore: "Confirm Restore",
      creating: "Creating…",
      backupId: "Backup ID",
      size: "Size",
      status: "Status",
      createdAt: "Created",
      restore: "Restore",
      restoring: "Restoring…",
      restoreConfirm: "Restore from this backup? Current data will be overwritten.",
      restored: "Restore completed",
      empty: "No backups yet",
      error: "Operation failed",
      statusCompleted: "Completed",
      statusFailed: "Failed",
      statusPending: "Pending",
    },
    replication: {
      title: "Replication",
      role: "Role",
      primary: "Primary",
      replica: "Replica",
      peer: "Peer node",
      lag: "Replication lag",
      failover: "Failover",
      failoverConfirm: "Promote the replica to primary?",
      failoverDone: "Failover completed",
      noReplica: "No replica",
      error: "Operation failed",
      primaryToSecondary: "Primary to Secondary Sync",
      continuous: "Continuous logical replication active.",
      disabled: "Replication disabled for this cell.",
      off: "Off",
      enabled: "Enabled",
      disabledLabel: "Disabled",
      secondary: "Secondary",
      lastSync: "Last Sync",
      controls: "Controls",
      disableReplica: "Disable Replica",
      enableReplica: "Enable Replica",
      advancedOps: "Advanced Operations",
      manualFailoverDesc: "Manual failover will promote the secondary replica to primary. This action may cause brief downtime.",
      typeToConfirm: "Type cell name to confirm",
      initiateFailover: "Initiate Failover",
    },
    cellUsage: {
      title: "Usage",
      requests: "Requests",
      bytesIn: "Bytes in",
      bytesOut: "Bytes out",
      storage: "Storage bytes",
      empty: "No usage data yet",
    },
    common: {
      loading: "Loading…",
      error: "Something went wrong",
      cancel: "Cancel",
      save: "Save",
      delete: "Delete",
      empty: "No data",
      retry: "Retry",
      unauthorized: "Not signed in",
    },
  },
};

export function getDict(locale: Locale): Dict {
  return dict[locale] ?? dict.zh;
}
