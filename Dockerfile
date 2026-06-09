# L5M server image. Build:  docker build -t l5m-server .
# Run:    docker run -p 8080:8080 -e L5M_API_KEY=changeme l5m-server
FROM rust:1-bookworm AS builder
WORKDIR /build
COPY . .
RUN cargo build --release -p l5m-server

FROM debian:bookworm-slim
RUN useradd --system --uid 10001 l5m
COPY --from=builder /build/target/release/l5m-server /usr/local/bin/l5m-server
USER l5m
EXPOSE 8080
ENV L5M_BIND=0.0.0.0:8080
ENTRYPOINT ["l5m-server"]
