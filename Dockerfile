# syntax=docker/dockerfile:1

FROM rust:1.97.1-bookworm AS builder

WORKDIR /src

COPY Cargo.toml Cargo.lock rust-toolchain.toml ./
COPY adapters ./adapters
COPY apps ./apps
COPY core ./core
COPY extensions ./extensions
COPY schemas ./schemas
COPY tools ./tools

RUN cargo +1.97.1 build --locked --release -p graphhelm-cli

FROM debian:bookworm-slim AS runtime

# The base image owns the compatible Debian package snapshot; pinning a point version here would
# make routine base-image security updates fail when that exact package revision leaves the mirror.
# hadolint ignore=DL3008
RUN apt-get update \
    && apt-get install --yes --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/* \
    && groupadd --system --gid 10001 graphhelm \
    && useradd --system --uid 10001 --gid graphhelm \
        --home-dir /var/lib/graphhelm --shell /usr/sbin/nologin graphhelm \
    && install --directory --owner graphhelm --group graphhelm --mode 0700 /var/lib/graphhelm

COPY --from=builder /src/target/release/graphhelm /usr/local/bin/graphhelm

ENV GRAPHHELM_EVENTS_DIR=/var/lib/graphhelm/events \
    GRAPHHELM_BIND=127.0.0.1:8080

USER 10001:10001
VOLUME ["/var/lib/graphhelm"]

ENTRYPOINT ["/bin/sh", "-eu", "-c"]
CMD ["token=${GRAPHHELM_API_TOKEN:?GRAPHHELM_API_TOKEN must be set}; case \"$token\" in *[!0-9a-f]*|'') echo 'GRAPHHELM_API_TOKEN must be exactly 64 lowercase hexadecimal characters' >&2; exit 64;; esac; if [ \"${#token}\" -ne 64 ]; then echo 'GRAPHHELM_API_TOKEN must be exactly 64 lowercase hexadecimal characters' >&2; exit 64; fi; mkdir -p \"$GRAPHHELM_EVENTS_DIR\"; token_file=\"${GRAPHHELM_EVENTS_DIR}.token\"; if [ -L \"$token_file\" ]; then echo 'refusing a symbolic-link token file' >&2; exit 65; elif [ -e \"$token_file\" ]; then if [ ! -f \"$token_file\" ] || [ \"$(cat \"$token_file\")\" != \"$token\" ]; then echo 'the persisted token does not match GRAPHHELM_API_TOKEN' >&2; exit 65; fi; else umask 077; printf '%s' \"$token\" > \"$token_file\"; fi; chmod 0600 \"$token_file\"; unset GRAPHHELM_API_TOKEN token; exec /usr/local/bin/graphhelm serve --events \"$GRAPHHELM_EVENTS_DIR\" --bind \"$GRAPHHELM_BIND\""]
