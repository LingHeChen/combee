//! Combee SDK 入口。

import { Http, type CombeeOptions } from "./http.js";
import { ApiKeys, Credits, Pricing, Usage } from "./control.js";
import { Cell, Cells } from "./cells.js";

export * from "./errors.js";
export * from "./types.js";
export { Cell, Cells } from "./cells.js";
export type { CombeeOptions } from "./http.js";
export type { Page } from "./types.js";

/**
 * Combee 客户端。
 *
 * ```ts
 * const combee = new Combee({ baseUrl, apiKey });
 * const cell = await combee.cells.create({ name: "my-app" });
 * await cell.sql.execute("CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT)");
 * await cell.kv.set("session:abc", "user:1", { ttl: 3600 });
 * ```
 */
export class Combee {
  readonly cells: Cells;
  readonly apiKeys: ApiKeys;
  readonly usage: Usage;
  readonly credits: Credits;
  readonly pricing: Pricing;
  private http: Http;

  constructor(opts: CombeeOptions) {
    this.http = new Http(opts);
    this.cells = new Cells(this.http);
    this.apiKeys = new ApiKeys(this.http);
    this.usage = new Usage(this.http);
    this.credits = new Credits(this.http);
    this.pricing = new Pricing(this.http);
  }

  /** 按 id 绑定 Cell(本地句柄,不立即发请求)。 */
  cell(id: string): Cell {
    return new Cell(this.http, id);
  }
}
