//! Cells / SQL / KV / Backups / Replication(Data Plane + Cell 管理)。
/** SQL 命名空间(挂在一个 Cell 下)。 */
export class Sql {
    http;
    cellId;
    constructor(http, cellId) {
        this.http = http;
        this.cellId = cellId;
    }
    async query(sql, params) {
        // 服务端 rows 为位置数组;SDK 按 columns 映射为对象(对齐 SDK_SPEC §8.1)
        const raw = await this.http.request("POST", `/v1/databases/${this.cellId}/sql`, { sql, params });
        const rows = raw.rows.map((row) => Object.fromEntries(raw.columns.map((c, i) => [c, row[i]])));
        return { columns: raw.columns, rows };
    }
    async execute(sql, params) {
        return this.http.request("POST", `/v1/databases/${this.cellId}/sql`, {
            sql,
            params,
        });
    }
    async transaction(statements) {
        return this.http.request("POST", `/v1/databases/${this.cellId}/transaction`, { statements });
    }
}
/** KV 命名空间(Redis-style,SQLite-backed)。 */
export class Kv {
    http;
    cellId;
    constructor(http, cellId) {
        this.http = http;
        this.cellId = cellId;
    }
    async get(key) {
        const r = await this.http.request("GET", `/v1/databases/${this.cellId}/kv/${encodeURIComponent(key)}`);
        return r.value;
    }
    async getJson(key) {
        const v = await this.get(key);
        return v === null ? null : JSON.parse(v);
    }
    async set(key, value, opts = {}) {
        const r = await this.http.request("PUT", `/v1/databases/${this.cellId}/kv/${encodeURIComponent(key)}`, {
            value,
            ttl_seconds: opts.ttl ?? null,
            nx: opts.condition === "nx",
            xx: opts.condition === "xx",
        });
        return r.written;
    }
    async setJson(key, value, opts = {}) {
        return this.set(key, JSON.stringify(value), opts);
    }
    async delete(key) {
        const r = await this.http.request("DELETE", `/v1/databases/${this.cellId}/kv/${encodeURIComponent(key)}`);
        return r.deleted;
    }
    async exists(key) {
        const r = await this.http.request("POST", `/v1/databases/${this.cellId}/kv/ops/exists`, { keys: [key] });
        return r[0];
    }
    async mget(keys) {
        const r = await this.http.request("POST", `/v1/databases/${this.cellId}/kv/ops/mget`, { keys });
        return r.values;
    }
    async mset(entries) {
        const items = Object.entries(entries).map(([key, value]) => ({ key, value }));
        await this.http.request("POST", `/v1/databases/${this.cellId}/kv/ops/mset`, { items });
    }
    async ttl(key) {
        // 服务端 ttl 为批量端点:{keys:[...]} → Vec<Option<i64>>
        const r = await this.http.request("POST", `/v1/databases/${this.cellId}/kv/ops/ttl`, { keys: [key] });
        const ttl = r[0];
        // 服务端语义:null = key 不存在;-1 = persistent;>=0 = 剩余秒
        if (ttl === null)
            return { state: "missing" };
        if (ttl < 0)
            return { state: "persistent" };
        return { state: "expires", seconds: ttl };
    }
    async expire(key, seconds) {
        const r = await this.http.request("POST", `/v1/databases/${this.cellId}/kv/ops/expire`, { key, ttl_seconds: seconds });
        return r.updated;
    }
    async persist(key) {
        // ttl_seconds: null = 移除过期(服务端 Option<u64> None = persist)
        const r = await this.http.request("POST", `/v1/databases/${this.cellId}/kv/ops/expire`, { key, ttl_seconds: null });
        return r.updated;
    }
    async incr(key, delta = 1) {
        const r = await this.http.request("POST", `/v1/databases/${this.cellId}/kv/ops/incr`, { key, delta });
        return r.value;
    }
    async decr(key, delta = 1) {
        return this.incr(key, -delta);
    }
}
/** 备份/恢复(对象存储)。 */
export class Backups {
    http;
    cellId;
    constructor(http, cellId) {
        this.http = http;
        this.cellId = cellId;
    }
    async create() {
        return this.http.request("POST", `/v1/databases/${this.cellId}/backup`);
    }
    async createIncremental() {
        return this.http.request("POST", `/v1/databases/${this.cellId}/backup/incr`);
    }
    async restore(version) {
        await this.http.request("POST", `/v1/databases/${this.cellId}/restore`, version ? { version } : {});
    }
    async restoreLatest() {
        await this.restore();
    }
}
/** 复制状态(单 replica)。 */
export class Replication {
    http;
    cellId;
    constructor(http, cellId) {
        this.http = http;
        this.cellId = cellId;
    }
    async get() {
        const r = await this.http.request("GET", `/v1/databases/${this.cellId}/replication`);
        return {
            enabled: Boolean(r.replica_node),
            replica_node: r.replica_node ?? undefined,
            ...("storage_node_id" in r ? { primary_node: String(r.storage_node_id) } : {}),
            ...("generation" in r ? { generation: Number(r.generation) } : {}),
        };
    }
    async enable(replicaNode) {
        await this.http.request("POST", `/v1/databases/${this.cellId}/replication`, { replica_node: replicaNode });
    }
    async disable() {
        await this.http.request("DELETE", `/v1/databases/${this.cellId}/replication`);
    }
}
/** 一个 Cell 的本地句柄(可延迟绑定,首次调用才发请求)。 */
export class Cell {
    http;
    id;
    sql;
    kv;
    backups;
    replication;
    constructor(http, id) {
        this.http = http;
        this.id = id;
        this.sql = new Sql(http, id);
        this.kv = new Kv(http, id);
        this.backups = new Backups(http, id);
        this.replication = new Replication(http, id);
    }
    async info() {
        // 通过列表取回该 Cell 信息(无单点 GET 端点时使用)
        const all = await this.http.request("GET", `/v1/databases?limit=1000`);
        const found = all.find((c) => c.id === this.id);
        if (!found) {
            throw new (await import("./errors.js")).CellNotFoundError(`cell not found: ${this.id}`);
        }
        return found;
    }
    async delete() {
        await this.http.request("DELETE", `/v1/databases/${this.id}`);
    }
}
/** Cells 资源(租户级)。 */
export class Cells {
    http;
    constructor(http) {
        this.http = http;
    }
    async create(input) {
        const r = await this.http.request("POST", "/v1/databases", {}, { idempotencyKey: input?.name ? `cell:${input.name}` : undefined });
        return new Cell(this.http, r.id);
    }
    async get(id) {
        const cell = new Cell(this.http, id);
        await cell.info();
        return cell;
    }
    async list(limit = 100, cursor) {
        // 服务端 list 返回数组(非游标分页);包装为 Page 以对齐 SDK_SPEC
        void cursor;
        const sep = "?";
        const arr = await this.http.request("GET", `/v1/databases${sep}limit=${limit}`);
        return { items: arr, nextCursor: null };
    }
    async delete(id) {
        await this.http.request("DELETE", `/v1/databases/${id}`);
    }
}
