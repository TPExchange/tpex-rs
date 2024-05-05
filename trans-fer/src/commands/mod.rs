mod withdraw;
mod order;
mod banker;

use tpex::{AssetId, Auditable, Coins, PlayerId};
use poise::serenity_prelude::{self as serenity, CreateEmbed};
use itertools::Itertools;

#[derive(Debug, PartialEq, Clone, Default)]
#[derive(sqlx::FromRow)]
pub struct AutoConversion {
    // We don't have n_from, as that would give inconsistent conversion. 1:n only!
    pub from: AssetId,
    pub to: AssetId,
    pub scale: u64
}

pub struct Data {
    pub state: tpex_api::Mirrored,
    // pub db: Database
}
impl std::ops::Deref for Data {
    type Target = tpex_api::Mirrored;

    fn deref(&self) -> &Self::Target { &self.state }
}

pub(crate) type Error = Box<dyn std::error::Error + Send + Sync>;
type Context<'a> = poise::Context<'a, std::sync::Arc<Data>, Error>;

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
    ctx.defer_ephemeral().await?;

    let player = player.as_ref().unwrap_or(ctx.author());
    let name = player.name.clone();
    let player = player_id(player);
    let (bal, assets) = {
        let state = ctx.data().sync().await;
        (state.get_bal(&player), state.get_assets(&player))
    };
    ctx.send(
        poise::CreateReply::default()
        .content(format!("{} has {}.", name, bal))
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
    #[description = "The number of diamonds you wish to exchange for Coin(s) (1000c per diamond)"]
    n_diamonds: u64,
) -> Result<(), Error> {
    ctx.defer_ephemeral().await?;
    let player = player_id(ctx.author());
    ctx.data().apply(tpex::Action::BuyCoins { player, n_diamonds }).await?;
    ctx.reply(format!("You have succesfully bought {} for {} diamonds", Coins::from_diamonds(n_diamonds)?, n_diamonds)).await?;
    Ok(())
}
/// Convert your coins into diamonds, with 1000c for 1 diamond
#[poise::command(slash_command,ephemeral)]
async fn sellcoins(
    ctx: Context<'_>,
    #[description = "The number of diamonds you wish to get (1000c per diamond)"]
    n_diamonds: u64,
) -> Result<(), Error> {
    ctx.defer_ephemeral().await?;
    let player = player_id(ctx.author());
    ctx.data().apply(tpex::Action::SellCoins { player, n_diamonds }).await?;
    ctx.reply(format!("You have succesfully bought {} diamonds for {}", n_diamonds, Coins::from_diamonds(n_diamonds)?)).await?;
    Ok(())
}
/// Get the machine-readable list of all transactions
#[poise::command(slash_command,ephemeral)]
async fn txlog(
    ctx: Context<'_>
) -> Result<(), Error> {
    ctx.defer_ephemeral().await?;
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
    ctx.defer_ephemeral().await?;
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
    ctx.defer_ephemeral().await?;
    let state = serde_json::to_string_pretty(&*ctx.data().sync().await)?;
    ctx.send(poise::CreateReply::default()
        .attachment(serenity::CreateAttachment::bytes(state, "state.json"))
    ).await?;
    Ok(())
}

/// Get a list of everything in the bank
#[poise::command(slash_command,ephemeral)]
async fn audit(ctx: Context<'_>) -> Result<(), Error> {
    ctx.defer_ephemeral().await?;
    let audit = ctx.data().sync().await.soft_audit();
    let sorted_assets = std::collections::BTreeMap::from_iter(audit.assets);
    ctx.send(poise::CreateReply::default()
        .content(audit.coins.to_string())
        .embed(CreateEmbed::new()
            .field("Name", sorted_assets.keys().join("\n"), true)
            .field("Count", sorted_assets.values().join("\n"), true)
        )
    ).await?;
    Ok(())
}

/// List players in descending order of coin balance
#[poise::command(slash_command,ephemeral)]
async fn baltop(ctx: Context<'_>) -> Result<(), Error> {
    ctx.defer_ephemeral().await?;
    let (names,coins) : (Vec<_>, Vec<_>) = ctx.data().sync().await
        .get_bals()
        .into_iter()
        .sorted_by_key(|(_,key)| *key)
        .rev()
        .unzip();

    let names = {
        let mut new_names = Vec::with_capacity(names.len());
        for i in names {
            let name = user_id(&i).map(|x| x.to_user(&ctx));
            let name = match name { Some(fut) => fut.await.ok(), None => None };
            #[allow(deprecated)]
            let name = name.map(|x| x.name).unwrap_or_else(|| i.evil_deref().to_string());
            new_names.push(name);
        }
        new_names
    };

    ctx.send(poise::CreateReply::default()
        .embed(CreateEmbed::new()
            .field("Name", names.into_iter().join("\n"), true)
            .field("Coins", coins.into_iter().join("\n"), true)
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
        .field("Fees", state.calc_withdrawal_fee(assets)?.to_string(), false)
    )
}


pub fn get_commands() -> Vec<poise::Command<std::sync::Arc<Data>, Error>> {
    vec![
        balance(),
        buycoins(),
        sellcoins(),
        txlog(),
        restricted(),
        state_info(),
        audit(),
        baltop(),

        withdraw::withdraw(),
        order::order(),
        banker::banker()
    ]
}
