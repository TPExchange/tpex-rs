#![cfg(test)]

use std::{collections::{BTreeMap, HashMap}, fmt::Display};

use crate::{order::OrderType, shared_account::Proposal};

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
        ("100C", Coins::from_millicoins(100_000), "100c"),
        ("100.000c", Coins::from_millicoins(100_000), "100c"),
        ("3.140", Coins::from_millicoins(3_140), "3.14c"),
        ("3.14c", Coins::from_millicoins(3_140), "3.14c"),
        ("1000c", Coins::from_millicoins(1_000_000), "1,000c"),
        ("1,321c", Coins::from_millicoins(1_321_000), "1,321c"),
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
        assert_eq!(parse_res, Some(coins), "{parse_me:?}.parse() gave {parse_res:?} instead of expected Some({coins:?})");
        assert_eq!(Coins::from_millicoins(coins.millicoins()), coins, "To and from millicoins gave different answers");
    }
    for (input, err) in bad {
        assert_eq!(input.parse::<Coins>(), Err(err), "{input:?}.parse() should fail with Err(err), but didn't")
    }
}

#[tokio::test]
async fn coin_xchg_ratio() {
    assert_eq!(Coins::from_coins(42), Coins::from_millicoins(42_000));
    assert_eq!(Coins::from_coins(37), Coins::from_millicoins(37_000));
    assert_eq!(Coins::from_coins(0), Coins::from_millicoins(0));
    assert_eq!(Coins::from_coins(u32::MAX), Coins::from_millicoins(u32::MAX as u64 * 1000));
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
fn player(n: u64) -> PlayerId { PlayerId::assume_username_correct(n.to_string()) }

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
            for PendingOrder { id, coins_per, player, amount_remaining, asset, order_type, fee_ppm } in self.0.values() {
                let t = match order_type {
                    OrderType::Buy => 'B',
                    OrderType::Sell => 'S',
                };
                writeln!(f, "{id} {player} {t}: {amount_remaining} @ {coins_per} of {asset} ({fee_ppm} ppm)")?;
            }
            Ok(())
        }
    }
    OrderInfo(state.get_orders())
}

#[test]
fn check_fee_calc() {
    assert_eq!(Coins::from_millicoins(1).fee_ppm(1), Ok(Coins::from_millicoins(1)));
    assert_eq!(Coins::from_coins(1).fee_ppm(20_000), Ok(Coins::from_millicoins(20)));
    assert_eq!(Coins::from_coins(1).fee_ppm(20_001), Ok(Coins::from_millicoins(21)));
    assert_eq!(Coins::from_millicoins(237).fee_ppm(20_000), Ok(Coins::from_millicoins(5)));
    assert_eq!(Coins::from_millicoins(4182).fee_ppm(31_000), Ok(Coins::from_millicoins(130)));
}

#[derive(Default)]
struct ExpectedState {
    sell_profit: Vec<(PlayerId, Coins)>,
    buy_cost: Vec<(PlayerId, Coins)>,
    coins_appeared: Vec<(PlayerId, Coins)>,
    coins_disappeared: Vec<(PlayerId, Coins)>,
    diamonds_sold: Vec<(PlayerId, u64)>,
    diamonds_bought: Vec<(PlayerId, u64)>,
    assets: Vec<(PlayerId, String, u64)>,
    unfulfilled: Vec<(PlayerId, Coins)>,
    fulfilled: Vec<(PlayerId, Coins)>,
    should_fail: bool
}
struct MatchStateWrapper<Players, Sink: tokio::io::AsyncWrite + std::marker::Unpin> where for<'a> &'a Players: IntoIterator<Item=&'a PlayerId> {
    state: State,
    sink: Sink,
    players: Players
}
impl<Players, Sink: tokio::io::AsyncWrite + std::marker::Unpin> MatchStateWrapper<Players, Sink> where for<'a> &'a Players: IntoIterator<Item=&'a PlayerId> {
    async fn assert_state(&mut self, action: Action, expected: ExpectedState) -> Option<u64> {
        let before_bals = self.state.get_bals();
        // let before_assets: std::collections::HashMap<_, _, std::hash::RandomState> = ((&self.players).into_iter().map(|i| (i.clone(), self.state.get_assets(&i)))).collect();
        let ret =
            if !expected.should_fail {
                Some(self.state.apply(action, &mut self.sink).await.expect("Failed action"))
            }
            else {
                self.state.apply(action, &mut self.sink).await.expect_err("Action should have failed, but succeeded");
                None
            };
        self.state.hard_audit();
        let mut expected_bals = before_bals.clone();
        for (p, gain) in expected.coins_appeared {
            expected_bals.entry(p).or_default().checked_add_assign(gain).expect("Coins appeared overflowed");
        }
        for (p, loss) in expected.coins_disappeared {
            expected_bals.entry(p).or_default().checked_sub_assign(loss).expect("Coins disappeared underflowed");
        }
        for (p, gain) in expected.sell_profit {
            let buy_fee = gain.fee_ppm(self.state.rates.buy_order_ppm).unwrap();
            let expected_amount = gain.checked_sub(buy_fee).expect("Failed to subtract buy fee");
            expected_bals.entry(p).or_default().checked_add_assign(expected_amount).expect("Failed to add net gain");
            expected_bals.entry(PlayerId::the_bank()).or_default().checked_add_assign(buy_fee).expect("Failed to add fee");
        }
        for (p, loss) in expected.buy_cost {
            let sell_fee = loss.fee_ppm(self.state.rates.sell_order_ppm).unwrap();
            let expected_amount = loss.checked_add(sell_fee).expect("Failed to add sell fee");
            expected_bals.entry(p).or_default().checked_sub_assign(expected_amount).expect("Failed to take net loss");
            expected_bals.entry(PlayerId::the_bank()).or_default().checked_add_assign(sell_fee).expect("Failed to add fee");
        }
        for (_p, gain) in expected.unfulfilled {
            let buy_fee = gain.fee_ppm(self.state.rates.buy_order_ppm).unwrap();
            expected_bals.entry(PlayerId::the_bank()).or_default().checked_sub_assign(buy_fee).expect("Failed to sub unfulfilled fee");
        }
        for (_p, gain) in expected.fulfilled {
            let buy_fee = gain.fee_ppm(self.state.rates.buy_order_ppm).unwrap();
            expected_bals.entry(PlayerId::the_bank()).or_default().checked_add_assign(buy_fee).expect("Failed to add fulfilled fee");
        }
        for (p, gain) in expected.diamonds_sold {
            let total = DIAMOND_RAW_COINS.checked_mul(gain).unwrap();
            let fee = total.fee_ppm(self.state.rates.coins_buy_ppm).unwrap();
            expected_bals.entry(PlayerId::the_bank()).or_default().checked_add_assign(fee).expect("Failed to add fx fee");
            expected_bals.entry(p).or_default().checked_add_assign(total.checked_sub(fee).unwrap()).expect("Failed to add expected balance");
        }
        for (p, gain) in expected.diamonds_bought {
            let total = DIAMOND_RAW_COINS.checked_mul(gain).unwrap();
            let fee = total.fee_ppm(self.state.rates.coins_sell_ppm).unwrap();
            expected_bals.entry(PlayerId::the_bank()).or_default().checked_add_assign(fee).expect("Failed to add fx fee");
            expected_bals.entry(p).or_default().checked_sub_assign(total.checked_add(fee).unwrap()).expect("Failed to add expected balance");
        }
        // expected_bals.entry(PlayerId::the_bank()).or_default().checked_add_assign(self.total_fee).expect("Failed to add total fee");
        let mut after_bals = self.state.get_bals();
        expected_bals.retain(|_, &mut j| !j.is_zero());
        after_bals.retain(|_, j| !j.is_zero());
        assert_eq!(after_bals, expected_bals, "Started at {before_bals:?} (after != expected)");
        let result_assets: std::collections::HashMap<_, _, std::hash::RandomState> =
            (&self.players).into_iter()
            .map(|i| (i.clone(), self.state.get_assets(i)))
            .filter(|(_, j)| !j.is_empty())
            .collect();
        let mut expected_assets: HashMap<PlayerId, HashMap<String, u64>> = HashMap::new();
        for (p, item, count) in expected.assets {
            if count == 0 {
                continue;
            }
            *expected_assets.entry(p).or_default().entry(item).or_default() += count;
        }
        assert_eq!(result_assets, expected_assets);
        ret
    }
}


#[tokio::test]
async fn lifecycle() {
    println!("WE ASSUME ASSOCIATIVITY OF FEES HERE. IF THE FEE IS TOO FINE, OR THE COST TOO LOW, THEN IT ALL DIES");
    // This should hopefully be enough
    let mut state = MatchStateWrapper {
        state: State::new(),
        sink: Vec::new(),
        players: [player(1), player(2), player(3), PlayerId::the_bank()]
    };

    let item = "cobblestone".to_owned();

    println!("Deposit 1");
    state.assert_state(
        Action::Deposit {
            player: player(1),
            asset: item.clone(),
            count: 64,
            banker: PlayerId::the_bank()
        },
        ExpectedState {
            assets: vec![(player(1), item.clone(), 64)],
            ..Default::default()
        }
    ).await;
    println!("Deposit 2");
    state.assert_state(
        Action::Deposit {
            player: player(2),
            asset: item.clone(),
            count: 128,
            banker: PlayerId::the_bank()
        },
        ExpectedState {
            assets: vec![(player(1), item.clone(), 64), (player(2), item.clone(), 128)],
            ..Default::default()
        }
    ).await;
    println!("Deposit 3");
    state.assert_state(
        Action::Deposit {
            player: player(3),
            asset: DIAMOND_NAME.to_owned(),
            count: 64,
            banker: PlayerId::the_bank()
        },
        ExpectedState {
            assets: vec![(player(1), item.clone(), 64), (player(2), item.clone(), 128), (player(3), DIAMOND_NAME.to_owned(), 64)],
            ..Default::default()
        }
    ).await;
    println!("Buy coins");
    state.assert_state(
        Action::BuyCoins {
            player: player(3),
            n_diamonds: 64
        },
        ExpectedState {
            assets: vec![(player(1), item.clone(), 64), (player(2), item.clone(), 128)],
            diamonds_sold: vec![(player(3), 64)],
            ..Default::default()
        }
    ).await;
    assert_eq!(state.state.hard_audit(), Audit{coins: Coins::from_coins(64000), assets: [(item.clone(), 192)].into_iter().collect()});
    println!("Purposefully failing buy order");
    state.assert_state(
        Action::BuyOrder {
            player: player(1),
            asset: item.clone(),
            count: 64,
            coins_per: Coins::from_millicoins(1000)
        },
        ExpectedState {
            assets: vec![(player(1), item.clone(), 64), (player(2), item.clone(), 128)],
            should_fail: true,
            ..Default::default()
        }
    ).await;
    println!("Sell order 1");
    state.assert_state(
        Action::SellOrder {
            player: player(1),
            asset: item.clone(),
            count: 32,
            coins_per: Coins::from_coins(1)
        },
        ExpectedState {
            assets: vec![(player(1), item.clone(), 32), (player(2), item.clone(), 128)],
            ..Default::default()
        }
    ).await;
    println!("Sell order 2");
    state.assert_state(
        Action::SellOrder {
            player: player(1),
            asset: item.clone(),
            count: 16,
            coins_per: Coins::from_coins(3)
        },
        ExpectedState {
            assets: vec![(player(1), item.clone(), 16), (player(2), item.clone(), 128)],
            ..Default::default()
        }
    ).await;
    println!("Sell order 3");
    state.assert_state(
        Action::SellOrder {
            player: player(2),
            asset: item.clone(),
            count: 16,
            coins_per: Coins::from_coins(2)
        },
        ExpectedState {
            assets: vec![(player(1), item.clone(), 16), (player(2), item.clone(), 112)],
            ..Default::default()
        }
    ).await;
    println!("Sell order 4");
    state.assert_state(
        Action::SellOrder {
            player: player(2),
            asset: item.clone(),
            count: 16,
            coins_per: Coins::from_coins(2)
        },
        ExpectedState {
            assets: vec![(player(1), item.clone(), 16), (player(2), item.clone(), 96)],
            ..Default::default()
        }
    ).await;
    println!("Sell order 5");
    let cancel_me = state.assert_state(
        Action::SellOrder {
            player: player(2),
            asset: item.clone(),
            count: 16,
            coins_per: Coins::from_coins(1)
        },
        ExpectedState {
            assets: vec![(player(1), item.clone(), 16), (player(2), item.clone(), 80)],
            ..Default::default()
        }
    ).await.unwrap();
    println!("Cancel sell order 5");
    state.assert_state(
        Action::CancelOrder {
            target: cancel_me
        },
        ExpectedState {
            assets: vec![(player(1), item.clone(), 16), (player(2), item.clone(), 96)],
            ..Default::default()
        }
    ).await.unwrap();
    println!("Sell order 6");
    state.assert_state(
        Action::SellOrder {
            player: player(2),
            asset: item.clone(),
            count: 16,
            coins_per: Coins::from_coins(10)
        },
        ExpectedState {
            assets: vec![(player(1), item.clone(), 16), (player(2), item.clone(), 80)],
            ..Default::default()
        }
    ).await;
    println!("Sell order 7");
    state.assert_state(
        Action::SellOrder {
            player: player(2),
            asset: item.clone(),
            count: 16,
            coins_per: Coins::from_coins(1)
        },
        ExpectedState {
            assets: vec![(player(1), item.clone(), 16), (player(2), item.clone(), 64)],
            ..Default::default()
        }
    ).await;
    println!("Initial orders:\n{}", pretty_orders(&state.state));
    assert_eq!(state.state.get_prices(&item), (
        BTreeMap::from_iter([

        ]),
        BTreeMap::from_iter([
            (Coins::from_coins(1), 48),
            (Coins::from_coins(2), 32),
            (Coins::from_coins(3), 16),
            (Coins::from_coins(10), 16),
        ]),
    ));

    println!("Buy order 1");
    state.assert_state(
        Action::BuyOrder {
            player: player(3),
            asset: item.clone(),
            count: 40,
            coins_per: Coins::from_coins(4)
        },
        ExpectedState {
            assets: vec![(player(1), item.clone(), 16), (player(2), item.clone(), 64), (player(3), item.clone(), 40)],
            sell_profit: vec![(player(1), Coins::from_coins(32)), (player(2), Coins::from_coins(8))],
            buy_cost: vec![(player(3), Coins::from_coins(40))],
            ..Default::default()
        }
    ).await;
    println!("Post buy 1:\n{}", pretty_orders(&state.state));
    assert_eq!(state.state.get_prices(&item), (
        BTreeMap::from_iter([

        ]),
        BTreeMap::from_iter([
            (Coins::from_coins(1), 8),
            (Coins::from_coins(2), 32),
            (Coins::from_coins(3), 16),
            (Coins::from_coins(10), 16),
        ]),
    ));
    println!("Buy order 2");
    state.assert_state(
        Action::BuyOrder {
            player: player(3),
            asset: item.clone(),
            count: 80,
            coins_per: Coins::from_coins(4)
        },
        ExpectedState {
            assets: vec![(player(1), item.clone(), 16), (player(2), item.clone(), 64), (player(3), item.clone(), 96)],
            sell_profit: vec![(player(1), Coins::from_coins(48)), (player(2), Coins::from_coins(72))],
            buy_cost: vec![(player(3), Coins::from_coins(216))],
            unfulfilled: vec![(player(3), Coins::from_coins(24 * 4))],
            ..Default::default()
        }
    ).await;
    assert_eq!(state.state.get_prices(&item), (
        BTreeMap::from_iter([
            (Coins::from_coins(4), 24),
        ]),
        BTreeMap::from_iter([
            (Coins::from_coins(10), 16),
        ]),
    ));
    println!("Post buy 2:\n{}", pretty_orders(&state.state));

    println!("Buy order 3:");
    let cancel_me = state.assert_state(
        Action::BuyOrder {
            player: player(3),
            asset: item.clone(),
            count: 1,
            coins_per: Coins::from_millicoins(1)
        },
        ExpectedState {
            assets: vec![(player(1), item.clone(), 16), (player(2), item.clone(), 64), (player(3), item.clone(), 96)],
            buy_cost: vec![(player(3), Coins::from_millicoins(1))],
            unfulfilled: vec![(player(3), Coins::from_millicoins(1))],
            ..Default::default()
        }
    ).await.unwrap();
    println!("Post buy 3:\n{}", pretty_orders(&state.state));

    println!("Cancel buy order 3:");
    state.assert_state(
        Action::CancelOrder {
            target: cancel_me
        },
        ExpectedState {
            assets: vec![(player(1), item.clone(), 16), (player(2), item.clone(), 64), (player(3), item.clone(), 96)],
            coins_appeared: vec![(player(3), Coins::from_millicoins(1).fee_ppm(state.state.rates.buy_order_ppm + 1_000_000).unwrap())],
            ..Default::default()
        }
    ).await;
    println!("Post cancel 3:\n{}", pretty_orders(&state.state));

    println!("Sell order 8:");
    state.assert_state(
        Action::SellOrder {
            player: player(2),
            asset: item.clone(),
            count: 24,
            coins_per: Coins::from_coins(4)
        },
        ExpectedState {
            assets: vec![(player(1), item.clone(), 16), (player(2), item.clone(), 40), (player(3), item.clone(), 120)],
            sell_profit: vec![(player(2), Coins::from_coins(96))],
            fulfilled: vec![(player(3), Coins::from_coins(24 * 4))],
            ..Default::default()
        }
    ).await;
    println!("Post sell 8:\n{}", pretty_orders(&state.state));
    assert_eq!(state.state.get_prices(&item), (
        BTreeMap::from_iter([
        ]),
        BTreeMap::from_iter([
            (Coins::from_coins(10), 16),
        ]),
    ));
    assert_eq!(state.state.hard_audit(), Audit{coins: Coins::from_coins(64000), assets: [(item.clone(), 192)].into_iter().collect()});

    println!("Sell coins");
    state.assert_state(
        Action::SellCoins {
            player: player(3),
            n_diamonds: 32,
        },
        ExpectedState {
            assets: vec![(player(1), item.clone(), 16), (player(2), item.clone(), 40), (player(3), item.clone(), 120), (player(3), DIAMOND_NAME.to_owned(), 32)],
            diamonds_bought: vec![(player(3), 32)],
            ..Default::default()
        }
    ).await;

    // Test over-withdrawing
    println!("Purposefully oversell coins");
    state.assert_state(
        Action::RequestWithdrawal {
            player: player(3),
            assets: [(item.clone(), 192)].into()
        },
        ExpectedState {
            assets: vec![(player(1), item.clone(), 16), (player(2), item.clone(), 40), (player(3), item.clone(), 120), (player(3), DIAMOND_NAME.to_owned(), 32)],
            should_fail: true,
            ..Default::default()
        }
    ).await;

    println!("Withdraw the {item}");
    let target = state.assert_state(
        Action::RequestWithdrawal {
            player: player(3),
            assets: [(item.clone(), 120)].into()
        },
        ExpectedState {
            assets: vec![(player(1), item.clone(), 16), (player(2), item.clone(), 40), (player(3), item.clone(), 0), (player(3), DIAMOND_NAME.to_owned(), 32)],
            ..Default::default()
        }
    ).await.unwrap();

    println!("Completing withdrawal");
    state.assert_state(
        Action::CompleteWithdrawal {
            target,
            banker: PlayerId::the_bank()
        },
        ExpectedState {
            assets: vec![(player(1), item.clone(), 16), (player(2), item.clone(), 40), (player(3), item.clone(), 0), (player(3), DIAMOND_NAME.to_owned(), 32)],
            ..Default::default()
        }
    ).await;



    let sync: StateSync = (&state.state).into();
    let sync_json = serde_json::to_string(&sync).expect("Failed to JSONise StateSync");
    println!("State: {:?}", state.state);
    println!("Serialised state: {sync:?}");
    println!("Serialised JSON: {sync_json:?}");
    let sync_dejson: StateSync = serde_json::from_str(&sync_json).expect("Failed to de-JSONise StateSync");
    let deser: State = sync_dejson.try_into().expect("Could not deserialise state sync");
    let deser_ser: StateSync = (&deser).into();
    assert_eq!(sync, deser_ser, "FastSync mismatch");
}

#[tokio::test]
async fn authorisations() {
    let authed = AssetId::from("cobblestone");
    let unauthed = AssetId::from("wither_skeleton_skull");
    let mut state = MatchStateWrapper {
        state: State::new(),
        sink: WriteSink::default(),
        players: [player(1), player(2), player(3), PlayerId::the_bank()]
    };
    println!("Depositing authorised item");
    state.assert_state(
        Action::Deposit {
            player: player(1),
            asset: authed.clone(),
            count: 1,
            banker: PlayerId::the_bank()
        },
        ExpectedState {
            assets: vec![(player(1), authed.clone(), 1)],
            ..Default::default()
        }
    ).await;
    println!("Depositing to-be unauthorised item");
    state.assert_state(
        Action::Deposit {
            player: player(1),
            asset: unauthed.clone(),
            count: 100,
            banker: PlayerId::the_bank()
        },
        ExpectedState {
            assets: vec![(player(1), authed.clone(), 1), (player(1), unauthed.clone(), 100)],
            ..Default::default()
        }
    ).await;
    println!("Restricting item");
    state.assert_state(
        Action::UpdateRestricted {
            restricted_assets: [unauthed.clone()].into_iter().collect(),
        },
        ExpectedState {
            assets: vec![(player(1), authed.clone(), 1), (player(1), unauthed.clone(), 100)],
            ..Default::default()
        }
    ).await;
    println!("Withdrawing restricted asset after already having deposited it");
    state.assert_state(
        Action::RequestWithdrawal {
            player: player(1),
            assets: [(unauthed.clone(), 1)].into()
        },
        ExpectedState {
            assets: vec![(player(1), authed.clone(), 1), (player(1), unauthed.clone(), 99)],
            ..Default::default()
        }
    ).await;
    println!("Sending restricted asset to unauthorised player");
    state.assert_state(
        Action::TransferAsset {
            payer: player(1),
            payee: player(2),
            asset: unauthed.clone(),
            count: 2
        },
        ExpectedState {
            assets: vec![(player(1), authed.clone(), 1), (player(1), unauthed.clone(), 97), (player(2), unauthed.clone(), 2)],
            ..Default::default()
        }
    ).await;
    println!("Attempting to withdraw restricted asset from unauthorised player");
    state.assert_state(
        Action::RequestWithdrawal {
            player: player(2),
            assets: [(unauthed.clone(), 2)].into()
        },
        ExpectedState {
            assets: vec![(player(1), authed.clone(), 1), (player(1), unauthed.clone(), 97), (player(2), unauthed.clone(), 2)],
            should_fail: true,
            ..Default::default()
        }
    ).await;
    println!("Manually authorising player withdrawing restricted item");
    state.assert_state(
        Action::AuthoriseRestricted {
            authorisee: player(2),
            asset: unauthed.clone(),
            new_count: 1
        },
        ExpectedState {
            assets: vec![(player(1), authed.clone(), 1), (player(1), unauthed.clone(), 97), (player(2), unauthed.clone(), 2)],
            ..Default::default()
        }
    ).await;
    println!("Attempting to overwithdraw restricted asset from newly authorised player");
    state.assert_state(
        Action::RequestWithdrawal {
            player: player(2),
            assets: [(unauthed.clone(), 2)].into()
        },
        ExpectedState {
            assets: vec![(player(1), authed.clone(), 1), (player(1), unauthed.clone(), 97), (player(2), unauthed.clone(), 2)],
            should_fail: true,
            ..Default::default()
        }
    ).await;
    println!("Withdrawing restricted asset from newly authorised player");
    state.assert_state(
        Action::RequestWithdrawal {
            player: player(2),
            assets: [(unauthed.clone(), 1)].into()
        },
        ExpectedState {
            assets: vec![(player(1), authed.clone(), 1), (player(1), unauthed.clone(), 97), (player(2), unauthed.clone(), 1)],
            ..Default::default()
        }
    ).await;
    println!("Again overwithdrawing restricted asset from newly authorised player");
    state.assert_state(
        Action::RequestWithdrawal {
            player: player(2),
            assets: [(unauthed.clone(), 1)].into()
        },
        ExpectedState {
            assets: vec![(player(1), authed.clone(), 1), (player(1), unauthed.clone(), 97), (player(2), unauthed.clone(), 1)],
            should_fail: true,
            ..Default::default()
        }
    ).await;
    println!("Unrestricting asset again");
    state.assert_state(
        Action::UpdateRestricted {
            restricted_assets: Default::default(),
        },
        ExpectedState {
            assets: vec![(player(1), authed.clone(), 1), (player(1), unauthed.clone(), 97), (player(2), unauthed.clone(), 1)],
            ..Default::default()
        }
    ).await;
    println!("Withdrawing final part");
    state.assert_state(
        Action::RequestWithdrawal {
            player: player(2),
            assets: [(unauthed.clone(), 1)].into()
        },
        ExpectedState {
            assets: vec![(player(1), authed.clone(), 1), (player(1), unauthed.clone(), 97)],
            ..Default::default()
        }
    ).await;
}

#[tokio::test]
async fn update_bankers() {
    let mut state = MatchStateWrapper {
        state: State::new(),
        sink: WriteSink::default(),
        players: [player(1), player(2), player(3), PlayerId::the_bank()]
    };
    assert!(!state.state.is_banker(&player(1)));
    println!("Replacing default banker");
    state.assert_state(
        Action::CreateOrUpdateShared {
            name: SharedId::the_bank(),
            owners: vec![player(1), player(3)],
            min_difference: 1,
            min_votes: 1,
        },
        ExpectedState {
            ..Default::default()
        }
    ).await;
    assert_eq!(state.state.get_bankers(), &[player(1), player(3)].into());
    println!("Trying to update bankers as non-banker");
    state.assert_state(
        Action::Propose {
            action: Box::new(Action::CreateOrUpdateShared {
                name: SharedId::the_bank(),
                owners: vec![player(1), player(3)],
                min_difference: 1,
                min_votes: 1,
            }),
            target: SharedId::the_bank(),
            proposer: player(2)
        },
        ExpectedState {
            should_fail: true,
            ..Default::default()
        }
    ).await;
    println!("Trying to update bankers as a banker");
    state.assert_state(
        Action::Propose {
            action: Box::new(Action::CreateOrUpdateShared {
                name: SharedId::the_bank(),
                owners: vec![player(2), player(3)],
                min_difference: 1,
                min_votes: 1,
            }),
            target: SharedId::the_bank(),
            proposer: player(1)
        },
        ExpectedState {
            ..Default::default()
        }
    ).await;
}

#[tokio::test]
async fn transfer_asset() {
    let mut state = MatchStateWrapper {
        state: State::new(),
        sink: WriteSink::default(),
        players: [player(1), player(2), player(3), PlayerId::the_bank()]
    };
    let item = "cobblestone".to_owned();
    state.assert_state(
        Action::Deposit {
            player: player(1),
            asset: item.clone(),
            count: 64,
            banker: PlayerId::the_bank()
        },
        ExpectedState {
            assets: vec![(player(1), item.clone(), 64)],
            ..Default::default()
        }
    ).await;
    state.assert_state(
        Action::Deposit {
            player: player(2),
            asset: DIAMOND_NAME.to_owned(),
            count: 2,
            banker: PlayerId::the_bank()
        },
        ExpectedState {
            assets: vec![(player(1), item.clone(), 64), (player(2), DIAMOND_NAME.to_owned(), 2)],
            ..Default::default()
        }
    ).await;
    state.assert_state(
        Action::BuyCoins {
            player: player(2),
            n_diamonds: 2
        },
        ExpectedState {
            assets: vec![(player(1), item.clone(), 64)],
            diamonds_sold: vec![(player(2), 2)],
            ..Default::default()
        }
    ).await;
    state.assert_state(
        Action::TransferAsset {
            payer: player(1),
            payee: player(2),
            asset: item.clone(),
            count: 4,
        },
        ExpectedState {
            assets: vec![(player(1), item.clone(), 60), (player(2), item.clone(), 4)],
            ..Default::default()
        }
    ).await;
    state.assert_state(
        Action::TransferCoins {
            payer: player(2),
            payee: player(1),
            count: Coins::from_coins(37),
        },
        ExpectedState {
            assets: vec![(player(1), item.clone(), 60), (player(2), item.clone(), 4)],
            coins_appeared: vec![(player(1), Coins::from_coins(37))],
            coins_disappeared: vec![(player(2), Coins::from_coins(37))],
            ..Default::default()
        }
    ).await;
    state.assert_state(
        Action::TransferCoins {
            payer: player(2),
            payee: player(1),
            count: Coins::from_coins(963),
        },
        ExpectedState {
            assets: vec![(player(1), item.clone(), 60), (player(2), item.clone(), 4)],
            coins_appeared: vec![(player(1), Coins::from_coins(963))],
            coins_disappeared: vec![(player(2), Coins::from_coins(963))],
            ..Default::default()
        }
    ).await;
}

// After nasty bug that caused reloads to not have newlines
#[tokio::test]
async fn reload_state() {
    let mut state = State::new();
    let mut log = Vec::new();
    for i in [
        Action::Deposit { player: player(1), asset: "cobblestone".into(), count: 1, banker: PlayerId::the_bank() },
        Action::Deposit { player: player(1), asset: "cobblestone".into(), count: 2, banker: PlayerId::the_bank() },
        Action::Deposit { player: player(1), asset: "cobblestone".into(), count: 3, banker: PlayerId::the_bank() },
    ] { state.apply(i, &mut log).await.expect("Failed to apply action"); }

    let mut loaded_state = State::new();
    loaded_state.replay(&mut log.as_ref(), true).await.expect("Failed to replay saved state");
    assert_eq!(StateSync::from(&loaded_state), StateSync::from(&state));
}

#[test]
fn fuzz_shared_id() {
    assert!(SharedId::the_bank().take_name().is_none());
    assert_eq!(SharedId::the_bank().parts().collect::<Vec<&str>>(), Vec::<&str>::new(), "Bank had parts");

    SharedId::try_from(PlayerId::assume_username_correct("foo".to_owned())).expect_err("Invalid SharedId got through");
    SharedId::try_from(PlayerId::assume_username_correct("foo/".to_owned())).expect_err("Invalid SharedId got through");
    SharedId::try_from(PlayerId::assume_username_correct("/foo/".to_owned())).expect_err("Invalid SharedId got through");

    let single = SharedId::try_from(PlayerId::assume_username_correct("/foo".to_owned())).expect("Could not parse valid SharedId");
    let (parent, name) = single.take_name().expect("Single name didn't have a name");
    assert_eq!(parent.collect::<Vec<&str>>(), Vec::<&str>::new(), "Somehow had parent in single name");
    assert_eq!(name, "foo");
    assert_eq!(single.parent(), Some(SharedId::the_bank()));
    let multi = SharedId::try_from(PlayerId::assume_username_correct("/foo/bar".to_owned())).expect("Could not parse valid SharedId");
    let (parent, name) = multi.take_name().expect("Multi name didn't have a name");
    assert_eq!(parent.collect::<Vec<_>>(), vec!["foo"]);
    assert_eq!(name, "bar");

}

#[tokio::test]
async fn test_shared() {
    let shared_name: SharedId = "/foo".parse().expect("Could not parse name");
    let mut state = MatchStateWrapper {
        state: State::new(),
        sink: WriteSink::default(),
        players: [player(1), player(2), player(3), shared_name.clone().into(), PlayerId::the_bank()]
    };
    // Try with invalid consensus
    state.assert_state(
        Action::CreateOrUpdateShared {
            name: shared_name.clone(),
            owners: vec![player(1)],
            min_difference: 1,
            min_votes: 2
        },
        ExpectedState {
            should_fail: true,
            ..Default::default()
        }
    ).await;
    let shared_name2: SharedId = "/foo2".parse().expect("Could not parse name");
    // Try with invalid consensus
    state.assert_state(
        Action::CreateOrUpdateShared {
            name: shared_name.clone(),
            owners: vec![player(2)],
            min_difference: 1,
            min_votes: 2,
        },
        ExpectedState {
            should_fail: true,
            ..Default::default()
        }
    ).await;
    state.assert_state(
        Action::CreateOrUpdateShared {
            name: shared_name.clone(),
            owners: vec![player(1)],
            min_difference: 2,
            min_votes: 2
        },
        ExpectedState {
            should_fail: true,
            ..Default::default()
        }
    ).await;
    // Try it properly
    state.assert_state(
        Action::CreateOrUpdateShared {
            name: shared_name.clone(),
            owners: vec![player(1)],
            min_difference: 1,
            min_votes: 1
        },
        ExpectedState {
            ..Default::default()
        }
    ).await;
    // Try to give it invalid consensus after founding
    state.assert_state(
        Action::CreateOrUpdateShared {
            name: shared_name.clone(),
            owners: vec![player(1)],
            min_difference: 2,
            min_votes: 2
        },
        ExpectedState {
            should_fail: true,
            ..Default::default()
        }
    ).await;
    state.assert_state(
        Action::Deposit {
            player: player(1),
            asset: "diamond".into(),
            count: 16,
            banker: PlayerId::the_bank()
        },
        ExpectedState {
            assets: vec![(player(1), "diamond".into(), 16)],
            ..Default::default()
        }
    ).await;
    state.assert_state(
        Action::BuyCoins {
            player: player(1),
            n_diamonds: 16
        },
        ExpectedState {
            diamonds_sold: vec![(player(1), 16)],
            ..Default::default()
        }
    ).await;
    state.assert_state(
        Action::TransferCoins {
            payer: player(1),
            payee: shared_name.clone().into(),
            count: Coins::from_coins(2000),
        },
        ExpectedState {
            coins_appeared: vec![(shared_name.clone().into(), Coins::from_coins(2000))],
            coins_disappeared: vec![(player(1), Coins::from_coins(2000))],
            ..Default::default()
        }
    ).await;
    state.assert_state(
        Action::TransferCoins {
            payer: player(1),
            payee: shared_name2.clone().into(),
            count: Coins::from_coins(2000),
        },
        ExpectedState {
            coins_appeared: vec![(shared_name2.clone().into(), Coins::from_coins(2000))],
            coins_disappeared: vec![(player(1), Coins::from_coins(2000))],
            ..Default::default()
        }
    ).await;
    // Try to steal coins
    state.assert_state(
        Action::Propose {
            action: Box::new(Action::TransferCoins {
                payer: shared_name2.clone().into(),
                payee: player(1),
                count: Coins::from_coins(1000)
            }),
            proposer: player(1),
            target: shared_name.clone(),
        },
        ExpectedState {
            should_fail: true,
            ..Default::default()
        }
    ).await;
    state.assert_state(
        Action::Propose {
            action: Box::new(Action::TransferCoins {
                payer: shared_name2.clone().into(),
                payee: player(1),
                count: Coins::from_coins(1000)
            }),
            proposer: player(1),
            target: shared_name2.clone(),
        },
        ExpectedState {
            should_fail: true,
            ..Default::default()
        }
    ).await;
    // Propose (and immediately pass) transferring coins to player 3
    state.assert_state(
        Action::Propose {
            action: Box::new(Action::TransferCoins {
                payer: shared_name.clone().into(),
                payee: player(3),
                count: Coins::from_coins(10)
            }),
            proposer: player(1),
            target: shared_name.clone(),
        },
        ExpectedState {
            coins_appeared: vec![(player(3), Coins::from_coins(10))],
            coins_disappeared: vec![(shared_name.clone().into(), Coins::from_coins(10))],
            ..Default::default()
        }
    ).await;
    // Update the shared account with another player, but the same thesholds
    state.assert_state(
        Action::CreateOrUpdateShared {
            name: shared_name.clone(),
            owners: vec![player(1), player(2)],
            min_difference: 1,
            min_votes: 1
        },
        ExpectedState {
            ..Default::default()
        }
    ).await;
    // Propose (and immediately pass) transferring coins to player 3 again
    state.assert_state(
        Action::Propose {
            action: Box::new(Action::TransferCoins {
                payer: shared_name.clone().into(),
                payee: player(3),
                count: Coins::from_coins(10)
            }),
            proposer: player(1),
            target: shared_name.clone(),
        },
        ExpectedState {
            coins_appeared: vec![(player(3), Coins::from_coins(10))],
            coins_disappeared: vec![(shared_name.clone().into(), Coins::from_coins(10))],
            ..Default::default()
        }
    ).await;
    // Update the shared account to require both players to vote, but only need at least 50% to agree
    state.assert_state(
        Action::CreateOrUpdateShared {
            name: shared_name.clone(),
            owners: vec![player(1), player(2)],
            min_difference: 0,
            min_votes: 2
        },
        ExpectedState {
            ..Default::default()
        }
    ).await;
    // Propose transferring coins to player 3 yet again
    let proposal1 = state.assert_state(
        Action::Propose {
            action: Box::new(Action::TransferCoins {
                payer: shared_name.clone().into(),
                payee: player(3),
                count: Coins::from_coins(10)
            }),
            proposer: player(1),
            target: shared_name.clone(),
        },
        ExpectedState {
            ..Default::default()
        }
    ).await.unwrap();
    // Player 2 doesn't like this, but their vote hits the notice threshold
    state.assert_state(
        Action::Disagree {
            player: player(2),
            proposal_id: proposal1
        },
        ExpectedState {
            coins_appeared: vec![(player(3), Coins::from_coins(10))],
            coins_disappeared: vec![(shared_name.clone().into(), Coins::from_coins(10))],
            ..Default::default()
        }
    ).await;
    // Player 2 is done with this
    let proposal2 = state.assert_state(
        Action::Propose {
            action: Box::new(Action::CreateOrUpdateShared {
                name: shared_name.clone(),
                owners: vec![player(1), player(2)],
                min_difference: 2,
                min_votes: 2
            }),
            proposer: player(2),
            target: shared_name.clone(),
        },
        ExpectedState {
            ..Default::default()
        }
    ).await.unwrap();
    // Player 1 thinks it's working fine
    state.assert_state(
        Action::Disagree {
            player: player(1),
            proposal_id: proposal2
        },
        ExpectedState {
            ..Default::default()
        }
    ).await;
    assert_eq!(StateSync::from(&state.state).shared_account.proposals.into_iter().collect::<Vec<(u64, Proposal)>>(), Vec::<(u64, Proposal)>::new());
    // Player 1 tries to vote again
    state.assert_state(
        Action::Disagree {
            player: player(1),
            proposal_id: proposal2
        },
        ExpectedState {
            should_fail: true,
            ..Default::default()
        }
    ).await;
    // Player 1 wants it back the way it was
    let proposal3 = state.assert_state(
        Action::Propose {
            action: Box::new(Action::CreateOrUpdateShared {
                name: shared_name.clone(),
                owners: vec![player(1), player(2)],
                min_difference: 1,
                min_votes: 2
            }),
            proposer: player(1),
            target: shared_name.clone(),
        },
        ExpectedState {
            ..Default::default()
        }
    ).await.unwrap();
    // Player 2 disagrees
    state.assert_state(
        Action::Disagree {
            player: player(2),
            proposal_id: proposal3
        },
        ExpectedState {
            ..Default::default()
        }
    ).await;
    assert_eq!(StateSync::from(&state.state).shared_account.proposals[&proposal3].agree, [player(1)].into_iter().collect());
    assert_eq!(StateSync::from(&state.state).shared_account.proposals[&proposal3].disagree, [player(2)].into_iter().collect());
    // They both agree to make a new subcompany for player 3, but mess up by making it a direct child of /
    state.assert_state(
        Action::Propose {
            proposer: player(1),
            action: Box::new(Action::CreateOrUpdateShared {
                name: "/bar".parse().unwrap(),
                owners: vec![player(3)],
                min_difference: 1,
                min_votes: 1
            }),
            target: "/foo".parse().unwrap()
        },
        ExpectedState {
            should_fail: true,
            ..Default::default()
        }
    ).await;
    // They do it properly this time
    let child_name: SharedId = "/foo/bar".parse().unwrap();
    let proposal4 = state.assert_state(
        Action::Propose {
            proposer: player(1),
            action: Box::new(Action::CreateOrUpdateShared {
                name: child_name.clone(),
                owners: vec![player(3)],
                min_difference: 1,
                min_votes: 1
            }),
            target: "/foo".parse().unwrap()
        },
        ExpectedState {
            ..Default::default()
        }
    ).await.unwrap();
    state.assert_state(
        Action::Agree {
            player: player(2),
            proposal_id: proposal4
        },
        ExpectedState {
            ..Default::default()
        }
    ).await;
    let proposal5 = state.assert_state(
        Action::Propose {
            action: Box::new(Action::TransferCoins {
                payer: shared_name.clone().into(),
                payee: child_name.clone().into(),
                count: Coins::from_coins(2)
            }),
            proposer: player(2),
            target: shared_name.clone()
        },
        ExpectedState {
            ..Default::default()
        }
    ).await.unwrap();
    state.assert_state(
        Action::Agree {
            player: player(1),
            proposal_id: proposal5
        },
        ExpectedState {
            coins_appeared: vec![(child_name.clone().into(), Coins::from_coins(2))],
            coins_disappeared: vec![(shared_name.clone().into(), Coins::from_coins(2))],
            ..Default::default()
        }
    ).await;
    // They decide they want those coins back
    let proposal6 = state.assert_state(
        Action::Propose {
            action: Box::new(Action::TransferCoins {
                payer: child_name.clone().into(),
                payee: shared_name.clone().into(),
                count: Coins::from_coins(1)
            }),
            proposer: player(2),
            target: shared_name.clone()
        },
        ExpectedState {
            ..Default::default()
        }
    ).await.unwrap();
    state.assert_state(
        Action::Agree {
            player: player(1),
            proposal_id: proposal6
        },
        ExpectedState {
            coins_appeared: vec![(shared_name.clone().into(), Coins::from_coins(1))],
            coins_disappeared: vec![(child_name.clone().into(), Coins::from_coins(1))],
            ..Default::default()
        }
    ).await;
    let fs = StateSync::from(&state.state);
    assert_eq!(fs.shared_account.bank.children().keys().cloned().collect::<Vec<String>>(), vec!["foo".to_owned()]);
    // The bank winds up the company
    state.assert_state(
        Action::Propose {
            action: Box::new(Action::WindUp {
                account: shared_name.clone()
            }),
            proposer: PlayerId::the_bank(),
            target: SharedId::the_bank()
        },
        ExpectedState {
            coins_appeared: vec![(PlayerId::the_bank(), Coins::from_coins(1970))],
            coins_disappeared: vec![(shared_name.clone().into(), Coins::from_coins(1969)), (child_name.clone().into(), Coins::from_coins(1))],
            ..Default::default()
        }
    ).await;
}
#[test]
fn fuzz_etp() {
    let shared_name: SharedId = "/foo".parse().expect("Could not parse name");
    assert!(ETPId::try_new(shared_name.clone(), "foobar".to_owned()).is_ok());
    assert!(ETPId::try_new(shared_name.clone(), "%foobar".to_owned()).is_err());
    assert!(ETPId::try_new(shared_name.clone(), "f%oobar".to_owned()).is_err());
}

#[tokio::test]
async fn issue_etp() {
    let shared_name: SharedId = "/foo".parse().expect("Could not parse name");
    let mut state = MatchStateWrapper {
        state: State::new(),
        sink: WriteSink::default(),
        players: [player(1), player(2), player(3), shared_name.clone().into(), PlayerId::the_bank()]
    };
    // Create the issuer
    state.assert_state(
        Action::CreateOrUpdateShared {
            name: shared_name.clone(),
            owners: vec![player(1)],
            min_difference: 1,
            min_votes: 1
        },
        ExpectedState {
            ..Default::default()
        }
    ).await;
    let etp = ETPId::try_new(shared_name.clone(), "foobar".to_owned()).expect("Valid ETP name still threw error");
    // Ensure that ETPs need authorisation
    state.assert_state(
        Action::Issue {
            product: etp.clone(),
            count: 64
        },
        ExpectedState {
            should_fail: true,
            ..Default::default()
        }
    ).await;
    // Give them ETP status
    state.assert_state(
        Action::UpdateETPAuthorised  {
            accounts: [shared_name.clone()].into_iter().collect()
        },
        ExpectedState {
            ..Default::default()
        }
    ).await;
    // Issue the etps
    state.assert_state(
        Action::Issue {
            product: etp.clone(),
            count: 64
        },
        ExpectedState {
            assets: vec![(shared_name.clone().into(), (&etp).into(), 64)],
            ..Default::default()
        }
    ).await;
    // Issue some more etps
    state.assert_state(
        Action::Issue {
            product: etp.clone(),
            count: 32
        },
        ExpectedState {
            assets: vec![(shared_name.clone().into(), (&etp).into(), 96)],
            ..Default::default()
        }
    ).await;
    // Overremove etps
    state.assert_state(
        Action::Remove {
            product: etp.clone(),
            count: 100
        },
        ExpectedState {
            assets: vec![(shared_name.clone().into(), (&etp).into(), 96)],
            should_fail: true,
            ..Default::default()
        }
    ).await;
    // Remove some etps
    state.assert_state(
        Action::Remove {
            product: etp.clone(),
            count: 16
        },
        ExpectedState {
            assets: vec![(shared_name.clone().into(), (&etp).into(), 80)],
            ..Default::default()
        }
    ).await;
    // Remove remaining etps
    state.assert_state(
        Action::Remove {
            product: etp.clone(),
            count: 80
        },
        ExpectedState {
            ..Default::default()
        }
    ).await;
}
