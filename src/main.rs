mod commands;
mod trade;
use poise::serenity_prelude as serenity;
use tokio::io::AsyncReadExt;

#[tokio::main]
async fn main() {
    // The code here just starts the discord bot, as we respond to commands

    // Database setup
    // let mut db = self::db::DatabaseConnection::new(std::env::var("DATABASE_URL").expect("missing DATABASE_URL")).await.expect("Failed to init database");

    let argv: Vec<_> = std::env::args().collect();
    let asset_path: &str = argv.get(1).expect("Missing asset path as first argument");
    let trades_path: &str = argv.get(2).expect("Missing trades path as second argument");
    let mut assets = String::new();
    tokio::fs::File::open(asset_path).await.expect("Unable to open asset info").read_to_string(&mut assets).await.expect("Unable to read asset list");
    let mut trade_file = tokio::fs::File::options().read(true).write(true).truncate(false).create(true).open(trades_path).await.expect("Unable to open trade list");
    let state = trade::State::replay(&mut trade_file, serde_json::from_str(&assets).expect("Unable to parse asset info")).await.expect("Could not replay trades");

    let Ok(token) = std::env::var("DISCORD_TOKEN")
    else {
        println!("Missing DISCORD_TOKEN, so verification mode enabled.\nState result:\n{}", serde_json::to_string_pretty(&state).expect("Could not serialise state"));
        return;
    };

    // Discord setup
    let mut client = {
        let data = std::sync::Arc::new(tokio::sync::RwLock::new(commands::Data{state, trade_file}));
        let intents = serenity::GatewayIntents::non_privileged();

        let framework = poise::Framework::builder()
        .options(poise::FrameworkOptions::<commands::WrappedData, commands::Error> {
            commands: commands::get_commands(),
            ..Default::default()
        })
        .setup(|ctx, _ready, framework| {
            Box::pin(async move {
                poise::builtins::register_globally(ctx, &framework.options().commands).await?;
                Ok(data)
            })
        })
        .build();

        serenity::ClientBuilder::new(token, intents)
            .framework(framework)
            .await
            .unwrap()
    };

    // And awayyyy we gooo
    client.start().await.unwrap();
}
