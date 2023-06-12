FROM rust as builder

WORKDIR /usr/src/krypto

COPY . .

RUN cargo install --path .

FROM debian:buster-slim

RUN apt-get update && apt-get install -y openssl libssl1.1 ca-certificates

COPY --from=builder /usr/local/cargo/bin/krypto /usr/local/bin/krypto

CMD ["krypto"]

