# syntax=docker/dockerfile:1

# Pick the right host-built artifact
FROM --platform=$BUILDPLATFORM alpine AS pick
ARG BUILDPLATFORM
ARG TARGETARCH
WORKDIR /work

# Copy in the already-built target/ directory from the build context
COPY target ./target

# Map Docker arch -> Rust triple and stage the correct binary at /server
RUN set -eux; \
  case "${TARGETARCH}" in \
  amd64)  T=x86_64-unknown-linux-musl ;; \
  arm64)  T=aarch64-unknown-linux-musl ;; \
  *)      echo "unsupported TARGETARCH=${TARGETARCH}"; exit 1 ;; \
  esac; \
  install -m 0755 "target/${T}/release/server" /server

FROM cgr.dev/chainguard/static:latest
COPY --from=pick /server /

ENTRYPOINT ["/server"]