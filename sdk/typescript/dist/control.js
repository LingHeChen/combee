//! API Keys / Usage / Credits / Pricing(User Control Plane)。
export class ApiKeys {
    http;
    constructor(http) {
        this.http = http;
    }
    async create(name) {
        // 当前服务端不存 name;保留签名以对齐 SDK_SPEC(创建时 name 仅客户端备注)
        void name;
        // 服务端返回 {key, record:{...}} → 平铺为 CreatedApiKey
        const r = await this.http.request("POST", "/v1/api-keys", {}, { idempotencyKey: `key:${name ?? Date.now()}` });
        return { ...r.record, key: r.key };
    }
    async list() {
        return this.http.request("GET", "/v1/api-keys");
    }
    async revoke(id) {
        await this.http.request("DELETE", `/v1/api-keys/${id}`);
    }
}
export class Usage {
    http;
    constructor(http) {
        this.http = http;
    }
    async summary(opts) {
        const q = toQuery(opts);
        return this.http.request("GET", `/v1/usage/summary${q}`);
    }
    async cell(cellId, opts) {
        const q = toQuery(opts);
        return this.http.request("GET", `/v1/cells/${cellId}/usage${q}`);
    }
    async timeseries(opts) {
        const parts = [`metric=${encodeURIComponent(opts.metric)}`];
        parts.push(`interval=${opts.interval ?? "minute"}`);
        if (opts.from)
            parts.push(`from=${encodeURIComponent(opts.from)}`);
        if (opts.to)
            parts.push(`to=${encodeURIComponent(opts.to)}`);
        return this.http.request("GET", `/v1/usage/timeseries?${parts.join("&")}`);
    }
}
export class Credits {
    http;
    constructor(http) {
        this.http = http;
    }
    async balance() {
        return this.http.request("GET", "/v1/credits/balance");
    }
    async transactions(limit = 100, cursor) {
        return this.http.paginate("/v1/credits/transactions", limit, cursor);
    }
    async redeem(code) {
        return this.http.request("POST", "/v1/credits/redeem", { code });
    }
}
export class Pricing {
    http;
    constructor(http) {
        this.http = http;
    }
    async get() {
        return this.http.request("GET", "/v1/pricing");
    }
}
function toQuery(opts) {
    const parts = [];
    if (opts?.from)
        parts.push(`from=${encodeURIComponent(opts.from)}`);
    if (opts?.to)
        parts.push(`to=${encodeURIComponent(opts.to)}`);
    return parts.length ? `?${parts.join("&")}` : "";
}
