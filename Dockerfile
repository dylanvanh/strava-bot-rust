FROM rust:alpine

RUN apk add --no-cache musl-dev pkgconfig openssl-dev

WORKDIR /app
COPY Cargo.toml Cargo.lock ./
COPY src ./src

# Alpine uses musl, so Rust defaults to static linking
# But static OpenSSL libs aren't easily available
# So we disable static linking and use dynamic instead
ENV RUSTFLAGS="-C target-feature=-crt-static"
RUN cargo build --release

FROM alpine:latest

RUN apk add --no-cache ca-certificates libc6-compat openssl libgcc
RUN adduser -D strava-bot

COPY --from=0 /app/target/release/strava-bot /usr/local/bin/strava-bot
RUN chown strava-bot:strava-bot /usr/local/bin/strava-bot

USER strava-bot
CMD ["strava-bot"]
