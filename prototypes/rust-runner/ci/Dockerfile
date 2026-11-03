# Build the Linux binary first with ci/build-linux.sh (Rust 1.99.1 + Cargo.lock).
# Packaging accepts only the exact expected binary hash; no compiler in runtime.
FROM debian:bookworm-slim@sha256:99c0b25c46d0214d5a2091092e7908907dcaecd6a962cde0ac700acee222b6f3
ARG RUNNER_SHA256
COPY dist/reqon /usr/local/bin/reqon
COPY dist/ca-certificates.crt /etc/ssl/certs/ca-certificates.crt
RUN test -n "$RUNNER_SHA256" && printf '%s  /usr/local/bin/reqon\n' "$RUNNER_SHA256" | sha256sum -c - \
    && chmod 0755 /usr/local/bin/reqon \
    && test "$(reqon --version)" = 'reqon-runner-prototype 0.14.1' \
    && mkdir /work && chown 65532:65532 /work
LABEL org.opencontainers.image.title="Reqon Runner" org.opencontainers.image.version="0.14.1"
USER 65532:65532
WORKDIR /work
ENTRYPOINT ["/usr/local/bin/reqon"]
