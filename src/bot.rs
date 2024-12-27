use super::message::{Message, MessageEvent, ReplyMessageEvent};
use super::slack::{ConversationReplyResponse, EditMessageResponse, PostMessageResponse};
use anyhow::{anyhow, Context as _};
use async_trait::async_trait;
use log::debug;

#[async_trait]
pub trait Bot {
    fn bot_id(&self) -> &'_ str;
    fn bot_token(&self) -> &'_ str;
    fn openai_key(&self) -> &'_ str;
    fn gemini_key(&self) -> &'_ str;

    async fn send_message(
        &self,
        channel: &str,
        msg: Message<'_>,
        reply: Option<ReplyMessageEvent>,
        unfurl_links: Option<bool>,
    ) -> anyhow::Result<PostMessageResponse>;

    async fn edit_message(
        &self,
        channel: &str,
        msg: Message<'_>,
        ts: &str,
    ) -> anyhow::Result<EditMessageResponse>;

    async fn get_conversation_relies(
        &self,
        channel: &str,
        ts: &str,
    ) -> anyhow::Result<ConversationReplyResponse>;
    fn redis(&self) -> redis::Connection;
}

pub struct DittoBot {
    pub bot_id: String,
    pub bot_token: String,
    pub openai_key: String,
    pub gemini_key: String,
    pub http_client: reqwest::Client,
    pub redis_client: redis::Client,
}

#[async_trait]
impl Bot for DittoBot {
    fn bot_id(&self) -> &'_ str {
        &self.bot_id
    }

    fn bot_token(&self) -> &'_ str {
        &self.bot_token
    }

    fn openai_key(&self) -> &'_ str {
        &self.openai_key
    }

    fn gemini_key(&self) -> &'_ str {
        &self.gemini_key
    }

    async fn send_message(
        &self,
        channel: &str,
        message: Message<'_>,
        reply: Option<ReplyMessageEvent>,
        unfurl_links: Option<bool>,
    ) -> anyhow::Result<PostMessageResponse> {
        let builder = self
            .http_client
            .post("https://slack.com/api/chat.postMessage")
            .header("Content-type", "application/json; charset=utf-8")
            .header("Authorization", format!("Bearer {}", &self.bot_token));

        let reply = message.as_postmessage(channel, reply, unfurl_links);

        let resp = builder
            .json(&reply)
            .send()
            .await
            .context("Failed to send request")?;

        let resp = resp
            .json::<PostMessageResponse>()
            .await
            .context("Failed to parse response")?;

        Ok(resp)
    }

    async fn edit_message(
        &self,
        channel: &str,
        message: Message<'_>,
        ts: &str,
    ) -> anyhow::Result<EditMessageResponse> {
        let builder = self
            .http_client
            .post("https://slack.com/api/chat.update")
            .header("Content-type", "application/json; charset=utf-8")
            .header("Authorization", format!("Bearer {}", &self.bot_token));

        let body = message.as_editmessage(channel, ts);

        let resp = builder
            .json(&body)
            .send()
            .await
            .context("Failed to send request")?;

        let resp = resp
            .json::<EditMessageResponse>()
            .await
            .context("Failed to parse response")?;

        Ok(resp)
    }

    async fn get_conversation_relies(
        &self,
        channel: &str,
        ts: &str,
    ) -> anyhow::Result<ConversationReplyResponse> {
        let builder = self
            .http_client
            .get("https://slack.com/api/conversations.replies")
            .header("Content-type", "application/json; charset=utf-8")
            .header("Authorization", format!("Bearer {}", &self.bot_token))
            .query(&[("channel", channel), ("ts", ts)]);

        let res = builder.send().await.context("Failed to send request")?;

        let body = res.text().await?;

        let json_result = serde_json::from_str::<ConversationReplyResponse>(&body);

        if json_result.is_ok() {
            Ok(json_result.unwrap())
        } else {
            Err(anyhow!(
                "Json parsing failed for conversations.replies: {:?} {}",
                json_result.err(),
                body
            ))
        }
    }

    fn redis(&self) -> redis::Connection {
        self.redis_client
            .get_connection()
            .unwrap_or_else(|_| unsafe { std::hint::unreachable_unchecked() })
    }
}

impl DittoBot {
    pub async fn slack_event_handler(&self, msg: MessageEvent) -> anyhow::Result<()> {
        if msg.is_bot || msg.user.contains(&self.bot_id) {
            debug!("Ignoring bot message");
            return Ok(());
        }

        crate::modules::invoke_all_modules(self, msg).await;

        Ok(())
    }
}
