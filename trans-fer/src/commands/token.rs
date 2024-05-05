use poise::{serenity_prelude::{self as serenity, CreateInteractionResponseMessage}, CreateReply};
use tpex_api::TokenPostArgs;

use crate::commands::player_id;

use super::{Context, Error};
use crate::commands::banker;

#[poise::command(slash_command, ephemeral, subcommands("create"), check = banker::check)]
pub async fn token(_ctx: Context<'_>) -> Result<(), Error> { panic!("order metacommand called!"); }

#[derive(poise::ChoiceParameter)]
enum TokenLevel {
    #[name = "read only"]
    // #[description = "This token can only be used to get information"]
    ReadOnly,
    #[name = "impersonate"]
    // #[description = "This token can be used to impersonate you"]
    ProxyOne,
    #[name = "banker"]
    // #[description = "This token can control the whole of TPEx"]
    ProxyAll,
}
impl From<TokenLevel> for tpex_api::TokenLevel {
    fn from(val: TokenLevel) -> Self {
        match val {
            TokenLevel::ReadOnly => tpex_api::TokenLevel::ReadOnly,
            TokenLevel::ProxyOne => tpex_api::TokenLevel::ProxyOne,
            TokenLevel::ProxyAll => tpex_api::TokenLevel::ProxyAll,
        }
    }
}

/// Create an API token
#[poise::command(slash_command, ephemeral, check = banker::check)]
async fn create(ctx: Context<'_>,
    #[description = "The type of token you wish to create"]
    level: TokenLevel
) -> Result<(), Error> {
    const LIFETIME: std::time::Duration = std::time::Duration::from_secs(5 * 60);
    let die_time = (ctx.created_at().naive_utc() + LIFETIME).and_utc();

    let confirm_msg = match level {
        TokenLevel::ReadOnly => "Are you sure you want to do this? This token can be used to get any information about TPEx transactions.",
        TokenLevel::ProxyOne => "Are you sure you want to do this? This token can be used to trade on your behalf, completely impersonating you.",
        TokenLevel::ProxyAll => "Are you sure you want to do this? This token can be used to control the entire bank.",
    };

    let ctx_id = ctx.id();
    let ctx_suffix = format!("_{ctx_id}");
    let confirm_id = format!("confirm{ctx_suffix}");
    let cancel_id = format!("cancel{ctx_suffix}");

    let components = vec![
        serenity::CreateActionRow::Buttons(vec![
            serenity::CreateButton::new(confirm_id.clone())
                .label("Confirm")
                .style(poise::serenity_prelude::ButtonStyle::Success)
                .disabled(false),
            serenity::CreateButton::new(cancel_id.clone())
                .label("Cancel")
                .style(poise::serenity_prelude::ButtonStyle::Danger)
                .disabled(false)
        ])
    ];
    let ui = ctx.send(CreateReply::default()
        .content(confirm_msg)
        .components(components)
        .ephemeral(true)
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
            x if x == &confirm_id => {
                // Create the token
                let args = TokenPostArgs{level: level.into(), user: player_id(ctx.author())};

                match ctx.data().remote.create_token(&args).await {
                    Ok(token) => {
                        mci.create_response(ctx, serenity::CreateInteractionResponse::UpdateMessage(CreateInteractionResponseMessage::new()
                            .components(Vec::new())
                            .content(format!("Token: ||{token}||"))
                            .ephemeral(true)
                        )).await?;
                    },
                    Err(e) => {
                        mci.create_response(ctx, serenity::CreateInteractionResponse::Message(CreateInteractionResponseMessage::new()
                            .content(format!("Token creation failed: {e}"))
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

/// Delete an API token
#[poise::command(slash_command, ephemeral, check = banker::check)]
async fn delete(ctx: Context<'_>,
    #[description = "The value of the token you want to delete"]
    token: String
) -> Result<(), Error> {
    match ctx.data().remote.delete_token(&tpex_api::TokenDeleteArgs { token: Some(token.parse()?) }).await {
        Ok(()) => ctx.reply("Token deleted").await,
        Err(e) => ctx.reply(format!("Unable to delete token: {e}")).await,
    }?;
    Ok(())
}
