ARG RUST_VERSION="1.93.0"
ARG IMG_SHA256=""

FROM ghcr.io/sbellem/rust-xous:${RUST_VERSION}-slim-bullseye${IMG_SHA256:+@sha256:${IMG_SHA256}} AS builder
ARG task="dabao"
ARG app=""
ARG os="xous"

WORKDIR /home/baozi/xous-core
COPY --chown=baozi:baozi . .
RUN --mount=type=cache,target=/usr/local/cargo/registry,uid=1000,gid=1000 \
    --mount=type=cache,target=/home/baozi/xous-core/target,uid=1000,gid=1000 \
        cargo --locked xtask "${task}" ${app:+"${app}"} --no-verify \
        && mkdir -p artifacts \
        && cp target/riscv32imac-unknown-${os}-elf/release/*.uf2 artifacts/ 2>/dev/null || true \
        && cp target/riscv32imac-unknown-${os}-elf/release/*.img artifacts/ 2>/dev/null || true \
        && cp target/riscv32imac-unknown-${os}-elf/release/*.bin artifacts/ 2>/dev/null || true

FROM scratch
COPY --from=builder /home/baozi/xous-core/artifacts/ /
