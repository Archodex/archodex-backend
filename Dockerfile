# syntax=docker/dockerfile:1

# Pick the right host-built artifact
FROM --platform=$BUILDPLATFORM alpine AS pick
ARG BUILDPLATFORM
ARG TARGETARCH
WORKDIR /work

# Copy in the already-built binary from the build context
# See .dockerignore for how we filter to just the built binaries
COPY . .

# Map Docker arch -> Rust triple and stage the correct binary at /server
RUN set -eux; \
  case "${TARGETARCH}" in \
  amd64)  T=x86_64 ;; \
  arm64)  T=aarch64 ;; \
  *)      echo "unsupported TARGETARCH=${TARGETARCH}"; exit 1 ;; \
  esac; \
  install -m 0755 "target/${T}-unknown-linux-gnu/release/server" /server

FROM cgr.dev/chainguard/glibc-dynamic:latest
EXPOSE 5732
COPY --from=pick /server /

ENTRYPOINT ["/server"]