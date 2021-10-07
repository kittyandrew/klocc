FROM rustlang/rust:nightly-slim as builder
WORKDIR /usr/src/app

# Copying config/build files
COPY src src
COPY Cargo.toml .
COPY Cargo.lock .

RUN rustup target install x86_64-unknown-linux-musl \
 && cargo install --locked --target=x86_64-unknown-linux-musl --path .


FROM alpine:3.14 as main
RUN apk add --no-cache git

WORKDIR /usr/src/app
# Copying compiled executable from the 'builder'.
COPY --from=builder /usr/local/cargo/bin/klocc .
# Copying rocket config file into final instance.
COPY Rocket.toml .

ENTRYPOINT ["./klocc"]
