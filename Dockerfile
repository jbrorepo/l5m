# L5M server image. Build:  docker build -t l5m-server .
# Run:    docker run -p 8080:8080 -e L5M_API_KEY=changeme l5m-server
FROM rust:1-bookworm AS builder
WORKDIR /build
COPY . .
RUN cargo build --release -p l5m-server

FROM debian:bookworm-slim
# wget is for container healthchecks; ca-certificates for any TLS sidecar use.
RUN apt-get update \
    && apt-get install -y --no-install-recommends wget ca-certificates \
    && rm -rf /var/lib/apt/lists/* \
    && useradd --system --uid 10001 l5m \
    && mkdir -p /data && chown l5m:l5m /data
COPY --from=builder /build/target/release/l5m-server /usr/local/bin/l5m-server
USER l5m
EXPOSE 8080
VOLUME /data
ENV L5M_BIND=0.0.0.0:8080
HEALTHCHECK --interval=10s --timeout=3s --start-period=5s \
    CMD wget -qO- http://127.0.0.1:8080/healthz || exit 1
ENTRYPOINT ["l5m-server"]
