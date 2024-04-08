use tpex::Auditable;

#[tokio::test]
async fn init_state() {
    let state = tpex::State::new();
    assert_eq!(state.hard_audit(), tpex::Audit{coins: 0, assets: Default::default()});
}
