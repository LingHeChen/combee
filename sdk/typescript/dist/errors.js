//! 错误模型(与 docs/API.md §3 对齐):稳定 code → 类型化异常。
//! 每个错误携带 requestId 与 HTTP status。
export class CombeeError extends Error {
    code;
    status;
    requestId;
    constructor(code, message, status, requestId) {
        super(message);
        this.name = "CombeeError";
        this.code = code;
        this.status = status;
        this.requestId = requestId;
    }
}
export class AuthenticationError extends CombeeError {
    constructor(message, requestId) {
        super("unauthorized", message, 401, requestId);
    }
}
export class PermissionDeniedError extends CombeeError {
    constructor(message, requestId) {
        super("forbidden", message, 403, requestId);
    }
}
export class CellNotFoundError extends CombeeError {
    constructor(message, requestId) {
        super("database_not_found", message, 404, requestId);
    }
}
export class ApiKeyNotFoundError extends CombeeError {
    constructor(message, requestId) {
        super("api_key_not_found", message, 404, requestId);
    }
}
export class InvalidRequestError extends CombeeError {
    constructor(message, requestId) {
        super("invalid_request", message, 400, requestId);
    }
}
export class SqlError extends CombeeError {
    constructor(message, requestId) {
        super("sql", message, 400, requestId);
    }
}
export class SqlTimeoutError extends CombeeError {
    constructor(message, requestId) {
        super("sql_timeout", message, 408, requestId);
    }
}
export class RateLimitError extends CombeeError {
    constructor(message, requestId) {
        super("quota_exceeded", message, 429, requestId);
    }
}
export class QuotaExceededError extends CombeeError {
    constructor(message, requestId) {
        super("quota_exceeded", message, 429, requestId);
    }
}
export class InsufficientCreditsError extends CombeeError {
    constructor(message, requestId) {
        super("insufficient_credits", message, 402, requestId);
    }
}
export class DataNodeUnavailableError extends CombeeError {
    constructor(message, requestId) {
        super("data_node_unavailable", message, 503, requestId);
    }
}
export class InternalServerError extends CombeeError {
    constructor(message, status = 500, requestId) {
        super("internal", message, status, requestId);
    }
}
/** 由稳定 code 映射到类型化错误。 */
export function fromErrorBody(code, message, status, requestId) {
    switch (code) {
        case "unauthorized":
            return new AuthenticationError(message, requestId);
        case "forbidden":
            return new PermissionDeniedError(message, requestId);
        case "database_not_found":
            return new CellNotFoundError(message, requestId);
        case "api_key_not_found":
            return new ApiKeyNotFoundError(message, requestId);
        case "invalid_request":
            return new InvalidRequestError(message, requestId);
        case "sql":
            return new SqlError(message, requestId);
        case "quota_exceeded":
            return new QuotaExceededError(message, requestId);
        case "insufficient_credits":
            return new InsufficientCreditsError(message, requestId);
        default:
            if (status && status >= 500)
                return new InternalServerError(message, status, requestId);
            return new CombeeError(code, message, status, requestId);
    }
}
