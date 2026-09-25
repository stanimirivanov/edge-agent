# syntax=docker/dockerfile:1.7@sha256:a57df69d0ea827fb7266491f2813635de6f17269be881f696fbfdf2d83dda33e

ARG RUST_IMAGE=rust:1.98.1-alpine3.22@sha256:a1796ca6fa216d6727b5f61c69e4c665b120b1a4dcb969639e2f25f1ed309456
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
