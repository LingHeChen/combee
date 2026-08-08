//! Cells / SQL / KV / Backups / Replication(Data Plane + Cell 管理)。

import { Http } from "./http.js";
import type {
  BackupInfo,
  CellInfo,
  CreateCellInput,
  KvGetResult,
  KvSetOptions,
  KvSetResult,
  KvTtl,
  KvValue,
  Page,
  ReplicationStatus,
  SqlExecuteResult,
  SqlParam,
  SqlQueryResult,
  SqlStatement,
} from "./types.js";

/** SQL 命名空间(挂在一个 Cell 下)。 */
export class Sql {
  constructor(
    private http: Http,
    private cellId: string,
  ) {}

  async query<T = Record<string, unknown>>(
    sql: string,
    params?: SqlParam[],
  ): Promise<SqlQueryResult<T>> {
    // 服务端 rows 为位置数组;SDK 按 columns 映射为对象(对齐 SDK_SPEC §8.1)
    const raw = await this.http.request<{ columns: string[]; rows: unknown[][] }>(
      "POST",
      `/v1/databases/${this.cellId}/sql`,
      { sql, params },
    );
    const rows = raw.rows.map((row) =>
      Object.fromEntries(raw.columns.map((c, i) => [c, row[i]])),
    ) as T[];
    return { columns: raw.columns, rows };
  }

  async execute(
    sql: string,
    params?: SqlParam[],
  ): Promise<SqlExecuteResult> {
    return this.http.request<SqlExecuteResult>("POST", `/v1/databases/${this.cellId}/sql`, {
      sql,
      params,
    });
  }

  async transaction(statements: SqlStatement[]): Promise<SqlExecuteResult[]> {
    return this.http.request<SqlExecuteResult[]>(
      "POST",
      `/v1/databases/${this.cellId}/transaction`,
      { statements },
    );
  }
}

/** KV 命名空间(Redis-style,SQLite-backed)。 */
export class Kv {
  constructor(
    private http: Http,
    private cellId: string,
  ) {}

  async get(key: string): Promise<string | null> {
    const r = await this.http.request<KvGetResult>("GET", `/v1/databases/${this.cellId}/kv/${encodeURIComponent(key)}`);
    return r.value;
  }

  async getJson<T = unknown>(key: string): Promise<T | null> {
    const v = await this.get(key);
    return v === null ? null : (JSON.parse(v) as T);
  }

  async set(
    key: string,
    value: KvValue,
    opts: KvSetOptions = {},
  ): Promise<boolean> {
    const r = await this.http.request<KvSetResult>("PUT", `/v1/databases/${this.cellId}/kv/${encodeURIComponent(key)}`, {
      value,
      ttl_seconds: opts.ttl ?? null,
      nx: opts.condition === "nx",
      xx: opts.condition === "xx",
    });
    return r.written;
  }

  async setJson(key: string, value: unknown, opts: KvSetOptions = {}): Promise<boolean> {
    return this.set(key, JSON.stringify(value), opts);
  }

  async delete(key: string): Promise<boolean> {
    const r = await this.http.request<{ deleted: boolean }>("DELETE", `/v1/databases/${this.cellId}/kv/${encodeURIComponent(key)}`);
    return r.deleted;
  }

  async exists(key: string): Promise<boolean> {
    const r = await this.http.request<boolean[]>("POST", `/v1/databases/${this.cellId}/kv/ops/exists`, { keys: [key] });
    return r[0];
  }

  async mget(keys: string[]): Promise<Array<string | null>> {
    const r = await this.http.request<{ values: Array<string | null> }>("POST", `/v1/databases/${this.cellId}/kv/ops/mget`, { keys });
    return r.values;
  }

  async mset(entries: Record<string, string>): Promise<void> {
    const items = Object.entries(entries).map(([key, value]) => ({ key, value }));
    await this.http.request("POST", `/v1/databases/${this.cellId}/kv/ops/mset`, { items });
  }

  async ttl(key: string): Promise<KvTtl> {
    // 服务端 ttl 为批量端点:{keys:[...]} → Vec<Option<i64>>
    const r = await this.http.request<Array<number | null>>("POST", `/v1/databases/${this.cellId}/kv/ops/ttl`, { keys: [key] });
    const ttl = r[0];
    // 服务端语义:null = key 不存在;-1 = persistent;>=0 = 剩余秒
    if (ttl === null) return { state: "missing" };
    if (ttl < 0) return { state: "persistent" };
    return { state: "expires", seconds: ttl };
  }

  async expire(key: string, seconds: number): Promise<boolean> {
    const r = await this.http.request<{ updated: boolean }>("POST", `/v1/databases/${this.cellId}/kv/ops/expire`, { key, ttl_seconds: seconds });
    return r.updated;
  }

  async persist(key: string): Promise<boolean> {
    // ttl_seconds: null = 移除过期(服务端 Option<u64> None = persist)
    const r = await this.http.request<{ updated: boolean }>("POST", `/v1/databases/${this.cellId}/kv/ops/expire`, { key, ttl_seconds: null });
    return r.updated;
  }

  async incr(key: string, delta = 1): Promise<number> {
    const r = await this.http.request<{ value: number }>("POST", `/v1/databases/${this.cellId}/kv/ops/incr`, { key, delta });
    return r.value;
  }

  async decr(key: string, delta = 1): Promise<number> {
    return this.incr(key, -delta);
  }
}

/** 备份/恢复(对象存储)。 */
export class Backups {
  constructor(
    private http: Http,
    private cellId: string,
  ) {}

  async create(): Promise<BackupInfo> {
    return this.http.request<BackupInfo>("POST", `/v1/databases/${this.cellId}/backup`);
  }

  async createIncremental(): Promise<BackupInfo> {
    return this.http.request<BackupInfo>("POST", `/v1/databases/${this.cellId}/backup/incr`);
  }

  async restore(version?: string): Promise<void> {
    await this.http.request("POST", `/v1/databases/${this.cellId}/restore`, version ? { version } : {});
  }

  async restoreLatest(): Promise<void> {
    await this.restore();
  }
}

/** 复制状态(单 replica)。 */
export class Replication {
  constructor(
    private http: Http,
    private cellId: string,
  ) {}

  async get(): Promise<ReplicationStatus> {
    const r = await this.http.request<{ replica_node: string | null } & Record<string, unknown>>("GET", `/v1/databases/${this.cellId}/replication`);
    return {
      enabled: Boolean(r.replica_node),
      replica_node: r.replica_node ?? undefined,
      ...("storage_node_id" in r ? { primary_node: String(r.storage_node_id) } : {}),
      ...("generation" in r ? { generation: Number(r.generation) } : {}),
    };
  }

  async enable(replicaNode: string): Promise<void> {
    await this.http.request("POST", `/v1/databases/${this.cellId}/replication`, { replica_node: replicaNode });
  }

  async disable(): Promise<void> {
    await this.http.request("DELETE", `/v1/databases/${this.cellId}/replication`);
  }
}

/** 一个 Cell 的本地句柄(可延迟绑定,首次调用才发请求)。 */
export class Cell {
  readonly sql: Sql;
  readonly kv: Kv;
  readonly backups: Backups;
  readonly replication: Replication;

  constructor(
    private http: Http,
    readonly id: string,
  ) {
    this.sql = new Sql(http, id);
    this.kv = new Kv(http, id);
    this.backups = new Backups(http, id);
    this.replication = new Replication(http, id);
  }

  async info(): Promise<CellInfo> {
    // 通过列表取回该 Cell 信息(无单点 GET 端点时使用)
    const all = await this.http.request<CellInfo[]>("GET", `/v1/databases?limit=1000`);
    const found = all.find((c) => c.id === this.id);
    if (!found) {
      throw new (await import("./errors.js")).CellNotFoundError(`cell not found: ${this.id}`);
    }
    return found;
  }

  async delete(): Promise<void> {
    await this.http.request("DELETE", `/v1/databases/${this.id}`);
  }
}

/** Cells 资源(租户级)。 */
export class Cells {
  constructor(private http: Http) {}

  async create(input?: CreateCellInput): Promise<Cell> {
    const r = await this.http.request<{ id: string }>(
      "POST",
      "/v1/databases",
      {},
      { idempotencyKey: input?.name ? `cell:${input.name}` : undefined },
    );
    return new Cell(this.http, r.id);
  }

  async get(id: string): Promise<Cell> {
    const cell = new Cell(this.http, id);
    await cell.info();
    return cell;
  }

  async list(limit = 100, cursor?: string): Promise<Page<CellInfo>> {
    // 服务端 list 返回数组(非游标分页);包装为 Page 以对齐 SDK_SPEC
    void cursor;
    const sep = "?";
    const arr = await this.http.request<CellInfo[]>("GET", `/v1/databases${sep}limit=${limit}`);
    return { items: arr, nextCursor: null };
  }

  async delete(id: string): Promise<void> {
    await this.http.request("DELETE", `/v1/databases/${id}`);
  }
}
