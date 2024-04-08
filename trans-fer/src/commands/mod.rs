mod withdraw;
mod order;
mod banker;

use tpex::{AssetId, PlayerId, Auditable};
use poise::serenity_prelude::{self as serenity, CreateEmbed};
use itertools::Itertools;

pub(crate) type Error = Box<dyn std::error::Error + Send + Sync>;
type Context<'a> = poise::Context<'a, std::sync::Arc<tpex_api::Mirrored>, Error>;

fn player_id(user: &serenity::User) -> PlayerId {
    #[allow(deprecated)]
    PlayerId::evil_constructor(user.id.to_string())
}
fn user_id(player: &PlayerId) -> Option<serenity::UserId> {
    #[allow(deprecated)]
    PlayerId::evil_deref(player).parse().ok()
}

/// Get the coins and assets of a player
#[poise::command(slash_command,ephemeral)]
async fn balance(
    ctx: Context<'_>,
    #[description = "Player (Defaults to you)"]
    player: Option<serenity::User>,
) -> Result<(), Error> {
    let player = player.as_ref().unwrap_or(ctx.author());
    let name = player.name.clone();
    let player = player_id(player);
    let (bal, assets) = {
        let state = ctx.data().sync().await;
        (state.get_bal(&player), state.get_assets(&player))
    };
    ctx.send(
        poise::CreateReply::default()
        .content(format!("{} has {} coins.", name, bal))
        .embed(
            serenity::CreateEmbed::new()
            .field("Name", assets.keys().join("\n"), true)
            .field("Count", assets.values().join("\n"), true)
        )
    ).await?;
    Ok(())
}
/// Convert your diamonds into coins
#[poise::command(slash_command,ephemeral)]
async fn buycoins(
    ctx: Context<'_>,
    #[description = "The number of diamonds you wish to exchange for Coin(s)"]
    n_diamonds: u64,
) -> Result<(), Error> {
    let player = player_id(ctx.author());
    ctx.data().apply(tpex::Action::BuyCoins { player, n_diamonds }).await?;
    ctx.reply("Purchase successful").await?;
    Ok(())
}
/// Convert your coins into diamonds
#[poise::command(slash_command,ephemeral)]
async fn sellcoins(
    ctx: Context<'_>,
    #[description = "The number of diamonds you wish to get"]
    n_diamonds: u64,
) -> Result<(), Error> {
    let player = player_id(ctx.author());
    ctx.data().apply(tpex::Action::SellCoins { player, n_diamonds }).await?;
    ctx.reply("Purchase successful").await?;
    Ok(())
}
/// Get the machine-readable list of all transactions
#[poise::command(slash_command,ephemeral)]
async fn txlog(
    ctx: Context<'_>
) -> Result<(), Error> {
    // Lock read means no trades will be appended while we withdraw: i.e. no partial writes
    let data = ctx.data().remote.get_state(0).await?;

    ctx.send(poise::CreateReply::default()
        .attachment(serenity::CreateAttachment::bytes(data, "trades.list"))
    ).await?;
    Ok(())
}
/// Get the list of items that require authorisation to withdraw
#[poise::command(slash_command,ephemeral)]
async fn restricted(ctx: Context<'_>) -> Result<(), Error> {
    let assets = ctx.data().sync().await.get_restricted().join("\n");
    ctx.send(
        poise::CreateReply::default()
        .embed(
            serenity::CreateEmbed::new()
            .description("Restricted items:")
            .field("Name", assets, true)
        )
    ).await?;
    Ok(())
}
/// Get an info dump of the current state
#[poise::command(slash_command,ephemeral)]
async fn state_info(ctx: Context<'_>) -> Result<(), Error> {
    let state = serde_json::to_string(&*ctx.data().sync().await)?;
    ctx.send(poise::CreateReply::default()
        .attachment(serenity::CreateAttachment::bytes(state, "state.json"))
    ).await?;
    Ok(())
}

/// Get a list of everything in the bank
#[poise::command(slash_command,ephemeral)]
async fn audit(ctx: Context<'_>) -> Result<(), Error> {
    let audit = ctx.data().sync().await.soft_audit();
    ctx.send(poise::CreateReply::default()
        .content(format!("{} coins", audit.coins))
        .embed(CreateEmbed::new()
            .field("Name", audit.assets.keys().join("\n"), true)
            .field("Count", audit.assets.values().join("\n"), true)
        )
    ).await?;
    Ok(())
}

fn list_assets(state: &tpex::State, assets: &std::collections::HashMap<AssetId, u64>) -> Result<CreateEmbed, Error> {
    Ok(
        CreateEmbed::new()
        .field("Name", assets.keys().join("\n"), true)
        .field("Count", assets.values().join("\n"), true)
        .field("Restricted",  assets.keys().map(|x| state.is_restricted(x).to_string()).join("\n"), true)
        .field("Fees", state.calc_withdrawal_fee(assets)?.to_string() + " Coin(s)", false)
    )
}

pub fn get_commands() -> Vec<poise::Command<std::sync::Arc<tpex_api::Mirrored>, Error>> {
    vec![
        balance(),
        buycoins(),
        sellcoins(),
        txlog(),
        restricted(),
        state_info(),
        audit(),

        withdraw::withdraw(),
        order::order(),
        banker::banker()
    ]
}
