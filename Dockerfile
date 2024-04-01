FROM rust:alpine
WORKDIR /opt/tpex
RUN apk add clang
COPY . .
RUN cargo build
ENTRYPOINT ["cargo", "run", "assets.json"]
