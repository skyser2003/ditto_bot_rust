use anyhow::Context as _;
use log::{error, info};
use serenity::{
    all::{
        Context, CreateMessage, EventHandler, GatewayIntents, GuildChannel, GuildMemberUpdateEvent,
        Member, Ready,
    },
    model::id::{ApplicationId, ChannelId, RoleId},
    Client,
};

struct Handler {
    config: Config,
}

#[async_trait::async_trait]
impl EventHandler for Handler {
    async fn ready(&self, _ctx: Context, _data_about_bot: Ready) {
        info!("ready");
    }

    async fn thread_create(&self, ctx: Context, thread: GuildChannel) {
        let Some(config) = self
            .config
            .thread_notification
            .iter()
            .find(|config| Some(config.parent_id) == thread.parent_id)
        else {
            return;
        };

        let message = config.message_format.replace(
            "{link}",
            &format!(
                "https://discord.com/channels/{}/{}",
                thread.guild_id, thread.id
            ),
        );
        if let Err(e) = config
            .notification_channel_id
            .send_message(ctx, CreateMessage::new().content(message))
            .await
        {
            error!("Failed to notify member updated event - {e:?}");
        }
    }

    async fn guild_member_update(
        &self,
        ctx: Context,
        old_if_available: Option<Member>,
        new: Option<Member>,
        _event: GuildMemberUpdateEvent,
    ) {
        let Some(new) = new else {
            return;
        };
        let Some(old) = old_if_available else {
            return;
        };
        if new.nick == old.nick {
            return;
        }
        if new.nick.is_none() {
            return;
        };

        if !new.roles.contains(&self.config.accepted_role_id) {
            if let Err(e) = self
                .config
                .admin_channel_id
                .send_message(
                    ctx,
                    CreateMessage::new().content(format!(
                        "Not accepted user <@{}> changed nickname",
                        new.user.id
                    )),
                )
                .await
            {
                error!("Failed to notify member updated event - {e:?}");
            }
        }
    }
}

#[derive(serde::Deserialize)]
struct Config {
    app_id: ApplicationId,
    bot_token: String,
    admin_channel_id: ChannelId,
    accepted_role_id: RoleId,
    thread_notification: Vec<ThreadNotificationConfig>,
}

#[derive(serde::Deserialize)]
struct ThreadNotificationConfig {
    parent_id: ChannelId,
    notification_channel_id: ChannelId,
    message_format: String,
}

pub async fn run(
    config: toml::Value,
    mut stop_signal: tokio::sync::watch::Receiver<bool>,
) -> anyhow::Result<()> {
    info!("discord run");
    if *stop_signal.borrow_and_update() {
        return Ok(());
    }

    let config: Config = config
        .try_into()
        .context("Failed to parse as discord config")?;

    let mut client = Client::builder(
        &config.bot_token,
        GatewayIntents::GUILDS
            | GatewayIntents::GUILD_MEMBERS
            | GatewayIntents::GUILD_MESSAGES
            | GatewayIntents::GUILD_PRESENCES
            | GatewayIntents::MESSAGE_CONTENT
            | GatewayIntents::GUILD_SCHEDULED_EVENTS,
    )
    .application_id(config.app_id)
    .event_handler(Handler { config })
    .await?;

    let shard_manager = client.shard_manager.clone();

    // stop the bot when SIGINT occurred.
    let stop_signal = tokio::spawn(async move {
        stop_signal
            .wait_for(|v| *v)
            .await
            .context("Stop signal is broken")?;
        info!("stop discord");
        shard_manager.shutdown_all().await;
        info!("discord closed");

        Result::<(), anyhow::Error>::Ok(())
    });

    let (discord_join, stop_join) = tokio::join!(client.start(), stop_signal);

    discord_join?;
    stop_join??;

    Ok(())
}
