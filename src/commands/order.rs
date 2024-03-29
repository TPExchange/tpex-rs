use poise::{serenity_prelude::{self as serenity, CreateEmbed, CreateInteractionResponseMessage}, CreateReply};

use crate::{commands::player_id, trade::Action};

use super::{Context, Error};
// Commands that handle orders
#[poise::command(slash_command, ephemeral, subcommands("buy", "sell", "pending", "price", "cancel"))]
pub async fn order(_ctx: Context<'_>) -> Result<(), Error> { panic!("order metacommand called!"); }

/// Places a buy order
#[poise::command(slash_command, ephemeral)]
async fn buy(ctx: Context<'_>, 
    #[description = "The item you want to place a buy order for"]
    item: String,
    #[description = "The amount you want to order"]
    amount: u64,
    #[description = "The price you want to pay per item"]
    coins_per: u64
) -> Result<(), Error> {
    const LIFETIME: std::time::Duration = std::time::Duration::from_secs(5 * 60); //5 * 60
    let die_time = (ctx.created_at().naive_utc() + LIFETIME).and_utc();
    let die_unix = die_time.timestamp();

    let total = coins_per * amount;
    let ctx_id = ctx.id();
    let ctx_suffix = format!("_{ctx_id}");
    let buy_id = format!("buy{ctx_suffix}");
    let cancel_id = format!("cancel{ctx_suffix}");

    let components = vec![
        serenity::CreateActionRow::Buttons(vec![
            serenity::CreateButton::new(buy_id.clone())
                .label("Buy")
                .style(poise::serenity_prelude::ButtonStyle::Success)
                .disabled(false),
            serenity::CreateButton::new(cancel_id.clone())
                .label("Cancel")
                .style(poise::serenity_prelude::ButtonStyle::Danger)
                .disabled(false)
        ])
    ];

    let ui = ctx.send(CreateReply::default()
        .content(format!("Are you sure you want to do the following? This prompt will expire <t:{die_unix}:R>."))
        .embed(CreateEmbed::new()
            .description(format!("Buy {amount} {item} for {coins_per} Coin(s) each (totalling {total})?"))
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
            .timeout(timeout)
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
            x if x == &buy_id => {
                // Place the order
                match ctx.data().write().await.run_action(Action::BuyOrder { player: player_id(ctx.author()), asset: item, count: amount, coins_per }).await {
                    Ok(id) => {
                        mci.create_response(ctx, serenity::CreateInteractionResponse::UpdateMessage(CreateInteractionResponseMessage::new()
                            .components(Vec::new())
                            .content(format!("Your buy order of the following (ID no. {id}) has been sent:"))
                            .ephemeral(true)
                        )).await?;
                    },
                    Err(e) => {
                        mci.create_response(ctx, serenity::CreateInteractionResponse::Message(CreateInteractionResponseMessage::new()
                            .content(format!("Order failed: {e}"))
                            .ephemeral(true)
                        )).await?;
                    }
                }
                return Ok(())
            },
            _ => ()
        }
    }
}

/// Places a sell order
#[poise::command(slash_command, ephemeral)]
async fn sell(ctx: Context<'_>, 
    #[description = "The item you want to place a sell order for"]
    item: String,
    #[description = "The amount you want to order"]
    amount: u64,
    #[description = "The Coin(s) you want to get per item"]
    coins_per: u64
) -> Result<(), Error> {
    const LIFETIME: std::time::Duration = std::time::Duration::from_secs(5 * 60); //5 * 60
    let die_time = (ctx.created_at().naive_utc() + LIFETIME).and_utc();
    let die_unix = die_time.timestamp();

    let total = coins_per * amount;
    let ctx_id = ctx.id();
    let ctx_suffix = format!("_{ctx_id}");

    let sell_id = format!("sell{ctx_suffix}");
    let cancel_id = format!("cancel{ctx_suffix}");

    let components = vec![
        serenity::CreateActionRow::Buttons(vec![
            serenity::CreateButton::new(sell_id.clone())
                .label("Sell")
                .style(poise::serenity_prelude::ButtonStyle::Success)
                .disabled(false),
            serenity::CreateButton::new(cancel_id.clone())
                .label("Cancel")
                .style(poise::serenity_prelude::ButtonStyle::Danger)
                .disabled(false)
        ])
    ];

    let ui = ctx.send(CreateReply::default()
        .content(format!("Are you sure you want to do the following? This prompt will expire <t:{die_unix}:R>."))
        .embed(CreateEmbed::new()
            .description(format!("Sell {amount} {item} for {coins_per} Coin(s) each (totalling {total})?"))
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
            .timeout(timeout)
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
            x if x == &sell_id => {
                // Place the order
                match ctx.data().write().await.run_action(Action::SellOrder { player: player_id(ctx.author()), asset: item, count: amount, coins_per }).await {
                    Ok(id) => {
                        mci.create_response(ctx, serenity::CreateInteractionResponse::UpdateMessage(CreateInteractionResponseMessage::new()
                            .components(Vec::new())
                            .content(format!("Your sell order of the following (ID no. {id}) has been sent:"))
                            .ephemeral(true)
                        )).await?;
                    },
                    Err(e) => {
                        mci.create_response(ctx, serenity::CreateInteractionResponse::Message(CreateInteractionResponseMessage::new()
                            .content(format!("Order failed: {e}"))
                            .ephemeral(true)
                        )).await?;
                    }
                }
                return Ok(())
            },
            _ => ()
        }
    }
}

#[poise::command(slash_command, ephemeral)]
async fn price(ctx: Context<'_>,
    #[description = "The item you want to check the price for"] 
    item: String
) -> Result<(), Error> {
    Ok(())
}
/// Cancels an order
#[poise::command(slash_command, ephemeral)]
async fn cancel(ctx: Context<'_>,
    #[description = "The id for the order"] 
    id: u64
) -> Result<(), Error> {
    let Some(order) = ctx.data().read().await.state.get_order(id)
    else {
        ctx.reply("No such order").await?;
        return Ok(());
    };
    if order.player.is_some_and(|x| x == player_id(ctx.author())) {
        ctx.reply("This is not your order. Recheck the id?").await?;
        return Ok(());
    }
    ctx.data().write().await.run_action(Action::CancelOrder { target_id: id }).await?;
    ctx.reply("Order cancelled").await?;
    Ok(())
}

#[poise::command(slash_command, ephemeral)]
async fn pending(ctx: Context<'_>) -> Result<(), Error> {
    let ctx_id = ctx.id();
    let ctx_suffix = format!("_{ctx_id}");
    let prev_button_id = format!("prev{ctx_suffix}");
    let next_button_id = format!("next{ctx_suffix}");
    let cancel_button_id = format!("cancel{ctx_suffix}");
    let refresh_button_id = format!("refresh{ctx_suffix}");

    let components = serenity::CreateActionRow::Buttons(vec![
        serenity::CreateButton::new(&prev_button_id).emoji('◀'),
        serenity::CreateButton::new(&cancel_button_id).label("Cancel").style(serenity::ButtonStyle::Danger),
        serenity::CreateButton::new(&refresh_button_id).label("Refresh").style(serenity::ButtonStyle::Primary),
        serenity::CreateButton::new(&next_button_id).emoji('▶'),
    ]);

    let mut curr_id = u64::MAX;
    let ui = ctx.reply("Loading orders").await?;
    loop {
        let prev_id;
        let next_id;
        let order;

        let data = ctx.data().read().await;
        let mut orders = data.state.get_orders();
        let user = player_id(ctx.author());
        orders.retain(|_, x| x.player.as_ref().is_some_and(|player| player == &user));

        // Recheck what the nearest id is, and get the ones either side while we're at it
        ((prev_id, curr_id, next_id), order) = {
            let mut lower_range = orders.range(..curr_id).rev();
            let mut upper_range = orders.range(curr_id..);

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
                    // All orders have completed, we have nothing left
                    ui.edit(ctx, CreateReply::default().content("No orders left").components(Vec::new())).await?;
                    return Ok(());
                }
            }
        };

        ui.edit(ctx, CreateReply::default()
            .content("")
            .embed(CreateEmbed::new()
                .field("ID", order.id.to_string(), true)
                .field("Type", format!("{}", order.order_type), true)
                .field("Item", order.asset.clone(), true)
                .field("Remaining", order.amount_remaining.to_string(), true)
                .field("Coins per item", order.coins_per.to_string(), true)
            )
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
            x if x == &cancel_button_id => {
                mci.create_response(ctx, serenity::CreateInteractionResponse::Acknowledge).await?;
                // Since the IDs are unique, there's no way a user could have got here without owning the order
                ctx.data().write().await.run_action(Action::CancelOrder { target_id: curr_id }).await?;
            }
            x if x == &refresh_button_id => { mci.create_response(ctx, serenity::CreateInteractionResponse::Acknowledge).await?; },
            _ => ()
        }
    }
}
