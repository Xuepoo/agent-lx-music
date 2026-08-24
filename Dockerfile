# Stage 1: Build
FROM docker.io/library/rust:1.98-bookworm AS builder
WORKDIR /app
COPY . .
RUN apt-get update && apt-get install -y pkg-config libasound2-dev libmpv-dev clang libclang-dev && cargo build --release --bin alx

# Stage 2: Runtime
FROM docker.io/library/debian:bookworm-slim
RUN apt-get update && apt-get install -y mpv alsa-utils pulseaudio-utils procps && rm -rf /var/lib/apt/lists/*

COPY --from=builder /app/target/release/alx /usr/local/bin/alx

# Create a non-root test user belonging to audio group
RUN useradd -m -G audio alxuser
USER alxuser
WORKDIR /home/alxuser

# Default XDG Paths
ENV ALX_HOME=/home/alxuser/.local/share/agent-lx-music
ENV XDG_CONFIG_HOME=/home/alxuser/.config
ENV XDG_CACHE_HOME=/home/alxuser/.cache

RUN mkdir -p /home/alxuser/.config/agent-lx-music /home/alxuser/.local/share/agent-lx-music /home/alxuser/.cache/agent-lx-music

ENTRYPOINT ["/usr/local/bin/alx"]
