// Console 数据类型(对齐 docs/API.md 契约)。


export interface CellStat {
  id: string;
  name: string | null;
  state: string;
  created_at: number;
  storage_bytes: number;
  kv_keys: number;
  sql_tables: number;
  region: string;
  requests_24h: number;
  last_active: string;
  storage_pct: number;
  backup_health: string;
  replication: string;
  diagnostics: Record<string, unknown>;
}

export interface OverviewData {
  cellsTotal: number;
  cellsActive: number;
  requests: number;
  storageBytes: number;
  creditsBalance: string;
  recentCells: CellStat[];
}

export interface UsageSummary {
  period: { from: string; to: string };
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

export interface ApiKeyInfo {
  id: string;
  tenant_id: string;
  key_hash: string;
  created_at: number;
  revoked_at?: number | null;
}

export interface CreatedApiKey extends ApiKeyInfo {
  key: string;
}

export interface CreditBalance {
  available: string;
  reserved: string;
  currency: "CREDIT";
  updated_at: number;
}

export interface CreditTransaction {
  id: string;
  tenant_id: string;
  txn_type: string;
  amount_units: string;
  pricing_version?: number | null;
  reference_id?: string | null;
  description?: string | null;
  created_at: number;
  balance_after?: string | null;
}

export interface BackupInfo {
  id: string;
  type: string;
  created_at: string;
  size_bytes?: number;
}
