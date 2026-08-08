import type { Page } from "./types.js";
export interface CombeeOptions {
    baseUrl: string;
    apiKey: string;
    timeoutMs?: number;
    userAgent?: string;
    retry?: {
        maxAttempts?: number;
        baseDelayMs?: number;
        maxDelayMs?: number;
    };
}
export type HttpMethod = "GET" | "POST" | "PUT" | "DELETE";
/** 内部请求执行器(SDK 各资源共用)。 */
export declare class Http {
    private baseUrl;
    private apiKey;
    private timeoutMs;
    private userAgent;
    private retry;
    constructor(opts: CombeeOptions);
    /** 可安全自动重试的方法(GET / 读类)。 */
    private isSafeRetry;
    private attempt;
    /** 统一请求入口:对安全方法做有限退避重试。 */
    request<T>(method: HttpMethod, path: string, body?: unknown, opts?: {
        idempotencyKey?: string;
    }): Promise<T>;
    /** 游标分页 GET。 */
    paginate<T>(path: string, limit?: number, cursor?: string): Promise<Page<T>>;
}
