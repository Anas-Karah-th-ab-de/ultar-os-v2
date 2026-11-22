# ultraos-server als Container:  docker compose up -d   (im Ordner deploy/)
FROM rust:1-bookworm AS bau
WORKDIR /src
COPY Cargo.toml Cargo.lock ./
COPY src ./src
RUN cargo build --release

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates curl && rm -rf /var/lib/apt/lists/* \
 && useradd -r -u 10001 ultraos && install -d -o ultraos /var/lib/ultraos
COPY --from=bau /src/target/release/ultraos-server /usr/local/bin/ultraos-server
USER ultraos
ENV ULTRAOS_BIND=0.0.0.0:8080 ULTRAOS_DB=/var/lib/ultraos/ultraos.sqlite ULTRAOS_FILES=/var/lib/ultraos/files RUST_LOG=info
EXPOSE 8080
HEALTHCHECK --interval=15s --timeout=3s CMD curl -sf http://127.0.0.1:8080/v1/health || exit 1
CMD ["ultraos-server"]
