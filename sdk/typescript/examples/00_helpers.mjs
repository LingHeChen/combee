// 示例共享:读取环境变量并创建客户端。
import { Combee } from "../dist/index.js";

export function client() {
  return new Combee({
    baseUrl: process.env.COMBEE_URL ?? "http://127.0.0.1:8080",
    apiKey: process.env.COMBEE_API_KEY ?? "dev-key",
  });
}
