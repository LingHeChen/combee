# Combee Landing —— 静态导出(Next.js output: export)+ nginx。
# 构建上下文为 landing/ 目录:`docker build -t combee-landing ./landing`
# 对外地址为构建期内联(NEXT_PUBLIC_*),由 compose build args / 部署时显式传入。
FROM node:22-alpine AS builder
WORKDIR /app
COPY package.json package-lock.json ./
RUN npm ci
COPY . .
ARG NEXT_PUBLIC_CONSOLE_URL
ARG NEXT_PUBLIC_CONSOLE_REGISTER_PATH
ARG NEXT_PUBLIC_ALPHA_EMAIL
ARG NEXT_PUBLIC_COMBEE_API_URL
ENV NEXT_PUBLIC_CONSOLE_URL=$NEXT_PUBLIC_CONSOLE_URL \
    NEXT_PUBLIC_CONSOLE_REGISTER_PATH=$NEXT_PUBLIC_CONSOLE_REGISTER_PATH \
    NEXT_PUBLIC_ALPHA_EMAIL=$NEXT_PUBLIC_ALPHA_EMAIL \
    NEXT_PUBLIC_COMBEE_API_URL=$NEXT_PUBLIC_COMBEE_API_URL
RUN npm run build

FROM nginx:1.27-alpine
COPY --from=builder /app/out /usr/share/nginx/html
EXPOSE 80
