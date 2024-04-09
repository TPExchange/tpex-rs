FROM rust:alpine as cargo-build
WORKDIR /opt/tpex
RUN apk add clang
COPY . .
RUN cargo build --release

# ---

FROM alpine
USER 1000
COPY --from=cargo-build /opt/tpex/target/release/tpex-srv /opt/tpex/target/release/trans-fer /usr/local/bin/
