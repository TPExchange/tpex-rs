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
    let path: &str = &argv[1];
    let asset_path: &str = &argv[2];
    let mut assets = String::new();
    tokio::fs::File::open(asset_path).await.expect("Unable to open asset info").read_to_string(&mut assets).await.expect("Unable to read asset list.");
    let mut trade_file = tokio::fs::File::options().read(true).write(true).truncate(false).create(true).open(path).await.expect("Unable to open trade list.");
    let state = trade::State::replay(&mut trade_file, serde_json::from_str(&assets).expect("Unable to parse asset info.")).await.expect("Could not replay trades.");
    let data = std::sync::Arc::new(tokio::sync::RwLock::new(commands::Data{state, trade_file}));

    // Discord setup
    let mut client = {
        let token = std::env::var("DISCORD_TOKEN").expect("missing DISCORD_TOKEN");
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
