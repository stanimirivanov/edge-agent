# syntax=docker/dockerfile:1.7

ARG RUST_IMAGE=rust:1.98.1-alpine3.22
FROM ${RUST_IMAGE} AS builder

WORKDIR /workspace

COPY Cargo.toml Cargo.lock rust-toolchain.toml ./
COPY crates ./crates
COPY services ./services

RUN cargo build --locked --release --workspace --bins

ARG SERVICE
RUN case "${SERVICE}" in \
        audit-projector|execution-simulator|gateway|market-data|research) ;; \
        *) echo "unsupported EdgeAgent service: ${SERVICE}" >&2; exit 64 ;; \
    esac \
    && mkdir -p /out \
    && cp "/workspace/target/release/edgeagent-${SERVICE}" /out/edgeagent-service

FROM scratch AS runtime

ARG SERVICE
ARG SOURCE_REVISION=unknown

LABEL org.opencontainers.image.title="edgeagent-${SERVICE}" \
      org.opencontainers.image.description="EdgeAgent ${SERVICE} service" \
      org.opencontainers.image.source="https://github.com/stanimirivanov/edge-agent" \
      org.opencontainers.image.revision="${SOURCE_REVISION}" \
      org.opencontainers.image.licenses="Apache-2.0"

COPY --from=builder --chown=65532:65532 /out/edgeagent-service /edgeagent-service

USER 65532:65532
ENTRYPOINT ["/edgeagent-service"]
CMD ["help"]
