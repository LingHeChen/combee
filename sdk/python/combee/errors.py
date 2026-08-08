"""错误模型:稳定 code → 类型化异常(对齐 docs/API.md §3)。"""

from __future__ import annotations


class CombeeError(Exception):
    code: str
    status: int | None
    request_id: str | None

    def __init__(self, code: str, message: str, status: int | None = None, request_id: str | None = None):
        super().__init__(message)
        self.code = code
        self.status = status
        self.request_id = request_id


class AuthenticationError(CombeeError):
    def __init__(self, message: str, request_id: str | None = None):
        super().__init__("unauthorized", message, 401, request_id)


class PermissionDeniedError(CombeeError):
    def __init__(self, message: str, request_id: str | None = None):
        super().__init__("forbidden", message, 403, request_id)


class CellNotFoundError(CombeeError):
    def __init__(self, message: str, request_id: str | None = None):
        super().__init__("database_not_found", message, 404, request_id)


class ApiKeyNotFoundError(CombeeError):
    def __init__(self, message: str, request_id: str | None = None):
        super().__init__("api_key_not_found", message, 404, request_id)


class InvalidRequestError(CombeeError):
    def __init__(self, message: str, request_id: str | None = None):
        super().__init__("invalid_request", message, 400, request_id)


class SqlError(CombeeError):
    def __init__(self, message: str, request_id: str | None = None):
        super().__init__("sql", message, 400, request_id)


class SqlTimeoutError(CombeeError):
    def __init__(self, message: str, request_id: str | None = None):
        super().__init__("sql_timeout", message, 408, request_id)


class RateLimitError(CombeeError):
    def __init__(self, message: str, request_id: str | None = None):
        super().__init__("quota_exceeded", message, 429, request_id)


class QuotaExceededError(CombeeError):
    def __init__(self, message: str, request_id: str | None = None):
        super().__init__("quota_exceeded", message, 429, request_id)


class InsufficientCreditsError(CombeeError):
    def __init__(self, message: str, request_id: str | None = None):
        super().__init__("insufficient_credits", message, 402, request_id)


class DataNodeUnavailableError(CombeeError):
    def __init__(self, message: str, request_id: str | None = None):
        super().__init__("data_node_unavailable", message, 503, request_id)


class InternalServerError(CombeeError):
    def __init__(self, message: str, status: int = 500, request_id: str | None = None):
        super().__init__("internal", message, status, request_id)


_CODE_TO_ERROR = {
    "unauthorized": AuthenticationError,
    "forbidden": PermissionDeniedError,
    "database_not_found": CellNotFoundError,
    "api_key_not_found": ApiKeyNotFoundError,
    "invalid_request": InvalidRequestError,
    "sql": SqlError,
    "quota_exceeded": QuotaExceededError,
    "insufficient_credits": InsufficientCreditsError,
}


def from_error_body(code: str, message: str, status: int | None = None, request_id: str | None = None) -> CombeeError:
    cls = _CODE_TO_ERROR.get(code)
    if cls is not None:
        return cls(message, request_id)
    if status is not None and status >= 500:
        return InternalServerError(message, status, request_id)
    return CombeeError(code, message, status, request_id)
