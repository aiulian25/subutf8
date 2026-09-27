# The SubUTF8 container: one static program, built here from source, and nothing else.
# The desktop packages are made from this same program, so every build converts identically.
ARG RUST_VERSION=1.98.1

FROM rust:${RUST_VERSION}-alpine3.24 AS build
WORKDIR /workspace
COPY . .
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    cargo build --release --locked -p subutf8-app \
    && packaging/collect-licenses.sh subutf8-app:x86_64-unknown-linux-musl >THIRD-PARTY-LICENSES.txt
# /data holds the settings and the history. Any PUID must be able to write to a new named
# volume there, which starts as a copy of this folder, so it is writable by all, like /tmp.
RUN install -d -m 1777 /rootfs/data

FROM scratch
ARG VERSION=dev
LABEL org.opencontainers.image.title="SubUTF8" \
      org.opencontainers.image.description="Convert subtitle text to UTF-8 safely" \
      org.opencontainers.image.source="https://github.com/aiulian25/subutf8" \
      org.opencontainers.image.licenses="MIT" \
      org.opencontainers.image.version="$VERSION"
COPY --from=build /workspace/target/release/subutf8 /subutf8
COPY --from=build /workspace/LICENSE /workspace/THIRD-PARTY-LICENSES.txt /licenses/
COPY --from=build /rootfs/ /
ENV SUBUTF8_MODE=container \
    SUBUTF8_DATA_DIR=/data
USER 65532:65532
EXPOSE 61880
ENTRYPOINT ["/subutf8"]
