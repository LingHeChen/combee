# Combee Cloud Console(web/)生产镜像 —— Next.js standalone。
# 构建上下文为 web/ 目录:`docker build -t combee-web ./web`
# NEXT_PUBLIC_COMBEE_API_URL 在构建期注入(浏览器端 API 地址,如 https://api.combee.cloud)。
FROM node:22-alpine AS deps
WORKDIR /app
COPY package.json package-lock.json ./
RUN npm ci

FROM node:22-alpine AS builder
WORKDIR /app
COPY --from=deps /app/node_modules ./node_modules
COPY . .
ARG NEXT_PUBLIC_COMBEE_API_URL
ENV NEXT_PUBLIC_COMBEE_API_URL=$NEXT_PUBLIC_COMBEE_API_URL
RUN npm run build

FROM node:22-alpine AS runner
WORKDIR /app
ENV NODE_ENV=production
# Next standalone server 用 HOSTNAME/PORT 决定绑定地址。Docker 会把 HOSTNAME 设成容器 id,
# 导致 Next 只绑容器 hostname → healthcheck(wget 127.0.0.1:3000)连不上 → 判 unhealthy 反复重启。
# 显式绑 0.0.0.0(所有网卡,含 loopback)修复。
ENV HOSTNAME=0.0.0.0
ENV PORT=3000
COPY --from=builder /app/.next/standalone ./
COPY --from=builder /app/.next/static ./.next/static
# public 静态资源(combee-*.png 等)standalone 不会自动包含,必须显式复制
COPY --from=builder /app/public ./public
EXPOSE 3000
CMD ["node", "server.js"]
