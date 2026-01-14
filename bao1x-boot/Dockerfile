FROM rust:1.92-slim AS toolchain

ARG RUST_VERSION="1.92.0"
ARG EXTRA="1"

RUN set -eux; \
    apt-get update && apt-get install -y --no-install-recommends \
        curl \
        git \
        unzip \
    && rm -rf /var/lib/apt/lists/* \
    && download_url="https://github.com/betrusted-io/rust/releases/download"; \
    zipfile="riscv32imac-unknown-xous_${RUST_VERSION}.zip"; \
    curl -L -O "${download_url}/${RUST_VERSION}.${EXTRA}/${zipfile}"; \
    unzip "${zipfile}" -d "$(rustc --print sysroot)"; \
    rm "${zipfile}"

RUN rustup target add riscv32imac-unknown-none-elf


FROM toolchain AS builder
WORKDIR /usr/src/xous-core
COPY . .
RUN --mount=type=cache,target=/root/.cargo/registry cargo xtask bao1x-alt-boot1
RUN --mount=type=cache,target=/root/.cargo/registry cargo xtask bao1x-boot1


FROM scratch
COPY --from=builder /usr/src/xous-core/target/riscv32imac-unknown-none-elf/release/bao1x-boot1.uf2 /
COPY --from=builder /usr/src/xous-core/target/riscv32imac-unknown-none-elf/release/bao1x-alt-boot1.uf2 /
