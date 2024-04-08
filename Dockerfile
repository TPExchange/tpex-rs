FROM rust:alpine
WORKDIR /opt/tpex
RUN apk add clang
COPY . .
RUN cargo build --all
ENTRYPOINT ["cargo", "run", "--bin"]
