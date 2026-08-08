/** 游标分页结果。 */
export interface Page<T> {
    items: T[];
    nextCursor: string | null;
}
/** SQL 参数值。 */
export type SqlParam = null | boolean | number | string;
export interface SqlStatement {
    sql: string;
    params?: SqlParam[];
}
export interface SqlQueryResult<T = Record<string, unknown>> {
    columns: string[];
    rows: T[];
}
export interface SqlExecuteResult {
    rows_affected: number;
    last_insert_rowid?: number | string | null;
}
/** Cell 信息(懒创建,状态为 idle/active/unavailable)。 */
export interface CellInfo {
    id: string;
    tenant_id: string;
    state: string;
    created_at: number;
    storage_node_id?: string | null;
}
export interface CreateCellInput {
    name?: string;
    region?: string;
}
export type KvValue = string;
export interface KvGetResult {
    value: string | null;
    ttl_seconds: number | null;
}
export interface KvSetOptions {
    ttl?: number;
    condition?: "nx" | "xx";
}
export interface KvSetResult {
    written: boolean;
}
export type KvTtl = {
    state: "expires";
    seconds: number;
} | {
    state: "persistent";
} | {
    state: "missing";
};
export interface BackupInfo {
    id: string;
    type: "snapshot" | "incremental";
    created_at: string;
    size_bytes?: number;
}
export interface ReplicationStatus {
    enabled: boolean;
    primary_node?: string;
    replica_node?: string;
    generation?: number;
}
export interface ApiKeyInfo {
    id: string;
    tenant_id: string;
    key_hash: string;
    created_at: number;
    revoked_at?: number | null;
}
export interface CreatedApiKey {
    id: string;
    tenant_id: string;
    key_hash: string;
    created_at: number;
    key: string;
}
export interface UsageSummary {
    period: {
        from: string;
        to: string;
    };
    operations: {
        kv_reads: number;
        kv_writes: number;
        sql_reads: number;
        sql_writes: number;
    };
    request_count: number;
    bytes_in: number;
    bytes_out: number;
    current_storage_bytes: number;
}
export interface CreditBalance {
    available: string;
    reserved: string;
    currency: "CREDIT";
    updated_at: number;
}
export type CreditTransactionType = "recharge" | "usage" | "grant" | "voucher" | "refund" | "adjustment";
export interface CreditTransaction {
    id: string;
    tenant_id: string;
    txn_type: CreditTransactionType;
    amount_units: string;
    pricing_version?: number | null;
    reference_id?: string | null;
    description?: string | null;
    created_at: number;
    balance_after?: string | null;
}
export interface RedeemResult {
    credits_added: string;
    balance: string;
    already_redeemed: boolean;
}
