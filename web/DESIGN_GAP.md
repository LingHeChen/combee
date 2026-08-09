# Combee Cloud Console — 设计稿 vs 当前实现差距报告

> **状态(2026-08-08 第二轮):G1–G7、M1–M7、S1–S2 已全部补齐**(见下「三」补全记录)。

> 对照:`design/stitch_combee_landing_page_design/`(15 个 code.html 原型)
> 当前实现:`web/`(Next.js + Tailwind v4 + shadcn,已提交 45ed9e8)
> 日期:2026-08-08

## 一、设计稿功能详情(逐页)

### 公共框架
- **SideNav(w-64)**:品牌区(logo + `Combee Cloud` + 版本徽标)+ 导航 Overview/Cells/API Keys/Usage/Credits + 琥珀 `Create Cell` 按钮 + Docs/Support;
- **TopNav**:Projects/Regions/Network(当前项琥珀下划线)+ 通知铃铛/设置/头像;
- **视觉**:深黑背景、Geist+JetBrains Mono、Material Symbols 图标、琥珀 `#ffb95f` 强调、卡片深灰底+描边、六边形元素(数据/状态徽标)、hover 琥珀过渡、按钮 mousedown 按压反馈。

### 1. account_settings
Identity(SEC-01):Primary Email `engineer@combee.cloud` + `Verified` 徽标;Tenant ID `tnt_98af7c2b4e109d`(mono + copy 按钮);Created At `October 12, 2023 · 14:32 UTC`。Security(SEC-02):Active Sessions 两条(`Mac OS · Chrome` + Current 徽标 + IP/城市;`iOS · Safari` + 2h ago)+ 红色 `Sign out all other sessions`。

### 2. api_keys_management
标题 + 琥珀 `Create API Key`(modal)。表列:**Name / Prefix / Last Used / Created**。行:`Production Environment cmb_sk_84fa•••••• 2 mins ago`、`Staging CI/CD cmb_sk_39db•••••• 5 days ago`、`Local Dev - Alex cmb_sk_11cc•••••• Never`。Modal:明文 key 一次性 + 复制 + **"Copy this key now. You will not be able to view it again."** warning。

### 3. backups(cell 级)
内部 Tabs(Overview/Metrics/**Backups**/Configuration)+ `Create Backup` 按钮。表列:**Created Date / Type / Size / Status / Actions**;行含 `Incremental 42.1MB Healthy Restore`、`Snapshot 1.4GB Healthy Restore`、`In Progress`(动画、Restore 禁用)。**Restore 危险确认 modal**:红色警告 "CRITICAL: destructive action... data after this timestamp will be permanently lost"。底部 retention 文案。

### 4. cell_overview
标题 + `Healthy` 徽标(hex-pulse)+ meta(Region Tokyo / Cell ID)+ `Connect` + more_vert 下拉(**Rename / Create Backup / 红色 Delete Cell**)。**7 tabs**:Overview/SQL/KV/Usage/Backups/Replication/Settings。**7 块 bento**:Status=Operational / Region / Storage 1.2GB+进度条 / Requests(30d) 4.5M / Created+Last Active / Backup Health / Replication Health。`Advanced Diagnostics (Internal)` 折叠 JSON。

### 5. cells
标题 + **搜索框** + **状态/区域两个筛选下拉**。卡片字段:ID、Active/Idle 徽标、Region、Storage、Requests(24h)、Last Active。示例:prod-eu-cache-01(Frankfurt 4.2GB 1.2M)、dev-ap-worker-02(Tokyo 128MB)、prod-us-stream-11(US-East 1.8GB 890k)。

### 6. connect_cell(模态)
Cell ID 卡(copy)+ API Base URL 卡;Quickstart 代码卡(**tabs TypeScript/Python/HTTP + 语法高亮** + copy);`View Documentation`。

### 7. create_cell(模态)
NAME 字段(REQUIRED 徽标、`#ID-AUTO` 徽标、placeholder `e.g., prod-api-gateway`);REGION 下拉(`Automatic (Lowest Latency)` / US East / US West / EU Central);Cancel + 琥珀 Create Cell。

### 8. credits
`Buy Credits`(描边)+ `Redeem Code`(琥珀)。Bento:Available Credits 大数字 `12,840.52 CRD`(+六边形装饰);Consumed (MTD) `3,245.00` +12%;Est. Remaining `~48 Days` + 进度条。交易表列 **Date / Type / Description / Amount / Balance**,Amount 红(负)/琥珀(正);`View Older Transactions`。

### 9. kv_browser(cell 级)
面包屑 + `Create Key`;**搜索按 pattern**(`user:*, config:*`)。**双栏**:左 KV 列表(Key Name/Value Preview/TTL/Updated,选中高亮,示例 `user:1042:session` TTL 3592s 等 4 行);右 **Key Details 面板**:Identifier(copy)/ Value JSON 编辑器(行号+`Editable`)/ TTL 数字输入 / Type / `Delete Key`(红)+`Save Changes`(琥珀)。

### 10. overview
4 统计卡:Cells `12 Total`(**9 Active** 琥珀六边形 + **3 Idle** 灰)、Requests `2.4M` +14%、Storage `845GB` + **进度条 65%**、Credits `450`。Recent Cells 表 **6 列**:Cell/Status/Region/Storage/Requests/Last Active;状态含 **Replicating**(蓝灰旋转)/ **Degraded**(红)。

### 11. replication(cell 级 tab)
主状态卡:Status=Healthy(琥珀六边形 pulse)/ Replica=Enabled / Region;指标:Replication Lag `120ms`、Last Sync `3s ago`;Controls:`Disable Replica` / `Enable Replica`;Advanced Operations 折叠:手动 failover 需**输入 cell 名确认** + 红色 `Initiate Failover`。

### 12. sql_workspace(cell 级)
子导航 + ⌘K 搜索;**左 Schema 面板**(`public.users` 展开列 uuid/email/timestamptz、sessions、audit_logs);**SQL 编辑器**(多 tab、行号、**语法高亮**、History/Clear/Run);**结果区**(`Success` + 0.042s + 250 rows + 结果表)。

### 13. usage(全站,最大页)
过滤器:`All Cells` + **7D/30D/Custom**。**6 统计卡**:Requests 1.24B / SQL R/W 845M / KV R/W 2.1B / Storage 4.2TB / Egress 12.8TB / **Credits Consumed 24,500**(hex 装饰)。**3 图表**:Compute & Data Ops 堆叠柱状图(30 柱、三色、hover tooltip)、Storage 面积图、Credit Burn 折线图。Consumption by Cell 表(Export CSV)。

### 14. usage(cell 级)
上下文头 + 24H/7D/30D;**6 卡**:Credits Consumed / Requests(双色条 99.8%+0.2%)/ SQL R/W 图例 / Storage / Egress;**2 图**:Operations 柱状图(Reads 蓝灰+Writes 琥珀)、Credits Burn 折线;**Usage by Service 表**(KV Reads/SQL Writes/Storage Base/Egress + Multiplier + Credits Total)。

### 15. welcome_onboarding
无导航居中;**Quick Start Guide 竖线步骤**(4 步:Create Cell/Create API Key/Install SDK/Run request);**右代码卡**(tabs TS/Python + 高亮);`Open Docs` + `Continue to Cell`。

## 二、当前实现 vs 设计稿差距

### 大差距(结构/功能未还原)

| # | 页面 | 差距 |
|---|---|---|
| G1 | account_settings | 内容完全不同:设计是 Identity(Email/Verified/TenantID+copy/CreatedAt)+ Security(Active Sessions + Sign out);当前是 Profile + Workspace 卡 |
| G2 | cell_overview | 设计 7 tabs(缺 Replication/Settings)+ 7 块 bento 统计(Status/Region/Requests 30d/Backup Health/Replication Health/进度条)+ Healthy 徽标 + more_vert 下拉(Rename/Create Backup/Delete)+ Advanced Diagnostics JSON;当前 5 tabs + 4 统计卡 |
| G3 | kv_browser | 设计为独立双栏浏览器(搜索 pattern + KV 列表 + Key Details JSON 编辑器/TTL/Save/Delete);当前只是单表单交互 panel |
| G4 | replication | 整个 Replication tab 缺失(Healthy/Replica/Lag 120ms/Enable-Disable/Failover 确认);当前仅 Overview tab 一行文本 |
| G5 | sql_workspace | 设计有 Schema 面板 + 语法高亮编辑器(多 tab/行号/History)+ 结果区(Success/耗时/rows);当前单 textarea + 简易结果 |
| G6 | usage(全站) | 设计 6 卡(含 Egress/Credits Consumed)+ 时间过滤器 + 3 图表(堆叠柱/面积/折线)+ Consumption 表;当前 4 卡 + CSS bars |
| G7 | cell usage | 设计 6 卡 + 2 图表 + Usage by Service 表(带 multiplier/credits);当前 JSON 文本展示 |

### 中差距(有雏形,缺细节)

| # | 页面 | 差距 |
|---|---|---|
| M1 | api-keys | 缺 Name/Prefix(cmb_sk_84fa••••••)/Last Used 列;创建用 modal(当前 inline 卡)+ "Copy this key now" warning |
| M2 | cells | 缺搜索框 + 状态/区域筛选;卡片缺 Region/Requests(24h)/Last Active 字段 |
| M3 | connect | 应为模态 + Cell ID/API Base URL 卡 + 代码 tabs(TS/Python/HTTP)+ 语法高亮;当前三卡平铺无高亮 |
| M4 | credits | 缺 Buy Credits、Consumed/Est.Remaining 卡(进度条);交易表缺 Date/Balance 列与 Amount 红/绿语义 |
| M5 | overview | 统计卡缺 9 Active/3 Idle 细分、进度条、+14% 变化;Recent Cells 表缺 Region/Requests/Last Active 列与 Replicating/Degraded 状态徽标 |
| M6 | welcome | 设计为竖线步骤 + 代码卡双栏;当前 4 卡片;缺代码卡(tabs TS/Python) |
| M7 | backups | 表列缺 Status/Actions;缺 Restore 危险确认 modal;缺 In Progress 状态与 retention 文案 |

### 小差距

| # | 页面 | 差距 |
|---|---|---|
| S1 | create-cell | 表单应为模态 + REGION 下拉选项(Automatic/US East/US West/EU Central)+ REQUIRED/#ID-AUTO 徽标;当前页面表单选项 auto/us-east/eu-west |
| S2 | 全局 | 版本徽标 v2.4.0-stable(vs v0.1.0-alpha,合理差异)、按钮 mousedown 按压反馈、Material Symbols 图标风格(vs lucide) |

## 三、补全记录(第二轮已完成)

- P0:G2 cell-overview(7 tabs + 7 bento + Healthy 徽标 + 下拉 + Diagnostics)、G5 sql-workspace(Schema + 高亮编辑器 + Success 结果)、G3 kv-browser(双栏 + 搜索 + JSON 编辑 + Save/Delete)、G1 account(Identity + Security Sessions + Sign out);
- P1:G4 replication tab(健康/指标/Enable-Disable/Failover 确认)、G6 usage 全站(6 卡 + 7D/30D + 堆叠柱图 + Consumption 表)、G7 cell usage(6 卡 + 柱图 + Usage by Service 表)、M1 api-keys modal + Name/Prefix/Last Used、M7 backups Restore 危险确认 + Status 列;
- P2:M2 cells 搜索/筛选、M3 connect 信息卡 + 代码 tabs、M4 credits Buy/Consumed/Est.Remaining + Date/Balance 列、M5 overview 6 列表 + Replicating/Degraded、M6 welcome 竖线步骤 + 代码卡、S1 create region 选项 + REQUIRED/#ID-AUTO;
- 验证:单元 10/10、build 11/11、E2E 40/40(零 console 错误)。

## 四、原差距清单(供追溯)

1. **P0(核心体验)**:G2 cell-overview(7 tabs + bento)、G5 sql-workspace、G3 kv-browser、G1 account-settings;
2. **P1(功能完整)**:G4 replication、G6/G7 usage 图表、M1 api-keys modal、M7 backups Restore;
3. **P2(打磨)**:M2-M6 细节、S1-S2 形式统一。
