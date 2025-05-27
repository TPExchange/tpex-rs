FROM rust:alpine AS cargo-build
WORKDIR /opt/tpex
RUN apk add clang sqlite-dev
COPY . .
RUN LIBSQLITE3_SYS_USE_PKG_CONFIG=1 RUSTFLAGS="-C target-feature=-crt-static" cargo build --bin=tpex-srv --release --features=server

# ---

FROM alpine
RUN apk add libgcc sqlite-libs
USER 1000
COPY --from=cargo-build /opt/tpex/target/release/tpex-srv /usr/local/bin/
