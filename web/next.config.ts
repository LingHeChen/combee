import type { NextConfig } from "next";

const nextConfig: NextConfig = {
  /* config options here */
  reactCompiler: true,
  // 生产镜像(web/Dockerfile)用 standalone 输出,只需 node 运行时。
  output: "standalone",
};

export default nextConfig;
