use crate::TokenLevel;

#[test]
fn token_level_deser() {
    match serde_json::from_str::<TokenLevel>(r#"0"#) {
        Ok(x) => assert_eq!(x, TokenLevel::ReadOnly),
        Err(e) => panic!("{e}")
    }
}
