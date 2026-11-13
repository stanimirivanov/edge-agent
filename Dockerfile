# syntax=docker/dockerfile:1.7@sha256:a57df69d0ea827fb7266491f2813635de6f17269be881f696fbfdf2d83dda33e

ARG RUST_IMAGE=rust:1.98.1-alpine3.22@sha256:a1796ca6fa216d6727b5f61c69e4c665b120b1a4dcb969639e2f25f1ed309456
FROM ${RUST_IMAGE} AS builder

WORKDIR /workspace

COPY Cargo.toml Cargo.lock rust-toolchain.toml ./
COPY .cargo/config.toml ./.cargo/config.toml
COPY .sqlx ./.sqlx
COPY crates ./crates
COPY services ./services
COPY scripts/run_explanation_owned.py scripts/qualify_explanation_model.py scripts/qualify_daily.py ./scripts/

# SQLx checks the committed schema descriptions, never a build-time database.
ENV SQLX_OFFLINE=true
RUN cargo build --locked --release --workspace --bins

ARG SERVICE
RUN case "${SERVICE}" in \
        audit-projector|execution-simulator|gateway|market-data|research) ;; \
        *) echo "unsupported EdgeAgent service: ${SERVICE}" >&2; exit 64 ;; \
    esac \
    && mkdir -p /out/owners \
    && cp "/workspace/target/release/edgeagent-${SERVICE}" /out/edgeagent-service \
    && if [ "${SERVICE}" = research ]; then \
        cp /workspace/target/release/edgeagent-market-data /out/owners/edgeagent-market-data; \
    fi

FROM scratch AS runtime

ARG SERVICE
ARG SOURCE_REVISION=unknown

LABEL org.opencontainers.image.title="edgeagent-${SERVICE}" \
      org.opencontainers.image.description="EdgeAgent ${SERVICE} service" \
      org.opencontainers.image.source="https://github.com/stanimirivanov/edge-agent" \
      org.opencontainers.image.revision="${SOURCE_REVISION}" \
      org.opencontainers.image.licenses="Apache-2.0"

COPY --from=builder --chown=65532:65532 /out/edgeagent-service /edgeagent-service
COPY --from=builder --chown=65532:65532 /out/owners/ /edgeagent-owner/

# Mozilla roots from the same immutable builder; certificate data license travels with it.
COPY --from=builder /etc/ssl/certs/ca-certificates.crt /etc/ssl/certs/ca-certificates.crt
COPY supply-chain/licenses/ /usr/share/licenses/ca-certificates/

USER 65532:65532
ENTRYPOINT ["/edgeagent-service"]
CMD ["help"]
