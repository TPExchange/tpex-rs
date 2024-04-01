// XXX: make sure to put the check in for EVERY command you add!

use std::ops::Deref;

use poise::{serenity_prelude::{self as serenity, Mentionable}, CreateReply};

use crate::{commands::{list_assets, user_id}, trade::{Action, PlayerId}};

use super::{player_id, Context, Error};
// Commands that handle withdrawals
#[poise::command(slash_command, ephemeral, subcommands("raw", "deposit", "complete", "current", "authorise"), check = check)]
pub async fn banker(_ctx: Context<'_>) -> Result<(), Error> { panic!("Banker metacommand called."); }

async fn check(ctx: Context<'_>) -> Result<bool, Error> {
    Ok(ctx.data().read().await.state.is_banker(&player_id(ctx.author())))
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
    ctx.data().write().await.run_action(action).await?;
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
    #[description = "The amount of that asset to be deposited"]
    count: u64
) -> Result<(), Error> {
    let player = player_id(&player);
    let banker = player_id(ctx.author());
    let response = format!("Deposited {count} {asset} for {player}.");
    ctx.data().write().await.run_action(Action::Deposit { player, asset, count, banker }).await?;
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
    // Do these back to back
    {
        let mut data = ctx.data().write().await;
        data.run_action(Action::Deposit { player: PlayerId::the_bank(), asset: asset.clone(), count, banker }).await?;
        data.run_action(Action::Invest { player: PlayerId::the_bank(), asset, count }).await.expect("Unable to invest from bank");
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
    ctx.data().write().await.run_action(Action::WithdrawlCompleted { target: withdrawal_id, banker }).await?;
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
    ctx.data().write().await.run_action(Action::TransferCoins { payer: PlayerId::the_bank(), payee: banker, count: coins }).await?;

    ctx.reply("Profits taken").await?;
    Ok(())
}

/// Gets the next withdrawal that needs to be completed
#[poise::command(slash_command,ephemeral, check = check)]
pub async fn current(ctx: Context<'_>) -> Result<(), Error> {
    let Some(current) = ctx.data().read().await.state.get_next_withdrawal()
    else {
        ctx.reply("No withdrawals left.").await?;
        return Ok(());
    };

    ctx.send(
        CreateReply::default()
        .embed(list_assets(ctx.data().read().await.deref(), &current.assets)?)
        .content(format!("Deliver to {}", user_id(&current.player).expect("Invalid player ID").mention()))
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
    ctx.data().write().await.run_action(Action::AuthoriseRestricted { authorisee: player_id(&player), banker: player_id(ctx.author()), asset, new_count }).await?;
    Ok(())
}
