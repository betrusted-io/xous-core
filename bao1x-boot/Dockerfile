ARG RUST_VERSION="1.92.0"
ARG IMG_SHA256=""
FROM rust:${RUST_VERSION}-slim-bullseye${IMG_SHA256:+@sha256:${IMG_SHA256}} AS toolchain

ARG EXTRA="1"

RUN set -eux; \
    apt-get update && apt-get install -y --no-install-recommends \
        curl \
        git \
        unzip \
    && rm -rf /var/lib/apt/lists/* \
    && download_url="https://github.com/betrusted-io/rust/releases/download"; \
    zipfile="riscv32imac-unknown-xous_${RUST_VERSION}.zip"; \
    curl --proto '=https' --tlsv1.2 -fsSL -O "${download_url}/${RUST_VERSION}.${EXTRA}/${zipfile}"; \
    sha256="fc7e7210444ee33c96956c4015eeafe96a588c12d68385ad763a4d1f3c6ec811"; \
    echo "${sha256} *${zipfile}" | sha256sum --strict --check -; \
    unzip "${zipfile}" -d "$(rustc --print sysroot)"; \
    rm "${zipfile}"

RUN rustup target add riscv32imac-unknown-none-elf


FROM toolchain AS builder
RUN useradd --create-home --uid 1000 baozi
USER baozi
WORKDIR /home/baozi/xous-core
COPY --chown=baozi:baozi . .
RUN --mount=type=cache,target=/home/baozi/.cargo/registry,uid=1000,gid=1000 cargo --locked xtask bao1x-alt-boot1
RUN --mount=type=cache,target=/home/baozi/.cargo/registry,uid=1000,gid=1000 cargo --locked xtask bao1x-boot1


FROM scratch
COPY --from=builder /home/baozi/xous-core/target/riscv32imac-unknown-none-elf/release/bao1x-boot1.uf2 /
COPY --from=builder /home/baozi/xous-core/target/riscv32imac-unknown-none-elf/release/bao1x-alt-boot1.uf2 /
