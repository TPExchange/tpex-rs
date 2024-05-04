use std::borrow::Borrow;

use crate::commands::{list_assets, player_id, user_id};
use tpex::Action;
use poise::{serenity_prelude::{self as serenity, CreateInteractionResponseMessage, CreateMessage}, CreateReply};

use super::{Context, Error};

#[derive(Debug, poise::Modal)]
struct SetItemCountModal {
    item: String,
    count: String
}
/// Commands that handle withdrawals
#[poise::command(slash_command,ephemeral, subcommands("new", "pending"))]
pub async fn withdraw(_ctx: Context<'_>) -> Result<(), Error> { panic!("withdraw metacommand called!"); }

/// List your pending withdrawals
#[poise::command(slash_command,ephemeral)]
async fn pending(ctx: Context<'_>) -> Result<(), Error> {
    ctx.defer_ephemeral().await?;

    let ctx_id = ctx.id();
    let ctx_suffix = format!("_{ctx_id}");
    let prev_button_id = format!("prev{ctx_suffix}");
    let next_button_id = format!("next{ctx_suffix}");
    let expedite_button_id = format!("expedite{ctx_suffix}");
    let refresh_button_id = format!("refresh{ctx_suffix}");

    let components = serenity::CreateActionRow::Buttons(vec![
        serenity::CreateButton::new(&prev_button_id).emoji('◀'),
        serenity::CreateButton::new(&expedite_button_id).label("Expedite").style(serenity::ButtonStyle::Primary),
        serenity::CreateButton::new(&refresh_button_id).label("Refresh").style(serenity::ButtonStyle::Primary),
        serenity::CreateButton::new(&next_button_id).emoji('▶'),
    ]);

    let mut curr_id = u64::MAX;
    let ui = ctx.reply("Loading withdrawals").await?;
    loop {
        let prev_id;
        let next_id;
        let withdrawal;

        // This will lock the entire data stream, so be careful
        let data = ctx.data().sync().await;
        let mut withdrawals = data.get_withdrawals();
        let user = player_id(ctx.author());
        withdrawals.retain(|_, x| x.player == user);

        // Recheck what the nearest id is, and get the ones either side while we're at it
        ((prev_id, curr_id, next_id), withdrawal) = {
            let mut lower_range = withdrawals.range(..curr_id).rev();
            let mut upper_range = withdrawals.range(curr_id..);

            match (lower_range.next(), upper_range.next()) {
                (Some(closest), None) =>
                    ((lower_range.next().map(|i| i.0), *closest.0, None), closest.1),
                (None, Some(closest)) =>
                    ((None, *closest.0, upper_range.next().map(|i| i.0)), closest.1),
                (Some(lower), Some(upper)) => {
                    if curr_id.abs_diff(*lower.0) < curr_id.abs_diff(*upper.0) {
                        ((lower_range.next().map(|i| i.0), *lower.0, Some(upper.0)), lower.1)
                    }
                    else {
                        ((Some(lower.0), *upper.0, upper_range.next().map(|i| i.0)), upper.1)
                    }
                },
                (None, None) => {
                    // All withdrawals have completed, we have nothing left
                    ui.edit(ctx, CreateReply::default().content("No withdrawals left.")).await?;
                    return Ok(());
                }
            }
        };

        ui.edit(ctx, CreateReply::default()
            .content("")
            .embed(list_assets(data.borrow(), &withdrawal.assets)?.field("Expedited", withdrawal.expedited.to_string(), false).field("ID", curr_id.to_string(), false))
            .components(vec![components.clone()])
        ).await?;
        drop(data);

        let Some(mci) = serenity::ComponentInteractionCollector::new(ctx)
            .author_id(ctx.author().id)
            .channel_id(ctx.channel_id())
            // FIXME: Filter is weird with captures and I cba
            // .filter(move |mci| mci.data.custom_id.ends_with(&*suffix))
            .await
        else { return Ok(()); };
        match &mci.data.custom_id {
            x if x == &prev_button_id => {
                // idk if someone can mess with this, so I'm going to soft check
                if let Some(id) = prev_id { curr_id = *id; }
                mci.create_response(ctx, serenity::CreateInteractionResponse::Acknowledge).await?;
                continue;
            },
            x if x == &next_button_id => {
                // idk if someone can mess with this, so I'm going to soft check
                if let Some(id) = next_id { curr_id = *id; }
                mci.create_response(ctx, serenity::CreateInteractionResponse::Acknowledge).await?;
                continue;
            },
            x if x == &expedite_button_id => {
                // Check to make sure the user is aware this isn't free
                let fee = ctx.data().sync().await.expedite_fee().to_string();

                // Because discord doesn't bother to tell us if the use canceled, this must be done as a task
                let serenity_ctx = ctx.serenity_context().clone();
                let data = ctx.data().clone();
                tokio::spawn(async move {
                    let Some(check_modal) = mci.quick_modal(&serenity_ctx,
                        serenity::CreateQuickModal::new("Are you sure?")
                        .short_field(format!("Type \"{fee}\" (The fee you will pay):"))).await?
                    else {
                        return Ok::<(), Error>(())
                    };
                    if check_modal.inputs[0] != fee {
                        return Ok(());
                    }
                    check_modal.interaction.create_response(&serenity_ctx.http, serenity::CreateInteractionResponse::Acknowledge).await?;
                    // We don't need to check further, as ids are unique, and so the only way a user could get this is if they satisfied the earlier name filter
                    data.apply(Action::Expedited { target: curr_id }).await?;
                    // DM all bankers
                    //
                    // TODO: parallelise
                    for id in data.sync().await.get_bankers() {
                        let user = user_id(&id).expect("Unable to parse banker ID").to_user(&serenity_ctx.http).await.expect("Unable to contact banker.");
                        user.dm(&serenity_ctx, CreateMessage::new().content("New expedited order!")).await.expect("Unable to DM banker.");
                    }
                    Ok(())
                });
            }
            x if x == &refresh_button_id => { mci.create_response(ctx, serenity::CreateInteractionResponse::Acknowledge).await?; },
            _ => ()
        }
    }
}

/// Begins a withdrawal request
#[poise::command(slash_command,ephemeral)]
pub async fn new(ctx: Context<'_>) -> Result<(), Error> {
    ctx.defer_ephemeral().await?;

    const LIFETIME: std::time::Duration = std::time::Duration::from_secs(5 * 60); //5 * 60
    let die_time = (ctx.created_at().naive_utc() + LIFETIME).and_utc();
    let die_unix = die_time.timestamp();

    let ctx_id = ctx.id();
    let ctx_suffix = format!("_{ctx_id}");
    let withdraw_id = format!("withdraw{ctx_suffix}");
    let cancel_id = format!("cancel{ctx_suffix}");
    let set_id = format!("add{ctx_suffix}");

    let components = vec![
        serenity::CreateActionRow::Buttons(vec![
            serenity::CreateButton::new(withdraw_id.clone())
                .label("Withdraw")
                .style(poise::serenity_prelude::ButtonStyle::Success)
                .disabled(false),
            serenity::CreateButton::new(cancel_id.clone())
                .label("Cancel")
                .style(poise::serenity_prelude::ButtonStyle::Danger)
                .disabled(false),
            serenity::CreateButton::new(set_id.clone())
                .label("Set item count")
                .style(poise::serenity_prelude::ButtonStyle::Primary)
                .disabled(false),
        ])
    ];

    let basket = std::sync::Arc::new(tokio::sync::Mutex::new(std::collections::HashMap::new()));

    let ui = ctx.send(
        poise::CreateReply::default()
        .content(format!("This basket will be deleted <t:{die_unix}:R>."))
        .embed(
            serenity::CreateEmbed::new()
            .field("Name", "", true)
            .field("Count", "", true)
            .field("Fees", ctx.data().sync().await.calc_withdrawal_fee(&std::collections::HashMap::new())?.to_string() + "c", false)
        )
        .components(components)
    ).await?;

    loop {
        let Ok(timeout) = die_time.signed_duration_since(std::convert::Into::<chrono::DateTime<chrono::Utc>>::into(std::time::SystemTime::now())).to_std()
        else {
            // The user hasn't done anything for ages, so the message has timed out
            let _ = ui.delete(ctx).await;
            return Ok(());
        };

        let Some(mci) = serenity::ComponentInteractionCollector::new(ctx)
            .author_id(ctx.author().id)
            .channel_id(ctx.channel_id())
            // .timeout(timeout)
            // FIXME: Filter is weird with captures and I cba
            // .filter(move |mci| mci.data.custom_id.ends_with(&*suffix))
            .await
        else {
            // Keep looping otherwise
            continue;
        };
        match &mci.data.custom_id {
            x if x == &cancel_id => {
                mci.create_response(ctx, serenity::CreateInteractionResponse::Acknowledge).await?;
                ui.delete(ctx).await?;
                return Ok(());
            },
            x if x == &withdraw_id => {
                // FIXME: Because discord doesn't bother to tell us if the use canceled, this must be done as a task

                // Check they want to pay!
                let data = ctx.data().clone();
                // Make a copy so that they can't claim some future withdrawal
                let basket = basket.lock().await.clone();
                let fee = data.sync().await.calc_withdrawal_fee(&basket)?.to_string();
                let serenity_ctx = ctx.serenity_context().clone();
                let player = player_id(ctx.author());
                tokio::spawn(async move {
                    if basket.is_empty() {
                        let Some(warn_modal) = mci.quick_modal(&serenity_ctx,
                            serenity::CreateQuickModal::new("Warning")
                            .short_field("You tried to withdraw an empty basket. Please close this, and add something first!")).await?
                        else {
                            return Ok::<(), Error>(())
                        };
                        warn_modal.interaction.create_response(serenity_ctx.http, serenity::CreateInteractionResponse::Acknowledge).await?;
                        return Ok(());
                    }

                    let Some(check_modal) = mci.quick_modal(&serenity_ctx,
                        serenity::CreateQuickModal::new("Are you sure?")
                        .short_field(format!("Type \"{fee}\" (The fee you will pay):"))).await?
                    else {
                        // ctx.say("Withdrawl canceled!").await?;
                        return Ok::<(), Error>(())
                    };
                    if check_modal.inputs[0] != fee {
                        // ctx.say("Incorrect amount entered. Withdrawl canceled!").await?;
                        return Ok(());
                    }

                    // Try to withdraw the items
                    match data.apply(Action::WithdrawlRequested { player, assets: basket.clone() }).await {
                        Ok(withdraw_id) => {
                            check_modal.interaction.create_response(serenity_ctx.http, serenity::CreateInteractionResponse::UpdateMessage(CreateInteractionResponseMessage::new()
                                .components(Vec::new())
                                .content(format!("Your withdrawal of the following (ID no. {withdraw_id}) has been accepted:"))
                                .ephemeral(true)
                            )).await?;
                        },
                        Err(e) => {
                            check_modal.interaction.create_response(serenity_ctx.http, serenity::CreateInteractionResponse::Message(CreateInteractionResponseMessage::new()
                                .content(format!("Withdrawal failed: {e}"))
                                .ephemeral(true)
                            )).await?;
                        }
                    }
                    Ok(())
                });
            },
            x if x == &set_id => {
                // Because discord doesn't bother to tell us if the use canceled, this must be done as a task

                // Check they want to pay!
                let data = ctx.data().clone();
                let basket = basket.clone();
                let serenity_ctx = ctx.serenity_context().clone();
                tokio::spawn(async move {
                    let Some(modal) = mci.quick_modal(&serenity_ctx, serenity::CreateQuickModal::new("Set item count")
                        .field(serenity::CreateInputText::new(serenity::InputTextStyle::Short, "Item", "item").required(true))
                        .field(serenity::CreateInputText::new(serenity::InputTextStyle::Short, "Amount", "amount").required(true))
                        .timeout(timeout)
                    ).await?
                    else { return Ok(()); };

                    // We need it to be a non-negative integer!
                    let amount : u64 = modal.inputs[1].parse()?;

                    let msg = {
                        let mut basket = basket.lock().await;
                        if amount == 0 {
                            basket.remove(&modal.inputs[0]);
                        }
                        else {
                            basket.insert(modal.inputs[0].clone(), amount);
                        }

                        CreateInteractionResponseMessage::default()
                        .add_embed(list_assets(data.sync().await.borrow(), &basket)?)
                        .ephemeral(true)
                    };

                    modal.interaction.create_response(serenity_ctx.http, serenity::CreateInteractionResponse::UpdateMessage(msg)).await?;

                    Ok::<_, Error> (())
                });
            },
            _ => ()
        }
    }
}
