use clap::Parser;
use poise::serenity_prelude as serenity;
use tokio::io::AsyncReadExt;
use std::io::Write;

mod commands;


#[derive(clap::Parser)]
struct Args {
    endpoint: String,
    db: String,
    assets: Option<std::path::PathBuf>,
}

#[tokio::main]
async fn main() {
    // Crash on inconsistency
    std::panic::set_hook(Box::new(move |info| {
        let _ = writeln!(std::io::stderr(), "{}", info);
        std::process::exit(1);
    }));

    let args = Args::parse();
    // The code here just starts the discord bot, as we respond to commands

    // Database setup
    // let mut db = self::db::DatabaseConnection::new(std::env::var("DATABASE_URL").expect("missing DATABASE_URL")).await.expect("Failed to init database");

    let remote_url = args.endpoint.parse().expect("Could not parse remote url");

    let remote_token: tpex_api::Token = std::env::var("TPEX_TOKEN").expect("Missing TPEX_TOKEN environment variable").parse().expect("Could not parse TPEX_TOKEN");

    let discord_token = std::env::var("DISCORD_TOKEN").expect("Missing DISCORD_TOKEN environment variable");


    // Discord setup
    let mut client = {
        let data = commands::Data{
            state: tpex_api::Mirrored::new(remote_url, remote_token),
            // db: commands::Database::new(&args.db).await
        };
        if let Some(asset_path) = args.assets {
            let mut assets = String::new();
            tokio::fs::File::open(asset_path).await.expect("Unable to open asset info").read_to_string(&mut assets).await.expect("Unable to read asset list");
            data.update_asset_info(serde_json::from_str(&assets).expect("Unable to parse asset info")).await;
        }

        let intents = serenity::GatewayIntents::non_privileged();

        let framework = poise::Framework::builder()
        .options(poise::FrameworkOptions {
            commands: commands::get_commands(),
            ..Default::default()
        })
        .setup(|ctx, _ready, framework| {
            Box::pin(async move {
                poise::builtins::register_globally(ctx, &framework.options().commands).await?;
                Ok(std::sync::Arc::new(data))
            })
        })
        .build();

        serenity::ClientBuilder::new(discord_token, intents)
            .framework(framework)
            .await
            .unwrap()
    };

    // And awayyyy we gooo
    client.start().await.unwrap();
}
