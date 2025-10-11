use std::{collections::HashMap, pin::pin};

use clap::Parser;
use futures::StreamExt;
use num_traits::Euclid;
use tpex::{order::OrderType, AssetId, Auditable, Coins, PlayerId, State, WrappedAction, DIAMOND_NAME, DIAMOND_RAW_COINS};
use tpex_api::Token;

#[derive(clap::Subcommand)]
enum Command {
    /// Stream the state to stdout
    Mirror,
    /// Generate an audit of the current state
    Audit,
    /// Create an atomically updated cache file of the FastSync data
    FastsyncCache {
        path: String
    },
    /// Generate an itemised audit of income
    CashFlow {
        // Defaults to the owner of the provided token
        account: Option<String>,
        // Gets the cash flow for the entire economy
        #[arg(long)]
        all: bool
    }
}

#[derive(clap::Parser)]
struct Args {
    /// The remote TPEx api endpoint
    #[arg(long, env = "TPEX_URL")]
    endpoint: reqwest::Url,
    /// The token for that remote
    #[arg(long, env = "TPEX_TOKEN")]
    token: Token,
    #[command(subcommand)]
    command: Command
}

#[tokio::main]
async fn main() {
    let args = Args::parse();
    match args.command {
        Command::Audit => {
            let remote = tpex_api::Remote::new(args.endpoint.clone(), args.token);
            let fastsync = remote.fastsync().await.expect("Failed to download fastsync");
            let mut audit = State::try_from(fastsync).expect("Failed to load fastsync").hard_audit();
            // Convert coins to diamonds
            let (coins_from_diamonds, hopefully_no_remainder) = audit.coins.millicoins().div_rem_euclid(&DIAMOND_RAW_COINS.millicoins());
            assert_eq!(hopefully_no_remainder, 0, "Non-integer number of diamonds in bank");
            *audit.assets.entry(DIAMOND_NAME.into()).or_default() += coins_from_diamonds;

            let mut x = Vec::from_iter(audit.assets);
            x.sort_by(|(a,_), (b, _)| a.cmp(b));

            for (asset, count) in x {
                println!("{asset}: {count}");
            }
        }
        Command::Mirror => {
            let mut next_id = 1;
            let remote = tpex_api::Remote::new(args.endpoint.clone(), args.token);
            let state_stream = remote.stream_state(next_id).await.expect("Failed to stream state");
            let mut state_stream = pin!(state_stream);
            while let Some(next) = state_stream.next().await {
                let next = next.unwrap();
                assert_eq!(next.id, next_id, "Skipped id in state");
                serde_json::to_writer(&std::io::stdout(), &next).expect("Failed to reserialise wrapped action");
                println!();
                next_id += 1;
            }
        },
        Command::FastsyncCache { path } => {
            let tmp_path = format!("{path}.tmp");
            let remote = tpex_api::Remote::new(args.endpoint.clone(), args.token);
            let state_stream = remote.stream_fastsync().await.expect("Failed to stream fastsync");
            let mut state_stream = pin!(state_stream);
            while let Some(next) = state_stream.next().await {
                let next = next.unwrap();
                println!("Id: {}", next.current_id);
                // Atomic overwrite of file
                tokio::fs::write(&tmp_path, serde_json::to_string(&next).unwrap()).await.expect("Could not write cached data");
                std::fs::rename(&tmp_path, &path).expect("Failed to overwrite cached data");
            }
        },

        Command::CashFlow { account , all} => {
            let remote = tpex_api::Remote::new(args.endpoint.clone(), args.token);
            let account = match account {
                Some(x) => PlayerId::assume_username_correct(x),
                None => remote.get_token().await.expect("Failed to get token owner").user
            };

            let mut revenue: HashMap<AssetId, Coins> = HashMap::new();
            let mut losses: HashMap<AssetId, Coins> = HashMap::new();

            let mut old_state;
            let mut state = State::new();
            // let mut last_account;
            // let mut new_account = if all { state.itemised_audit().balance } else { state.audit_player(&account) };
            // let mut last_orders: Vec<tpex::order::PendingOrder>;
            // let mut new_orders = state.get_orders_filter(|i| all || i.player == account).collect();
            eprint!("Fetching state...");
            let actions = remote.get_state(0).await.expect("Failed to stream fastsync");
            eprintln!(" done");
            for next in actions.lines() {
                let wrapped_action: WrappedAction = serde_json::from_str(next).expect("Failed to read action");
                old_state = state.clone();
                state.apply_wrapped(wrapped_action.clone(), tokio::io::sink()).await.expect("Failed to apply action");
                // last_account = new_account;
                // new_account = if all { state.itemised_audit().balance } else { state.audit_player(&account) };
                // last_orders = new_orders;
                // new_orders = state.get_orders_filter(|i| all || i.player == account).collect();

                match wrapped_action.action {
                    tpex::Action::BuyCoins { player, n_diamonds: _ } => {
                        if all {
                            let this_revenue = state.itemised_audit().balance.coins.checked_sub(old_state.itemised_audit().balance.coins).unwrap();
                            revenue.entry(DIAMOND_NAME.into())
                                .or_default()
                                .checked_add_assign(this_revenue)
                                .unwrap();
                        }
                        else if account.is_bank() || player == account {
                            let bank_gain = state.get_bal(&PlayerId::the_bank()).checked_sub(old_state.get_bal(&PlayerId::the_bank())).unwrap();
                            (
                                if account.is_bank() {
                                    &mut revenue
                                }
                                else {
                                    &mut losses
                                }
                            )
                                .entry(DIAMOND_NAME.into())
                                .or_default()
                                .checked_add_assign(bank_gain)
                                .unwrap();
                        }
                    },
                    tpex::Action::SellCoins { player, n_diamonds: _ } => {
                        if !all && !account.is_bank() && player != account {
                            continue;
                        }
                        let bank_gain = state.get_bal(&PlayerId::the_bank()).checked_sub(old_state.get_bal(&PlayerId::the_bank())).unwrap();
                        (
                            if account.is_bank() {
                                &mut revenue
                            }
                            else {
                                &mut losses
                            }
                        )
                            .entry(DIAMOND_NAME.into())
                            .or_default()
                            .checked_add_assign(bank_gain)
                            .unwrap();
                    },
                    tpex::Action::BuyOrder {player, asset, count: _, coins_per: _ } => {
                        if all {
                            let player_loss = old_state.get_bal(&player).checked_sub(state.get_bal(&player)).unwrap();
                            let this_revenue =
                                state.itemised_audit().balance.coins
                                .checked_add(player_loss).unwrap()
                                .checked_sub(old_state.itemised_audit().balance.coins).unwrap();
                            revenue.entry(asset)
                                .or_default()
                                .checked_add_assign(this_revenue).unwrap();
                        }
                        // If this isn't by the account we're tracking, then it is potential revenue
                        else if player != account {
                            let this_revenue = state.get_bal(&account).checked_sub(old_state.get_bal(&account)).unwrap();
                            revenue.entry(asset)
                                .or_default()
                                .checked_add_assign(this_revenue).unwrap();
                        }
                        // We will mark all of these as losses, and then remove the cancelled orders and the extant orders at the end
                        else {
                            let this_loss = old_state.get_bal(&account).checked_sub(state.get_bal(&account)).unwrap();
                            losses.entry(asset)
                                .or_default()
                                .checked_add_assign(this_loss).unwrap();
                        }
                    },
                    tpex::Action::SellOrder { player, asset, count: _, coins_per: _ } => {
                        if all || player == account {
                            let this_revenue = state.get_bal(&player).checked_sub(old_state.get_bal(&player)).unwrap();
                            revenue.entry(asset)
                                .or_default()
                                .checked_add_assign(this_revenue).unwrap();
                        }
                        // If this isn't by the account we're tracking, then it is either irrelevant or already included in a tracker buy order
                        else {

                        }
                    },
                    tpex::Action::CancelOrder { target } => {
                        // If it's our buy order, we haven't lost this money after all...
                        if let Ok(order) = old_state.get_order(target) &&
                            order.order_type == OrderType::Buy &&
                            order.player == account &&
                            !all {
                                losses.get_mut(&order.asset).unwrap().checked_sub_assign(order.coins_per.checked_mul(order.amount_remaining).unwrap()).unwrap();
                            }
                    }
                    // Deposits and withdrawals are just moving the assets around, not revenue or losses
                    tpex::Action::Deposit { .. } |
                    tpex::Action::RequestWithdrawal { .. } |
                    tpex::Action::CancelWithdrawal { .. } |
                    // These are admin actions that do not concern us
                    tpex::Action::CompleteWithdrawal { .. } |
                    tpex::Action::Undeposit { .. } |
                    tpex::Action::UpdateRestricted { .. } |
                    tpex::Action::AuthoriseRestricted { .. } |
                    tpex::Action::UpdateBankRates { .. } |
                    tpex::Action::CreateOrUpdateShared { .. } |
                    tpex::Action::Deleted { .. } => (),
                    // Dunno what to do with these yet
                    tpex::Action::TransferCoins { .. } => (),
                    tpex::Action::TransferAsset { .. } => (),
                    tpex::Action::Propose { .. } => todo!(),
                    tpex::Action::Agree { .. } => todo!(),
                    tpex::Action::Disagree { .. } => todo!(),
                    tpex::Action::WindUp { .. } => todo!(),
                    tpex::Action::UpdateETPAuthorised { .. } => todo!(),
                    tpex::Action::Issue { .. } => todo!(),
                    tpex::Action::Remove { .. } => todo!(),
                }
            }
            // Clean up still cancelable orders
            for order in state.get_orders_filter(|i| i.player == account) {
                if !all && order.order_type == OrderType::Buy {
                    losses.get_mut(&order.asset).unwrap().checked_sub_assign(order.coins_per.checked_mul(order.amount_remaining).unwrap()).unwrap();
                }
            }
            if all {
                println!("Gross output: {}", revenue.values().copied().reduce(|a, b| a.checked_add(b).unwrap()).unwrap_or_default());
                let mut revenue = Vec::from_iter(revenue);
                revenue.sort_unstable_by(|(_, a), (_, b)| b.cmp(a));
                for (asset, coins) in revenue {
                    if !coins.is_zero() {
                        println!("\t{asset}: {coins}");
                    }
                }
            }
            else {
                println!("Revenue: {}", revenue.values().copied().reduce(|a, b| a.checked_add(b).unwrap()).unwrap_or_default());
                let mut revenue = Vec::from_iter(revenue);
                revenue.sort_unstable_by(|(_, a), (_, b)| b.cmp(a));
                for (asset, coins) in revenue {
                    if !coins.is_zero() {
                        println!("\t{asset}: {coins}");
                    }
                }
                println!("Losses: {}", losses.values().copied().reduce(|a, b| a.checked_add(b).unwrap()).unwrap_or_default());
                let mut losses = Vec::from_iter(losses);
                losses.sort_unstable_by(|(_, a), (_, b)| b.cmp(a));
                for (asset, coins) in losses {
                    if !coins.is_zero() {
                        println!("\t{asset}: {coins}");
                    }
                }
            }
        }
    }

}
