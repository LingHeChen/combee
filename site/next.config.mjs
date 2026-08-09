import { createMDX } from 'fumadocs-mdx/next';

const withMDX = createMDX();

/** @type {import('next').NextConfig} */
const config = {
  reactStrictMode: true,
  // 生产镜像(site/Dockerfile)用 standalone 输出,只需 node 运行时。
  output: 'standalone',
};

export default withMDX(config);
