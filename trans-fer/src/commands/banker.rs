use std::ops::Deref;

// XXX: make sure to put the check in for EVERY command you add!
use poise::{serenity_prelude::{self as serenity, Mentionable}, CreateReply};

use crate::commands::{list_assets, user_id};
use tpex::{Action, PlayerId};

use super::{player_id, Context, Error};
// Commands that handle withdrawals
#[poise::command(slash_command, ephemeral, subcommands("raw", "deposit", "complete", "current", "authorise", "undeposit"), check = check)]
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

/// Run a raw JSON action on the state. DANGEROUS AF!!!
#[poise::command(slash_command,ephemeral, check = check)]
pub async fn raw(
    ctx: Context<'_>,
    #[description = "JSON action"]
    command: String,
) -> Result<(), Error> {
    ctx.defer_ephemeral().await?;
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
    asset: String,
    #[description = "The asset to be deposited, again"]
    asset_again: String,
    #[description = "The amount of that asset to be deposited"]
    count: u64,
    #[description = "The amount of that asset to be deposited, again"]
    count_again: u64
) -> Result<(), Error> {
    ctx.defer_ephemeral().await?;
    if asset != asset_again || count != count_again {
        ctx.reply("Inconsistent asset or count. PLEASE CHECK FOR TYPOS NEXT TIME!!!").await?;
        return Ok(());
    }
    let player = player_id(&player);
    let banker = player_id(ctx.author());
    let response = format!("Deposited {count} {asset} for {player}.");

    ctx.data().apply(Action::Deposit { player: player.clone(), asset: asset.clone(), count, banker }).await?;

    if asset == tpex::DIAMOND_NAME {
        ctx.data().apply(Action::BuyCoins { player, n_diamonds: count }).await?;
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
    asset: String,
    #[description = "The asset to be removed, again"]
    asset_again: String,
    #[description = "The amount of that asset to be removed"]
    count: u64,
    #[description = "The amount of that asset to be removed, again"]
    count_again: u64
) -> Result<(), Error> {
    ctx.defer_ephemeral().await?;
    if asset != asset_again || count != count_again {
        ctx.reply("Typo in asset or count. PLEASE CHECK FOR TYPOS NEXT TIME!!!").await?;
        return Ok(());
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
    ctx.defer_ephemeral().await?;
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
    ctx.defer_ephemeral().await?;
    let banker = player_id(ctx.author());
    ctx.data().apply(Action::WithdrawalCompleted { target: withdrawal_id, banker }).await?;
    ctx.reply("Withdrawal completion succeeded.").await?;
    Ok(())
}

/// Gets the next withdrawal that needs to be completed
#[poise::command(slash_command,ephemeral, check = check)]
pub async fn current(ctx: Context<'_>) -> Result<(), Error> {
    ctx.defer_ephemeral().await?;
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
    ctx.defer_ephemeral().await?;
    ctx.data().apply(Action::AuthoriseRestricted { authorisee: player_id(&player), banker: player_id(ctx.author()), asset, new_count }).await?;
    Ok(())
}
