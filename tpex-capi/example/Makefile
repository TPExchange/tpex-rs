tpex-capi-example: main.c ../tpex-capi.h ../../target/debug/libtpex_capi.so
	$(CC) main.c ../../target/debug/libtpex_capi.so -o $@

../tpex-capi.h: ../src/lib.rs ../build.rs ../Cargo.toml
	cargo build
