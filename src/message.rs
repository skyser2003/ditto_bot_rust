// service neutral message

use serenity::model::id as discord_id;

pub enum User {
    Slack(String),
    Discord(discord_id::UserId),
}

pub enum MessageSource {
    SlackChannel(String),
    SlackThread { channel: String, thread_ts: String },
    DiscordChannel(discord_id::ChannelId),
}

pub enum MessageId {
    SlackMessage(String),
    DiscordMessage(discord_id::MessageId),
}

pub struct MessageEvent {
    pub is_bot: bool,
    pub user: User,
    pub source: MessageSource,
    pub id: MessageId,
    pub text: String,
    pub link: Option<String>,
}

pub struct Message {
    pub text: Option<String>,
    pub components: Vec<Component>,
}

impl<'a> From<&'a str> for Message {
    fn from(value: &'a str) -> Self {
        Message {
            text: Some(value.to_string()),
            components: vec![],
        }
    }
}

pub enum Component {
    Button(Button),
}

pub struct Button {
    pub label: String,
    pub style: ButtonStyle,
    pub action: ButtonAction,
    pub disabled: bool,
}

pub enum ButtonStyle {
    Primary,
    Danger,
}

pub enum ButtonAction {
    Link(String),
    IteractionId(String),
}

pub enum MessageTarget {
    SlackChannel(String),
    SlackThread { channel: String, thread_ts: String, broadcast: bool, },
    SlackEditMessage {
        channel: String,
        ts: String,
    },
    DiscordChannel(discord_id::ChannelId),
}

impl From<&MessageSource> for MessageTarget {
    fn from(value: &MessageSource) -> Self {
        match value {
            MessageSource::SlackChannel(channel) => MessageTarget::SlackChannel(channel.clone()),
            MessageSource::SlackThread { channel, thread_ts } => MessageTarget::SlackThread {
                channel: channel.clone(),
                thread_ts: thread_ts.clone(),
                broadcast: true,
            },
            MessageSource::DiscordChannel(channel_id) => {
                MessageTarget::DiscordChannel(channel_id.clone())
            }
        }
    }
}
