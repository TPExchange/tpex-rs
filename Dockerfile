FROM rust:alpine AS cargo-build
WORKDIR /opt/tpex
RUN apk add clang sqlite-dev openssl-dev
COPY . .
RUN LIBSQLITE3_SYS_USE_PKG_CONFIG=1 RUSTFLAGS="-C target-feature=-crt-static" cargo build --release
RUN mkdir /tmp/out
RUN find  /opt/tpex/target/release/ -maxdepth 1 -type f -exec test -x {} \; -exec cp {} /tmp/out \;

# ---

FROM alpine
RUN apk add libgcc sqlite-libs
USER 1000
COPY --from=cargo-build /tmp/out/* /usr/local/bin/
