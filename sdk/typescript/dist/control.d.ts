import { Http } from "./http.js";
import type { ApiKeyInfo, CreatedApiKey, CreditBalance, CreditTransaction, Page, RedeemResult, UsageSummary } from "./types.js";
export declare class ApiKeys {
    private http;
    constructor(http: Http);
    create(name?: string): Promise<CreatedApiKey>;
    list(): Promise<ApiKeyInfo[]>;
    revoke(id: string): Promise<void>;
}
export declare class Usage {
    private http;
    constructor(http: Http);
    summary(opts?: {
        from?: string;
        to?: string;
    }): Promise<UsageSummary>;
    cell(cellId: string, opts?: {
        from?: string;
        to?: string;
    }): Promise<UsageSummary>;
    timeseries(opts: {
        metric: string;
        interval?: "minute" | "hour" | "day";
        from?: string;
        to?: string;
    }): Promise<Array<{
        bucket_start: string;
        value: number;
    }>>;
}
export declare class Credits {
    private http;
    constructor(http: Http);
    balance(): Promise<CreditBalance>;
    transactions(limit?: number, cursor?: string): Promise<Page<CreditTransaction>>;
    redeem(code: string): Promise<RedeemResult>;
}
export declare class Pricing {
    private http;
    constructor(http: Http);
    get(): Promise<{
        version: number;
        effective_at: number;
        units: Record<string, unknown>;
    }>;
}
