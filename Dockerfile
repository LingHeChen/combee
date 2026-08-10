# Combee 服务镜像:builder 编译两个二进制,api-server / data-node 各自独立运行时 target。
# 构建:
#   docker buildx build --platform linux/amd64 --target api-server -t combee/api-server .
#   docker buildx build --platform linux/amd64 --target data-node -t combee/data-node .
FROM rust:1.97-bookworm AS builder
WORKDIR /build
# 依赖下载走 rsproxy 镜像(crates.io 直连在部分网络下 TLS 不稳)
COPY .cargo-config /usr/local/cargo/config.toml
COPY Cargo.toml Cargo.lock ./
# workspace 根 package(combee)的 lib 入口,缺了 cargo 无法解析根 manifest
COPY src ./src
COPY crates ./crates
RUN cargo build --release -p combee-api-server -p combee-data-node

FROM debian:bookworm-slim AS api-server
# 直接从 builder 复制 CA 证书,避免运行时 apt-get(构建网络被本机代理/Clash 劫持时会失败)
COPY --from=builder /etc/ssl/certs/ca-certificates.crt /etc/ssl/certs/ca-certificates.crt
COPY --from=builder /build/target/release/combee-api-server /usr/local/bin/
CMD ["/usr/local/bin/combee-api-server"]

FROM debian:bookworm-slim AS data-node
COPY --from=builder /etc/ssl/certs/ca-certificates.crt /etc/ssl/certs/ca-certificates.crt
COPY --from=builder /build/target/release/combee-data-node /usr/local/bin/
CMD ["/usr/local/bin/combee-data-node"]
