
use super::*;

#[derive(Default)]
struct WriteSink {}

impl tokio::io::AsyncWrite for WriteSink {
    fn poll_write(
        self: std::pin::Pin<&mut Self>,
        _cx: &mut std::task::Context<'_>,
        buf: &[u8],
    ) -> std::task::Poll<Result<usize, std::io::Error>> {
        std::task::Poll::Ready(Ok(buf.len()))
    }

    fn poll_flush(self: std::pin::Pin<&mut Self>, _cx: &mut std::task::Context<'_>) -> std::task::Poll<Result<(), std::io::Error>> {
        std::task::Poll::Ready(Ok(()))
    }

    fn poll_shutdown(self: std::pin::Pin<&mut Self>, _cx: &mut std::task::Context<'_>) -> std::task::Poll<Result<(), std::io::Error>> {
        std::task::Poll::Ready(Ok(()))
    }
}

#[tokio::test]
async fn init_state() {
    let state = State::new();
    assert_eq!(state.hard_audit(), Audit::default());
}

fn create_playerid(id: String) -> PlayerId {
    #[allow(deprecated)]
    PlayerId::evil_constructor(id)
}
fn player(n: u64) -> PlayerId { create_playerid(n.to_string()) }

#[tokio::test]
async fn test_autoconvert_deposit() {
    let mut state = State::new();
    let mut sink = WriteSink::default();

    let from = "diamond_block".to_owned();
    let to = "diamond".to_owned();

    state.apply(Action::UpdateAutoConvert {
        conversions: vec![AutoConversion{from:from.clone(),to:to.clone(),n_to:9}],
        banker: PlayerId::the_bank()
    }, &mut sink).await.expect("Autoconvert apply failed");
    state.apply(Action::Deposit {
        player: player(1),
        asset: from.clone(),
        count: 49,
        banker: PlayerId::the_bank()
    }, &mut sink).await.expect("Autoconvert deposit failed");
    assert_eq!(state.get_assets(&player(1)).get(&from).cloned(), None);
    assert_eq!(state.get_assets(&player(1)).get(&to).cloned(), Some(441));
    assert_eq!(state.hard_audit(), Audit{coins: 0, assets: [(to, 441)].into_iter().collect()});
}

#[tokio::test]
async fn test_invalid_deposit() {
    let mut state = State::new();
    let mut sink = WriteSink::default();

    state.apply(Action::Deposit {
        player: player(1),
        asset: "costelbone".to_owned(),
        count: 49,
        banker: PlayerId::the_bank()
    }, &mut sink).await.expect_err("Costlebone deposited");
}

#[tokio::test]
async fn test_undeposit() {
    let mut state = State::new();
    let mut sink = WriteSink::default();

    let item = "cobblestone".to_owned();

    state.apply(Action::Deposit {
        player: player(1),
        asset: item.clone(),
        count: 49,
        banker: PlayerId::the_bank()
    }, &mut sink).await.expect("Deposit failed");
    assert_eq!(state.get_assets(&player(1)).get(&item).cloned(), Some(49));
    state.apply(Action::Undeposit {
        player: player(1),
        asset: item.clone(),
        count: 48,
        banker: PlayerId::the_bank()
    }, &mut sink).await.expect("First undeposit failed");
    assert_eq!(state.get_assets(&player(1)).get(&item).cloned(), Some(1));
    state.apply(Action::Undeposit {
        player: player(1),
        asset: item.clone(),
        count: 1,
        banker: PlayerId::the_bank()
    }, &mut sink).await.expect("Second undeposit failed");
    assert_eq!(state.get_assets(&player(1)).get(&item).cloned(), None);
    assert_eq!(state.hard_audit(), Audit::default());
}

#[tokio::test]
async fn test_matching() {
    let mut state = State::new();
    let mut sink = WriteSink::default();

    let item = "cobblestone".to_owned();

    state.apply(Action::Deposit {
        player: player(1),
        asset: item.clone(),
        count: 64,
        banker: PlayerId::the_bank()
    }, &mut sink).await.expect("Deposit 1 failed");
    state.apply(Action::Deposit {
        player: player(2),
        asset: item.clone(),
        count: 128,
        banker: PlayerId::the_bank()
    }, &mut sink).await.expect("Deposit 2 failed");
    state.apply(Action::Deposit {
        player: player(3),
        asset: DIAMOND_NAME.to_owned(),
        count: 64,
        banker: PlayerId::the_bank()
    }, &mut sink).await.expect("Deposit 3 failed");
    state.apply(Action::BuyCoins {
        player: player(3),
        n_diamonds: 64
    }, &mut sink).await.expect("Buy coins failed");

    assert_eq!(state.hard_audit(), Audit{coins: 64000, assets: [(item.clone(), 192)].into_iter().collect()});

    state.apply(Action::BuyOrder {
        player: player(1),
        asset: item.clone(),
        count: 64,
        coins_per: 1
    }, &mut sink).await.expect_err("Bought with insufficient coins");
    state.apply(Action::SellOrder {
        player: player(1),
        asset: item.clone(),
        count: 32,
        coins_per: 1
    }, &mut sink).await.expect("Sell order 1 failed");
    state.apply(Action::SellOrder {
        player: player(1),
        asset: item.clone(),
        count: 16,
        coins_per: 3
    }, &mut sink).await.expect("Sell order 2 failed");
    state.apply(Action::SellOrder {
        player: player(2),
        asset: item.clone(),
        count: 16,
        coins_per: 2
    }, &mut sink).await.expect("Sell order 3 failed");
    state.apply(Action::SellOrder {
        player: player(2),
        asset: item.clone(),
        count: 16,
        coins_per: 2
    }, &mut sink).await.expect("Sell order 4 failed");
    let cancel_me = state.apply(Action::SellOrder {
        player: player(2),
        asset: item.clone(),
        count: 16,
        coins_per: 1
    }, &mut sink).await.expect("Sell order 5 failed");
    state.apply(Action::CancelOrder {
        target_id: cancel_me
    }, &mut sink).await.expect("Cancel sell order 5 failed");
    state.apply(Action::SellOrder {
        player: player(2),
        asset: item.clone(),
        count: 16,
        coins_per: 10
    }, &mut sink).await.expect("Sell order 6 failed");
    state.apply(Action::BuyOrder {
        player: player(3),
        asset: item.clone(),
        count: 40,
        coins_per: 4
    }, &mut sink).await.expect("Buy order 1 failed");

    for (p, bal) in [(player(1), 32), (player(2), 8), (player(3), 63960)] {
        assert_eq!(state.get_bal(&p), bal);
    }
    assert_eq!(state.get_assets(&player(3)), [(item.clone(), 40)].into_iter().collect());

    state.apply(Action::BuyOrder {
        player: player(3),
        asset: item.clone(),
        count: 80,
        coins_per: 4
    }, &mut sink).await.expect("Buy order 2 failed");
    for (p, bal) in [(player(1), 32), (player(2), 80), (player(3), 63728)] {
        assert_eq!(state.get_bal(&p), bal);
    }
    assert_eq!(state.get_assets(&player(3)), [(item.clone(), 80)].into_iter().collect());

    state.apply(Action::SellOrder {
        player: player(2),
        asset: item.clone(),
        count: 16,
        coins_per: 4
    }, &mut sink).await.expect("Sell order 7 failed");

}
