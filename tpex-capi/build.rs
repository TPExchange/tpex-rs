extern crate cbindgen;
// Example custom build script.
fn main() {
    let config = cbindgen::Config {
        language: cbindgen::Language::C,
        header: Some("typedef struct tpex_state tpex_state;".to_string()),
        export: cbindgen::ExportConfig {
            rename: [("State".to_string(), "tpex_state".to_string())].into_iter().collect(),
            ..Default::default()
        },
        ..Default::default()
    };
    cbindgen::generate_with_config(".", config).unwrap().write_to_file("tpex-capi.h");
}
