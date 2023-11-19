FROM rustlang/rust:nightly-alpine3.14 as builder
WORKDIR /usr/src/app
# Copying config/build files.
COPY src src
COPY Cargo.toml .
COPY Cargo.lock .
RUN apk add musl-dev \
 # Running rust-target install for the static-binary target (musl). \
 && rustup target install x86_64-unknown-linux-musl \
 # Installing static binary, using locked dependcies (no auto-update for anything). \
 && cargo install --locked --target=x86_64-unknown-linux-musl --path .


FROM alpine:3.14 as main
RUN apk add --no-cache git curl
WORKDIR /usr/src/app
# Copying compiled executable from the 'builder'.
COPY --from=builder /usr/local/cargo/bin/klocc .
# Copying rocket config file into final instance (startup/runtime config).
COPY Rocket.toml .
# Running binary.
ENTRYPOINT ["./klocc"]

# Additional layer for the healthcheck inside the container. This allows us to
# display a container status in the 'docker ps' (or any other docker monitor).
HEALTHCHECK --interval=1m --timeout=3s \
  CMD curl -sf 0.0.0.0:8080/api/health || exit 1
