# Combee 生产镜像:API Server 与 Data Node 共用(运行时通过 command 选择)。
FROM rust:1.97-bookworm AS builder
WORKDIR /build
COPY Cargo.toml Cargo.lock ./
COPY crates ./crates
RUN cargo build --release -p combee-api-server -p combee-data-node

FROM debian:bookworm-slim
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/*
COPY --from=builder /build/target/release/combee-api-server /usr/local/bin/
COPY --from=builder /build/target/release/combee-data-node /usr/local/bin/
CMD ["/usr/local/bin/combee-data-node"]
