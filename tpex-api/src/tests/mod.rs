#![cfg(test)]

use std::{io::Write, str::FromStr, sync::Arc};

use futures::StreamExt;
use tokio_util::sync::DropGuard;
use tpex::{AssetId, PlayerId, WrappedAction};
use tracing_subscriber::EnvFilter;

use crate::{server::{self, tokens::TokenHandler}, Mirrored, Remote, Token, TokenLevel};

fn player(n: u64) -> PlayerId { PlayerId::assume_username_correct(n.to_string()) }

struct RunningServer {
    cancel: DropGuard,
    // We need this as a handle
    #[allow(dead_code)]
    pub database: tempfile::NamedTempFile,
    pub handle: tokio::task::JoinHandle<()>,
    pub url: reqwest::Url,
    pub token: Token,
}
impl RunningServer {
    async fn start_server_with_log(log: impl Into<Vec<u8>>) -> RunningServer {
        let _ = tracing_subscriber::fmt()
            .with_test_writer()
            .with_env_filter(EnvFilter::try_new("trace").unwrap())
            .try_init();

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("Failed to bind");
        let endpoint = listener.local_addr().expect("Could not get local address");
        let mut database = tempfile::NamedTempFile::new().expect("Could not make database tempfile");
        database.write_all(include_bytes!("test.db")).expect("Failed to write out db");
        database.flush().expect("Failed to flush db");
        let trade_log = tokio::io::BufStream::new(std::io::Cursor::new(log.into()));
        let cancel = tokio_util::sync::CancellationToken::new();
        let url = reqwest::Url::from_str(&format!("http://{endpoint}")).expect("Failed to parse static ep");
        let handle = tokio::spawn(server::run_server(
            cancel.clone(),
            trade_log,
            TokenHandler::new(database.path().to_str().expect("Got wacky tempfile name")).await.expect("Failed to create TokenHandler"),
            listener
        ));
        RunningServer {
            cancel: cancel.drop_guard(),
            database,
            handle,
            url,
            token: Token([0;16])
        }
    }
    fn start_server() -> impl Future<Output = RunningServer> {
        Self::start_server_with_log(Vec::new())
    }
    async fn stop(self) {
        drop(self.cancel);
        self.handle.await.expect("Failed to cancel server");
    }
}

#[test]
fn token_level_deser() {
    match serde_json::from_str::<TokenLevel>(r#"0"#) {
        Ok(x) => assert_eq!(x, TokenLevel::ReadOnly),
        Err(e) => panic!("{e}")
    }
}

#[tokio::test]
async fn launch_server() {
    let server = RunningServer::start_server().await;
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
    server.stop().await;
}

#[tokio::test]
async fn deposit() {
    let server = RunningServer::start_server().await;
    let client = Remote::new(server.url.clone(), server.token);
    let deposit_action = tpex::Action::Deposit {
        player: PlayerId::assume_username_correct("test".to_owned()),
        asset: AssetId::from("cobblestone"),
        count: 64,
        banker: PlayerId::the_bank()
    };
    client.apply(&deposit_action).await.expect("Failed to apply deposit");
    // This will be a single action, so we can treat it as a single wrapped action
    let new_state: WrappedAction = serde_json::from_slice(&client.get_state(0).await.expect("FastSync failed")).expect("Failed to deserialise state");
    assert_eq!(new_state.action, deposit_action);
    assert_eq!(new_state.id, 1);
}

#[tokio::test]
async fn fastsync() {
    let server = RunningServer::start_server().await;
    let client = Remote::new(server.url.clone(), server.token);
    client.fastsync().await.expect("FastSync failed");
}

#[tokio::test]
async fn stream_state() {
    let server = RunningServer::start_server().await;
    let client = Remote::new(server.url.clone(), server.token);
    let client_copy = Remote::new(server.url.clone(), server.token);
    let stream = client.stream_state(1).await.expect("Stream failed");
    let actions = vec![
        tpex::Action::Deposit {
            player: PlayerId::assume_username_correct("test".to_owned()),
            asset: AssetId::from("cobblestone"),
            count: 1,
            banker: PlayerId::the_bank()
        },
        tpex::Action::Deposit {
            player: PlayerId::assume_username_correct("test".to_owned()),
            asset: AssetId::from("cobblestone"),
            count: 2,
            banker: PlayerId::the_bank()
        },
        tpex::Action::Deposit {
            player: PlayerId::assume_username_correct("test".to_owned()),
            asset: AssetId::from("cobblestone"),
            count: 3,
            banker: PlayerId::the_bank()
        },
        tpex::Action::Deposit {
            player: PlayerId::assume_username_correct("test".to_owned()),
            asset: AssetId::from("cobblestone"),
            count: 4,
            banker: PlayerId::the_bank()
        },
    ];
    let actions_copy = actions.clone();
    tokio::spawn(async move {
        for i in actions_copy {
            client_copy.apply(&i).await.expect("Failed to apply action");
        }
    });
    let mut stream = std::pin::pin!(stream);
    let wrapped = stream.next().await.expect("Stream terminated early").expect("Failed to read from stream");
    assert_eq!(wrapped.action, actions[0]);
    let wrapped = stream.next().await.expect("Stream terminated early").expect("Failed to read from stream");
    assert_eq!(wrapped.action, actions[1]);
    let wrapped = stream.next().await.expect("Stream terminated early").expect("Failed to read from stream");
    assert_eq!(wrapped.action, actions[2]);
    let wrapped = stream.next().await.expect("Stream terminated early").expect("Failed to read from stream");
    assert_eq!(wrapped.action, actions[3]);
    let _full_state = client.get_state(0).await.expect("Could not get full state");
}

#[tokio::test]
async fn mirrored_stream_state() {
    let server = RunningServer::start_server().await;
    let client = Arc::new(Mirrored::new(server.url.clone(), server.token));
    let client_2 = Arc::new(Mirrored::new(server.url.clone(), server.token));
    let client_clone = client.clone();
    let stream = client.stream().await.expect("Stream failed");
    let actions = vec![
        tpex::Action::Deposit {
            player: PlayerId::assume_username_correct("test".to_owned()),
            asset: AssetId::from("cobblestone"),
            count: 1,
            banker: PlayerId::the_bank()
        },
        tpex::Action::Deposit {
            player: PlayerId::assume_username_correct("test".to_owned()),
            asset: AssetId::from("cobblestone"),
            count: 2,
            banker: PlayerId::the_bank()
        },
        tpex::Action::Deposit {
            player: PlayerId::assume_username_correct("test".to_owned()),
            asset: AssetId::from("cobblestone"),
            count: 3,
            banker: PlayerId::the_bank()
        },
        tpex::Action::Deposit {
            player: PlayerId::assume_username_correct("test".to_owned()),
            asset: AssetId::from("cobblestone"),
            count: 4,
            banker: PlayerId::the_bank()
        },
    ];
    let actions_copy = actions.clone();
    let client_2_clone = client_2.clone();
    tokio::spawn(async move {
        for i in actions_copy {
            client_2_clone.apply(i).await.expect("Failed to apply action");
        }
    });
    let mut stream = std::pin::pin!(stream);
    let wrapped = stream.next().await.expect("Stream terminated early").expect("Failed to read from stream");
    assert_eq!(wrapped.1.action, actions[0]);
    let wrapped = stream.next().await.expect("Stream terminated early").expect("Failed to read from stream");
    assert_eq!(wrapped.1.action, actions[1]);
    let wrapped = stream.next().await.expect("Stream terminated early").expect("Failed to read from stream");
    assert_eq!(wrapped.1.action, actions[2]);
    let wrapped = stream.next().await.expect("Stream terminated early").expect("Failed to read from stream");
    assert_eq!(wrapped.1.action, actions[3]);
    assert_eq!(tpex::StateSync::from(&*client_2.fastsync().await.unwrap()), tpex::StateSync::from(&*client_clone.fastsync().await.unwrap()))
}

#[tokio::test]
async fn stream_fastsync() {
    let server = RunningServer::start_server().await;
    let client = Remote::new(server.url.clone(), server.token);
    let client_copy = Remote::new(server.url.clone(), server.token);
    let stream = client.stream_fastsync().await.expect("FastSync failed");
    let actions = vec![
        tpex::Action::Deposit {
            player: PlayerId::assume_username_correct("test".to_owned()),
            asset: AssetId::from("cobblestone"),
            count: 1,
            banker: PlayerId::the_bank()
        },
        tpex::Action::Deposit {
            player: PlayerId::assume_username_correct("test".to_owned()),
            asset: AssetId::from("cobblestone"),
            count: 2,
            banker: PlayerId::the_bank()
        },
        tpex::Action::Deposit {
            player: PlayerId::assume_username_correct("test".to_owned()),
            asset: AssetId::from("cobblestone"),
            count: 3,
            banker: PlayerId::the_bank()
        },
    ];
    let actions_copy = actions.clone();
    tokio::spawn(async move {
        for i in actions_copy {
            client_copy.apply(&i).await.expect("Failed to apply action");
        }
    });
    let mut stream = std::pin::pin!(stream);
    let mut count = 0;
    let wrapped = loop {
        let res = stream.next().await.expect("Stream terminated early").expect("Failed to read from stream");
        if res.current_id == 3 { break res; }
        // Make sure that we're not being spammed
        count += 1;
        if count > 4 {
            panic!("Too many loops");
        }
    };
    assert_eq!(wrapped.balance.assets[&PlayerId::assume_username_correct("test".to_owned())][&AssetId::from("cobblestone")], 6);
}

// After nasty bug that caused reloads to not have newlines
#[tokio::test]
async fn reload_state() {
    let server1 = RunningServer::start_server().await;
    let client1 = Remote::new(server1.url, server1.token);
    for i in [
        tpex::Action::Deposit { player: player(1), asset: "cobblestone".into(), count: 1, banker: PlayerId::the_bank() },
        tpex::Action::Deposit { player: player(1), asset: "cobblestone".into(), count: 2, banker: PlayerId::the_bank() },
        tpex::Action::Deposit { player: player(1), asset: "cobblestone".into(), count: 3, banker: PlayerId::the_bank() },
    ] { client1.apply(&i).await.expect("Failed to apply action"); }
    let state1 = client1.get_state(0).await.expect("Could not get initial state");
    let server2 = RunningServer::start_server_with_log(state1.clone()).await;
    let client2 = Remote::new(server2.url, server2.token);
    let state2 = client2.get_state(0).await.expect("Could not get second state");
    assert_eq!(state1, state2);
}

// After nasty bug that caused reloads to not have newlines
#[tokio::test]
async fn test_inspect() {
    let server = RunningServer::start_server().await;
    let client = Remote::new(server.url, server.token);
    // Disable fees
    client.apply(&tpex::Action::UpdateBankRates { rates: tpex::BankRates::free() }).await.unwrap();
    // Deposit diamonds
    client.apply(&tpex::Action::Deposit { player: player(1), asset: tpex::DIAMOND_NAME.into(), count: 64, banker: PlayerId::the_bank() }).await.unwrap();
    // No autoconversion
    assert_eq!(client.get_balance(&player(1)).await.expect("Failed to get balance"), tpex::Coins::default());
    assert_eq!(client.get_assets(&player(1)).await.expect("Failed to get assets"), [(tpex::DIAMOND_NAME.into(), 64)].into());
    // Non-existent account should be empty
    assert_eq!(client.get_balance(&player(2)).await.expect("Failed to get balance"), tpex::Coins::default());
    assert_eq!(client.get_assets(&player(2)).await.expect("Failed to get assets"), Default::default());
    // Convert diamonds
    client.apply(&tpex::Action::BuyCoins { player: player(1), n_diamonds: 32 }).await.unwrap();
    // Should be split now
    assert_eq!(client.get_balance(&player(1)).await.expect("Failed to get balance"), tpex::Coins::from_coins(32_000));
    assert_eq!(client.get_assets(&player(1)).await.expect("Failed to get assets"), [(tpex::DIAMOND_NAME.into(), 32)].into());
    // Non-existent account should be empty
    assert_eq!(client.get_balance(&player(2)).await.expect("Failed to get balance"), tpex::Coins::default());
    assert_eq!(client.get_assets(&player(2)).await.expect("Failed to get assets"), Default::default());

    // Test audit
    assert_eq!(client.itemised_audit().await.expect("Failed to get itemised audit"), tpex::State::try_from(client.fastsync().await.expect("Failed to fastsync")).expect("Bad fastsync").itemised_audit())
}
