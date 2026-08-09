# Combee Cloud Console

Web 控制台(设计稿:`design/stitch_combee_landing_page_design/` — High-Density Engineering Core)。
**Next.js BFF 架构**:Console 自带后端承担 Auth / Session / Proxy / Aggregation;
**整个前端项目的数据存储走 Combee 自身**(会话存 Combee KV 带 TTL,页面数据经
Combee Public API —— Usage / Credits / Pricing / Cell 生命周期 / 备份复制全部由 Combee 承载)。

**技术栈**:Next.js 16(App Router)+ Tailwind CSS v4 + shadcn/ui + lucide 图标。
设计 tokens(Material 3 dark 主题 / Geist + JetBrains Mono / 琥珀强调色)映射在
`src/app/globals.css`。

## 页面

| 路由 | 页面 | 设计稿对照 |
|---|---|---|
| `/` | Welcome / onboarding | combee_cloud_welcome_onboarding |
| `/overview` | 概览(统计 bento + Recent Cells) | combee_cloud_overview |
| `/cells` | Cells 列表 | combee_cloud_cells |
| `/cells/new` | 创建 Cell 表单 | combee_cloud_create_cell |
| `/cells/[id]` | Cell 详情(**7 tabs**: Overview / SQL / KV / Usage / Backups / Replication / Settings;7 块 bento + Healthy 徽标 + more_vert 下拉 + Advanced Diagnostics) | combee_cloud_cell_overview_blog_prod 等 |
| `/cells/[id]/connect` | 连接指引(TS / Python / curl) | combee_cloud_connect_cell |
| `/api-keys` | API Keys 管理 | combee_cloud_api_keys_management |
| `/usage` | 用量汇总 + 条形图 | combee_cloud_usage |
| `/credits` | 余额 / 兑换 / 账本 | combee_cloud_credits |
| `/account` | 账户设置 | combee_cloud_account_settings |

## BFF 架构

```text
Browser (Console UI)
   │  httpOnly cookie(combee_session)
   ▼
Next.js BFF (web/ 服务端)
   ├── Auth/Session   lib/bff/auth.ts   —— 登录用 Combee API key;会话存 Combee KV(带 TTL)
   ├── Proxy          /api/bff/v1/*     —— 转发到 Combee /v1/*(带会话 key,401 拦截)
   ├── Aggregation    /api/bff/overview —— cells+usage+credits+storage 一次聚合
   └── 页面(server components 经服务层 / client 组件经 /api/bff/*)
   ▼
Combee API Server(数据面;所有数据存储)
   ├── Cell(SQLite)      —— 前端业务数据 + BFF 会话(KV,TTL 24h)
   ├── PostgreSQL         —— 租户/API keys/usage buckets/credits/pricing
   └── 对象存储           —— 备份/恢复
```

- **登录 = 用户名 + 密码**(不再是 API key——Console 是用来签发 key 的,拿 key 登录是死锁);
- 用户凭据存 Combee:`console_users` 表(Session Cell 的 SQL),密码 scrypt 加盐哈希;
- **用户数据全量存 Combee**:Profile(display_name/avatar/locale/timezone)+ Console
  Preferences(默认时间范围/region/table page size/UI)+ Onboarding(从 Combee 实际
  数据推断:cell 数/API key 数/usage)+ 辅助数据(saved SQL snippets、最近访问 Cells、
  Query history —— 仅存截断 SQL,**不含参数**);见 `/api/bff/profile` 与 Account 页;
- 注册时 BFF 服务账号(`COMBEE_BFF_API_KEY`)为用户自动签发专属 Combee key,之后代理请求用该 key(租户隔离在 Combee 侧保持);
- 会话 id 存 Combee KV(`bff:session:{sid}`,TTL 24h,惰性过期);会话 Cell 首次自动创建并持久化到 `.bff-cell-id`;
- 浏览器永不直接触碰 Combee —— 全部经 BFF(代理/聚合)转发。

## 测试计划(自设计,共 3 层)

### 1. 单元测试(vitest + Testing Library,`npx vitest run`)

| 文件 | 覆盖 |
|---|---|
| `src/lib/utils.test.ts` | `formatBytes` / `shortId` / `formatTime` 边界 |
| `src/lib/mock.test.ts` | mock 数据完整性:overview 与 cells 一致(12 cells / 9 active)、字段合法、详情查找 |
| `src/app/cells/new/page.test.tsx` | 表单渲染、输入 name → 提交 → 路由跳转;KV panel 交互(SET 输出) |

### 2. E2E(Playwright chromium,`PLAYWRIGHT_BROWSERS_PATH=0 node e2e-qa.mjs`)

对 `next start`(生产构建,端口 3100)的 Chrome(headless)自动化,**40 项断言**:

- overview:统计卡、进度条、Recent Cells 6 列表格(状态徽标);
- cells:搜索过滤、状态/区域筛选;
- cell 详情:**7 tabs**、Healthy 徽标、bento、Advanced Diagnostics 折叠、
  SQL workspace(Schema + 编辑器 + Success 结果)、KV browser 双栏、Backups Restore 危险确认、
  Replication(failover 需输入 cell 名确认)、cell usage 图表 + Usage by Service 表;
- create-cell 表单(4 个 region 选项);api-keys **创建 modal**(明文一次性 + "Copy this key now");
- usage 全站:范围过滤器 + 图表 + Consumption 表;credits:余额/兑换/账本;
- account:Identity(Email/Verified/TenantID copy)+ Security(Active Sessions + Sign out);
- welcome 移动端 + Quick Start 竖线步骤 + 代码卡;
- **全程 console 无 error**。

截图输出到 `/tmp/web-shots/*.png`(overview / cells / cell-detail / create-cell / api-keys / credits / welcome-mobile)。

### 3. 构建验证

`npm run build`(Turbopack):11/11 页面静态生成,TypeScript 严格模式 0 错误。

## 运行(BFF + 真实 Combee)

```bash
# 1. 起 Combee API Server(数据面;dev 单进程即可)
cargo run -p combee-api-server

# 2. 构建并启动 Console(BFF)
COMBEE_API_URL=http://127.0.0.1:8080 npm run build
COMBEE_API_URL=http://127.0.0.1:8080 npx next start -p 3100
# 打开 http://127.0.0.1:3100 → 注册/登录(用户名+密码)

# Closed Alpha:生成邀请码(默认 1000 Alpha Credits/码)
COMBEE_ADMIN_TOKEN=<token> ./scripts/generate-invites.sh 10
# 用户注册时填写该码 → 自动获得对应 Credits

# 生产环境变量
COMBEE_API_URL        # Combee API Server 地址(必填)
COMBEE_BFF_API_KEY    # BFF 服务账号 key(建表/为用户签发专属 key;dev auth=off 可空)
COMBEE_BFF_CELL       # BFF 会话 Cell id(可选;默认自动创建并写入 .bff-cell-id)
COMBEE_CONSOLE_SIGNUP # 注册模式:code(默认,必须 Alpha access code)/ open(任意)/ off(关闭)
```

## 测试

```bash
npx vitest run                 # 单元测试
node e2e-qa.mjs                # E2E(BFF 模式):自动起 Combee + Next,登录→页面→登出 16 项断言
```

## 国际化(中英双语,默认中文)

- **默认中文**:`src/middleware.ts` 把无语言前缀的请求 307 到 `/{locale}/…`(cookie `combee-locale=en` 时才英文);
- **路由**:页面位于 `src/app/[locale]/…`(`/zh/overview`、`/en/overview` 等);BFF API(`/api/bff/*`)不受语言影响;
- **切换器**:Shell 顶栏 `LangSwitcher`(中文 / EN)写 cookie 并跳转同路径对应语言;登录/注册/welcome 页可通过进入任意已登录页切换;
- **字典**:`src/lib/i18n.ts` — `Dict` 接口(TS 强约束 en/zh 结构一致)+ 全部文案;
  client 组件用 `useT()`(context),server 组件用 `getDict(params.locale)`;
- **SEO**:每个 locale 独立 `html lang`(zh-CN / en)与 metadata。

## 测试

- `npx vitest run` — 单测(页面渲染 + 组件 + 国际化字典)
- `node e2e-qa.mjs` — 全栈 E2E(BFF):登录/注册(邀请码)/overview/cells/创建 Cell/SQL 高亮/
  usage/credits/profile/snippets/**语言切换 zh→en→zh**/**根路径默认 /zh**/登出重登/零 console 错误
