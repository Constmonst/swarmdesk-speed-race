FROM debian:bookworm-slim

ENV DEBIAN_FRONTEND=noninteractive

RUN apt-get update && apt-get install -y --no-install-recommends \
    build-essential \
    ca-certificates \
    curl \
    git \
    gnupg \
    jq \
    pkg-config \
  && mkdir -p /etc/apt/keyrings \
  && curl -fsSL https://cli.github.com/packages/githubcli-archive-keyring.gpg \
     -o /etc/apt/keyrings/githubcli-archive-keyring.gpg \
  && chmod go+r /etc/apt/keyrings/githubcli-archive-keyring.gpg \
  && echo "deb [arch=$(dpkg --print-architecture) signed-by=/etc/apt/keyrings/githubcli-archive-keyring.gpg] https://cli.github.com/packages stable main" \
     > /etc/apt/sources.list.d/github-cli.list \
  && curl -fsSL https://deb.nodesource.com/setup_22.x | bash - \
  && apt-get update && apt-get install -y --no-install-recommends \
    gh \
    nodejs \
  && rm -rf /var/lib/apt/lists/*

RUN useradd -m -u 1000 -s /bin/bash agent \
  && mkdir -p /workspace \
  && chown -R agent:agent /workspace

USER agent
WORKDIR /workspace

ENV CARGO_HOME="/home/agent/.cargo"
ENV RUSTUP_HOME="/home/agent/.rustup"
ENV PATH="/home/agent/.cargo/bin:${PATH}"

COPY --chown=agent:agent rust-toolchain.toml /workspace/rust-toolchain.toml
RUN curl --proto '=https' --tlsv1.2 -fsSL https://sh.rustup.rs \
     | sh -s -- -y --default-toolchain none --profile minimal \
  && rustup show \
  && rustup target add wasm32-unknown-unknown

COPY --chown=agent:agent . /workspace

RUN cargo build --release --locked
