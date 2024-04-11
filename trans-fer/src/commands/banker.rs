use std::ops::Deref;

use itertools::Itertools;
// XXX: make sure to put the check in for EVERY command you add!
use poise::{serenity_prelude::{self as serenity, CreateEmbed, Mentionable}, CreateReply};

use crate::commands::{list_assets, user_id, AutoConversion};
use tpex::{Action, PlayerId};

use super::{player_id, Context, Error};
// Commands that handle withdrawals
#[poise::command(slash_command, ephemeral, subcommands("raw", "deposit", "complete", "current", "authorise", "undeposit", "autoconvert"), check = check)]
pub async fn banker(_ctx: Context<'_>) -> Result<(), Error> { panic!("Banker metacommand called."); }

async fn check(ctx: Context<'_>) -> Result<bool, Error> {
    if ctx.data().sync().await.is_banker(&player_id(ctx.author())) {
        Ok(true)
    }
    else {
        // We *cannot* let this fail or mess anything up
        let _ = ctx.reply("This is a banker-only command!").await;
        Ok(false)
    }
}


#[poise::command(slash_command, ephemeral, subcommands("list","update","remove"), check = check)]
async fn autoconvert(_ctx: Context<'_>) -> Result<(), Error> { panic!("Autoconvert metacommand called."); }
/// Lists all the autoconversions
#[poise::command(slash_command,ephemeral, check = check)]
async fn list(ctx: Context<'_>) -> Result<(), Error> {
    let list = ctx.data().db.list_autoconversions().await;
    let (froms,tos,scales) : (Vec<_>,Vec<_>,Vec<_>) = list
        .iter()
        .sorted_by_cached_key(|record| &record.from)
        .map(|record| (&record.from,&record.to,record.scale))
        .multiunzip();
    ctx.send(CreateReply::default()
        .embed(CreateEmbed::default()
            .field("From", froms.into_iter().join("\n"), true)
            .field("To", tos.into_iter().join("\n"), true)
            .field("Scale", scales.into_iter().join("\n"), true)
        )
    ).await?;
    Ok(())
}
/// Updates or adds an autoconversion
#[poise::command(slash_command,ephemeral, check = check)]
async fn update(ctx: Context<'_>,
    #[description = "The asset to convert from"]
    from: tpex::AssetId,
    #[description = "The asset to convert to"]
    to: tpex::AssetId,
    #[description = "The number of `to` that each `from` should create"]
    scale: u64
) -> Result<(), Error> {
    let response = format!("Will now convert each {} into {} {}", from, scale, to);
    ctx.data().db.update_autoconversion(AutoConversion{from,to,scale}).await;
    ctx.reply(response).await?;
    Ok(())
}
/// Updates or adds an autoconversion
#[poise::command(slash_command,ephemeral, check = check)]
async fn remove(ctx: Context<'_>,
    #[description = "The asset that will no longer be converted from"]
    from: tpex::AssetId
) -> Result<(), Error> {
    let response = format!("Will no longer convert {}", from);
    ctx.data().db.delete_autoconversion(&from).await;
    ctx.reply(response).await?;
    Ok(())
}

/// Run a raw JSON action on the state. DANGEROUS AF!!!
#[poise::command(slash_command,ephemeral, check = check)]
pub async fn raw(
    ctx: Context<'_>,
    #[description = "JSON action"]
    command: String,
) -> Result<(), Error> {
    let Ok(action) = serde_json::from_str(&command) else {
        ctx.say("Invalid command.").await?;
        return Ok(());
    };
    ctx.data().apply(action).await?;
    ctx.reply("Action succeeded!").await?;
    Ok(())
}

/// Mark resources as deposited for a user
#[poise::command(slash_command,ephemeral, check = check)]
pub async fn deposit(
    ctx: Context<'_>,
    #[description = "The depositing user"]
    player: serenity::User,
    #[description = "The asset to be deposited"]
    mut asset: String,
    #[description = "The asset to be deposited, again"]
    asset_again: String,
    #[description = "The amount of that asset to be deposited"]
    mut count: u64,
    #[description = "The amount of that asset to be deposited, again"]
    count_again: u64
) -> Result<(), Error> {
    if asset != asset_again || count != count_again {
        ctx.reply("Inconsistent asset or count. PLEASE CHECK FOR TYPOS NEXT TIME!!!").await?;
        return Ok(());
    }
    let player = player_id(&player);
    let banker = player_id(ctx.author());
    let response = format!("Deposited {count} {asset} for {player}.");
    // Do autoconvert
    if let Some(convert) = ctx.data().db.get_autoconversion(&asset).await {
        asset = convert.to.clone();
        count *= convert.scale;
    }
    ctx.data().apply(Action::Deposit { player: player.clone(), asset: asset.clone(), count, banker }).await?;

    if asset == tpex::DIAMOND_NAME {
        ctx.data().apply(Action::BuyCoins { player, n_diamonds: count * tpex::COINS_PER_DIAMOND }).await?;
    }
    ctx.reply(response).await?;
    Ok(())
}
/// Mark resources as deposited for a user
#[poise::command(slash_command,ephemeral, check = check)]
pub async fn undeposit(
    ctx: Context<'_>,
    #[description = "The depositing user"]
    player: serenity::User,
    #[description = "The asset to be removed"]
    mut asset: String,
    #[description = "The asset to be removed, again"]
    asset_again: String,
    #[description = "The amount of that asset to be removed"]
    mut count: u64,
    #[description = "The amount of that asset to be removed, again"]
    count_again: u64
) -> Result<(), Error> {
    if asset != asset_again || count != count_again {
        ctx.reply("Typo in asset or count. PLEASE CHECK FOR TYPOS NEXT TIME!!!").await?;
        return Ok(());
    }

    // Do autoconvert
    if let Some(convert) = ctx.data().db.get_autoconversion(&asset).await {
        asset = convert.to.clone();
        count *= convert.scale;
    }
    if asset == tpex::DIAMOND_NAME {
        ctx.reply("Cannot undo diamonds, as these were autoconverted. This requires manual intervention :(").await?;
        return Ok(())
    }
    let player = player_id(&player);
    let banker = player_id(ctx.author());
    let response = format!("Deposited {count} {asset} for {player}.");
    ctx.data().apply(Action::Undeposit { player, asset, count, banker }).await?;
    ctx.reply(response).await?;
    Ok(())
}

/// Mark resources as deposited for the bank
#[poise::command(slash_command,ephemeral, check = check)]
pub async fn reserve(
    ctx: Context<'_>,
    #[description = "The asset to be added to the reserve"]
    asset: String,
    #[description = "The amount of that asset to be added"]
    count: u64
) -> Result<(), Error> {
    let banker = player_id(ctx.author());
    let response = format!("Added {count} {asset} to the reserve.");
    // Do these back to back, but not necessarily consecutively
    {
        ctx.data().apply(Action::Deposit { player: PlayerId::the_bank(), asset: asset.clone(), count, banker }).await?;
        ctx.data().apply(Action::Invest { player: PlayerId::the_bank(), asset, count }).await?;
    }
    ctx.reply(response).await?;
    Ok(())
}

/// Mark a withdrawal as completed
#[poise::command(slash_command,ephemeral, check = check)]
pub async fn complete(
    ctx: Context<'_>,
    #[description = "The ID of the deposit to complete"]
    withdrawal_id: u64
) -> Result<(), Error> {
    let banker = player_id(ctx.author());
    ctx.data().apply(Action::WithdrawlCompleted { target: withdrawal_id, banker }).await?;
    ctx.reply("Withdrawal completion succeeded.").await?;
    Ok(())
}

/// Take profits
#[poise::command(slash_command,ephemeral, check = check)]
pub async fn pay(
    ctx: Context<'_>,
    #[description = "The banker to pay"]
    coins: u64
) -> Result<(), Error> {
    let banker = player_id(ctx.author());
    ctx.data().apply(Action::TransferCoins { payer: PlayerId::the_bank(), payee: banker, count: coins }).await?;

    ctx.reply("Profits taken").await?;
    Ok(())
}

/// Gets the next withdrawal that needs to be completed
#[poise::command(slash_command,ephemeral, check = check)]
pub async fn current(ctx: Context<'_>) -> Result<(), Error> {
    let Some(current) = ctx.data().sync().await.get_next_withdrawal()
    else {
        ctx.reply("No withdrawals left.").await?;
        return Ok(());
    };

    ctx.send(
        CreateReply::default()
        .embed(list_assets(ctx.data().sync().await.deref(), &current.assets)?)
        .content(format!("Deliver to {} (ID: {})", user_id(&current.player).expect("Invalid player ID").mention(), current.id))
    ).await?;
    Ok(())
}

/// Gets the next withdrawal that needs to be completed
#[poise::command(slash_command,ephemeral, check = check)]
pub async fn authorise(ctx: Context<'_>,
    #[description = "The authorised user"]
    player: serenity::User,
    #[description = "The asset to be authorised"]
    asset: String,
    #[description = "The new amount of that asset to be authorised"]
    new_count: u64
) -> Result<(), Error> {
    ctx.data().apply(Action::AuthoriseRestricted { authorisee: player_id(&player), banker: player_id(ctx.author()), asset, new_count }).await?;
    Ok(())
}
