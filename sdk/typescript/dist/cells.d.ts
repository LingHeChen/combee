import { Http } from "./http.js";
import type { BackupInfo, CellInfo, CreateCellInput, KvSetOptions, KvTtl, KvValue, Page, ReplicationStatus, SqlExecuteResult, SqlParam, SqlQueryResult, SqlStatement } from "./types.js";
/** SQL 命名空间(挂在一个 Cell 下)。 */
export declare class Sql {
    private http;
    private cellId;
    constructor(http: Http, cellId: string);
    query<T = Record<string, unknown>>(sql: string, params?: SqlParam[]): Promise<SqlQueryResult<T>>;
    execute(sql: string, params?: SqlParam[]): Promise<SqlExecuteResult>;
    transaction(statements: SqlStatement[]): Promise<SqlExecuteResult[]>;
}
/** KV 命名空间(Redis-style,SQLite-backed)。 */
export declare class Kv {
    private http;
    private cellId;
    constructor(http: Http, cellId: string);
    get(key: string): Promise<string | null>;
    getJson<T = unknown>(key: string): Promise<T | null>;
    set(key: string, value: KvValue, opts?: KvSetOptions): Promise<boolean>;
    setJson(key: string, value: unknown, opts?: KvSetOptions): Promise<boolean>;
    delete(key: string): Promise<boolean>;
    exists(key: string): Promise<boolean>;
    mget(keys: string[]): Promise<Array<string | null>>;
    mset(entries: Record<string, string>): Promise<void>;
    ttl(key: string): Promise<KvTtl>;
    expire(key: string, seconds: number): Promise<boolean>;
    persist(key: string): Promise<boolean>;
    incr(key: string, delta?: number): Promise<number>;
    decr(key: string, delta?: number): Promise<number>;
}
/** 备份/恢复(对象存储)。 */
export declare class Backups {
    private http;
    private cellId;
    constructor(http: Http, cellId: string);
    create(): Promise<BackupInfo>;
    createIncremental(): Promise<BackupInfo>;
    restore(version?: string): Promise<void>;
    restoreLatest(): Promise<void>;
}
/** 复制状态(单 replica)。 */
export declare class Replication {
    private http;
    private cellId;
    constructor(http: Http, cellId: string);
    get(): Promise<ReplicationStatus>;
    enable(replicaNode: string): Promise<void>;
    disable(): Promise<void>;
}
/** 一个 Cell 的本地句柄(可延迟绑定,首次调用才发请求)。 */
export declare class Cell {
    private http;
    readonly id: string;
    readonly sql: Sql;
    readonly kv: Kv;
    readonly backups: Backups;
    readonly replication: Replication;
    constructor(http: Http, id: string);
    info(): Promise<CellInfo>;
    delete(): Promise<void>;
}
/** Cells 资源(租户级)。 */
export declare class Cells {
    private http;
    constructor(http: Http);
    create(input?: CreateCellInput): Promise<Cell>;
    get(id: string): Promise<Cell>;
    list(limit?: number, cursor?: string): Promise<Page<CellInfo>>;
    delete(id: string): Promise<void>;
}
