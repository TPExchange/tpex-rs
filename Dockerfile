FROM rust:alpine AS cargo-build
WORKDIR /opt/tpex
RUN apk add clang
COPY . .
RUN RUSTFLAGS="-C target-feature=-crt-static" cargo build --release
RUN RUSTFLAGS="-C target-feature=-crt-static" cargo test --release

# ---

FROM alpine
RUN apk add libgcc
USER 1000
COPY --from=cargo-build /opt/tpex/target/release/tpex-srv /usr/local/bin/
