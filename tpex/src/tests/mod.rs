
use std::{collections::BTreeMap, fmt::Display};

use super::*;

#[derive(Default)]
struct WriteSink {}

impl tokio::io::AsyncWrite for WriteSink {
    fn poll_write(
        self: std::pin::Pin<&mut Self>,
        _cx: &mut std::task::Context<'_>,
        buf: &[u8],
    ) -> std::task::Poll<std::result::Result<usize, std::io::Error>> {
        std::task::Poll::Ready(Ok(buf.len()))
    }

    fn poll_flush(self: std::pin::Pin<&mut Self>, _cx: &mut std::task::Context<'_>) -> std::task::Poll<std::result::Result<(), std::io::Error>> {
        std::task::Poll::Ready(Ok(()))
    }

    fn poll_shutdown(self: std::pin::Pin<&mut Self>, _cx: &mut std::task::Context<'_>) -> std::task::Poll<std::result::Result<(), std::io::Error>> {
        std::task::Poll::Ready(Ok(()))
    }
}

#[tokio::test]
async fn coin_fuzz() {
    let good = [
        ("100c", Coins::from_millicoins(100_000), "100c"),
        ("100", Coins::from_millicoins(100_000), "100c"),
        ("100c", Coins::from_millicoins(100_000), "100c"),
        ("100.000c", Coins::from_millicoins(100_000), "100c"),
        ("3.140", Coins::from_millicoins(3_140), "3.14c"),
        ("3.14c", Coins::from_millicoins(3_140), "3.14c"),
    ];
    let bad = [
        ("100.0001c", Error::CoinStringTooPrecise),
        ("100c.", Error::CoinStringMangled),
        ("A hundred c", Error::CoinStringMangled),
    ];
    // Test good
    for (parse_me, coins, canonical) in good {
        let to_string_res = coins.to_string();
        assert_eq!(coins.to_string(), canonical, "{coins:?}.to_string() gave {to_string_res:?} instead of expected {canonical:?}");
        let parse_res = parse_me.parse().ok();
        assert_eq!(parse_res, Some(coins), "{parse_me:?}.parse() gave {parse_res:?} instead of expected Some({coins:?})")
    }
    for (input, err) in bad {
        assert_eq!(input.parse::<Coins>(), Err(err), "{input:?}.parse() should fail with Err(err), but didn't")
    }
}

#[tokio::test]
async fn coin_xchg_ratio() {
    assert_eq!(Coins::from_diamonds(42), Ok(Coins::from_millicoins(42_000_000)));
    assert_eq!(Coins::from_coins(42), Coins::from_millicoins(42_000));
}

#[tokio::test]
async fn coin_brute() {
    for millis in 0..100_000 {
        let coins = Coins::from_millicoins(millis);
        assert_eq!(coins.to_string().parse(), Ok(coins))
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
async fn deposit_undeposit() {
    let mut state = State::new();
    let mut sink = WriteSink::default();

    let item = "cobblestone".to_owned();

    state.apply(Action::Deposit {
        player: player(1),
        asset: item.clone(),
        count: 16384,
        banker: PlayerId::the_bank()
    }, &mut sink).await.expect("Deposit failed");
    assert_eq!(state.get_assets(&player(1)).get(&item).cloned(), Some(16384));
    assert_eq!(state.hard_audit(), Audit{coins: Coins::default(), assets: [(item.clone(), 16384)].into_iter().collect()});
    state.apply(Action::Undeposit {
        player: player(1),
        asset: item.clone(),
        count: 16384,
        banker: PlayerId::the_bank()
    }, &mut sink).await.expect("Undeposit failed");
    assert_eq!(state.hard_audit(), Audit::default());
}

#[tokio::test]
async fn invalid_deposit() {
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
async fn undeposit() {
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

fn pretty_orders(state: &State) -> impl Display {
    struct OrderInfo(BTreeMap<u64, PendingOrder>);
    impl std::fmt::Display for OrderInfo {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            for PendingOrder { id, coins_per, player, amount_remaining, asset, order_type } in self.0.values() {
                let t = match order_type {
                    OrderType::Buy => 'B',
                    OrderType::Sell => 'S',
                };
                writeln!(f, "{id} {player} {t}: {amount_remaining} @ {coins_per} of {asset}")?;
            }
            Ok(())
        }
    }
    OrderInfo(state.get_orders())
}

#[tokio::test]
async fn matching() {
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

    assert_eq!(state.hard_audit(), Audit{coins: Coins::from_coins(64000), assets: [(item.clone(), 192)].into_iter().collect()});

    state.apply(Action::BuyOrder {
        player: player(1),
        asset: item.clone(),
        count: 64,
        coins_per: Coins::from_millicoins(1000)
    }, &mut sink).await.expect_err("Bought with insufficient coins");
    state.apply(Action::SellOrder {
        player: player(1),
        asset: item.clone(),
        count: 32,
        coins_per: Coins::from_coins(1)
    }, &mut sink).await.expect("Sell order 1 failed");
    state.apply(Action::SellOrder {
        player: player(1),
        asset: item.clone(),
        count: 16,
        coins_per: Coins::from_coins(3)
    }, &mut sink).await.expect("Sell order 2 failed");
    state.apply(Action::SellOrder {
        player: player(2),
        asset: item.clone(),
        count: 16,
        coins_per: Coins::from_coins(2)
    }, &mut sink).await.expect("Sell order 3 failed");
    state.apply(Action::SellOrder {
        player: player(2),
        asset: item.clone(),
        count: 16,
        coins_per: Coins::from_coins(2)
    }, &mut sink).await.expect("Sell order 4 failed");
    let cancel_me = state.apply(Action::SellOrder {
        player: player(2),
        asset: item.clone(),
        count: 16,
        coins_per: Coins::from_coins(1)
    }, &mut sink).await.expect("Sell order 5 failed");
    state.apply(Action::CancelOrder {
        target: cancel_me
    }, &mut sink).await.expect("Cancel sell order 5 failed");
    state.apply(Action::SellOrder {
        player: player(2),
        asset: item.clone(),
        count: 16,
        coins_per: Coins::from_coins(10)
    }, &mut sink).await.expect("Sell order 6 failed");
    state.apply(Action::SellOrder {
        player: player(2),
        asset: item.clone(),
        count: 16,
        coins_per: Coins::from_coins(1)
    }, &mut sink).await.expect("Sell order 7 failed");
    println!("Initial orders:\n{}", pretty_orders(&state));

    state.apply(Action::BuyOrder {
        player: player(3),
        asset: item.clone(),
        count: 40,
        coins_per: Coins::from_coins(4)
    }, &mut sink).await.expect("Buy order 1 failed");
    println!("Post buy 1:\n{}", pretty_orders(&state));

    for (p, bal) in [(player(1), 32), (player(2), 8), (player(3), 63960)] {
        assert_eq!(state.get_bal(&p), Coins::from_coins(bal));
    }
    assert_eq!(state.get_assets(&player(3)), [(item.clone(), 40)].into_iter().collect());

    state.apply(Action::BuyOrder {
        player: player(3),
        asset: item.clone(),
        count: 80,
        coins_per: Coins::from_coins(4)
    }, &mut sink).await.expect("Buy order 2 failed");
    println!("Post buy 2:\n{}", pretty_orders(&state));

    for (p, bal) in [(player(1), 80), (player(2), 80), (player(3), 63744)] {
        assert_eq!(state.get_bal(&p), Coins::from_coins(bal));
    }
    assert_eq!(state.get_assets(&player(3)), [(item.clone(), 96)].into_iter().collect());

    state.apply(Action::SellOrder {
        player: player(2),
        asset: item.clone(),
        count: 24,
        coins_per: Coins::from_coins(4)
    }, &mut sink).await.expect("Sell order 8 failed");
    println!("Post sell 8:\n{}", pretty_orders(&state));

    for (p, bal) in [(player(1), 80), (player(2), 176), (player(3), 63744)] {
        assert_eq!(state.get_bal(&p), Coins::from_coins(bal));
    }
    assert_eq!(state.get_assets(&player(3)), [(item.clone(), 120)].into_iter().collect());

}
