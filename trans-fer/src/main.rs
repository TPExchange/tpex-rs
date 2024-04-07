use poise::serenity_prelude as serenity;
use tokio::io::AsyncReadExt;

mod commands;

#[tokio::main]
async fn main() {
    // The code here just starts the discord bot, as we respond to commands

    // Database setup
    // let mut db = self::db::DatabaseConnection::new(std::env::var("DATABASE_URL").expect("missing DATABASE_URL")).await.expect("Failed to init database");

    let argv: Vec<_> = std::env::args().collect();
    let asset_path: &str = argv.get(1).expect("Missing asset path as first argument");
    let remote_url = argv.get(2).expect("Missing trades path as second argument").parse().expect("Could not parse remote url");
    let mut assets = String::new();
    tokio::fs::File::open(asset_path).await.expect("Unable to open asset info").read_to_string(&mut assets).await.expect("Unable to read asset list");

    let remote_token: tpex_api::Token = std::env::var("TPEX_TOKEN").expect("Missing TPEX_TOKEN environment variable").parse().expect("Could not parse TPEX_TOKEN");

    let discord_token = std::env::var("DISCORD_TOKEN").expect("Missing DISCORD_TOKEN environment variable");

    // Discord setup
    let mut client = {
        let data = tpex_api::Mirrored::new(
            serde_json::from_str(&assets).expect("Unable to parse asset list"),
            remote_url,
            remote_token).await;
        let intents = serenity::GatewayIntents::non_privileged();

        let framework = poise::Framework::builder()
        .options(poise::FrameworkOptions::<std::sync::Arc<tpex_api::Mirrored>, commands::Error> {
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
