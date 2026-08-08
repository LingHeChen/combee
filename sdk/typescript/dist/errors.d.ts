export declare class CombeeError extends Error {
    readonly code: string;
    readonly status?: number;
    readonly requestId?: string;
    constructor(code: string, message: string, status?: number, requestId?: string);
}
export declare class AuthenticationError extends CombeeError {
    constructor(message: string, requestId?: string);
}
export declare class PermissionDeniedError extends CombeeError {
    constructor(message: string, requestId?: string);
}
export declare class CellNotFoundError extends CombeeError {
    constructor(message: string, requestId?: string);
}
export declare class ApiKeyNotFoundError extends CombeeError {
    constructor(message: string, requestId?: string);
}
export declare class InvalidRequestError extends CombeeError {
    constructor(message: string, requestId?: string);
}
export declare class SqlError extends CombeeError {
    constructor(message: string, requestId?: string);
}
export declare class SqlTimeoutError extends CombeeError {
    constructor(message: string, requestId?: string);
}
export declare class RateLimitError extends CombeeError {
    constructor(message: string, requestId?: string);
}
export declare class QuotaExceededError extends CombeeError {
    constructor(message: string, requestId?: string);
}
export declare class InsufficientCreditsError extends CombeeError {
    constructor(message: string, requestId?: string);
}
export declare class DataNodeUnavailableError extends CombeeError {
    constructor(message: string, requestId?: string);
}
export declare class InternalServerError extends CombeeError {
    constructor(message: string, status?: number, requestId?: string);
}
/** 由稳定 code 映射到类型化错误。 */
export declare function fromErrorBody(code: string, message: string, status?: number, requestId?: string): CombeeError;
