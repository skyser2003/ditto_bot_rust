use anyhow::anyhow;
use std::sync::RwLock;

use crate::{
    slack::protocol::{
        BlockElement, ConversationReplyResponse, EditMessageResponse, PostMessageResponse,
    },
    Message,
};

pub enum MockMessage {
    Blocks(Vec<BlockElement>),
    Text(String),
}

impl<'a> From<Message<'a>> for MockMessage {
    fn from(msg: Message<'a>) -> Self {
        match msg {
            Message::Blocks(blocks) => MockMessage::Blocks(blocks.iter().cloned().collect()),
            Message::Text(text) => MockMessage::Text(text.to_string()),
        }
    }
}

#[derive(Default)]
pub struct MockBot {
    messages: RwLock<Vec<(String, MockMessage)>>,
}

impl MockBot {
    pub fn dump_messages(&self) -> anyhow::Result<Vec<(String, MockMessage)>> {
        let mut messages = self
            .messages
            .write()
            .map_err(|e| anyhow!("write lock failed - {}", e))?;
        let mut ret = Vec::new();
        std::mem::swap(messages.as_mut(), &mut ret);

        Ok(ret)
    }
}

#[async_trait::async_trait]
impl super::Bot for MockBot {
    fn bot_id(&self) -> &str {
        ""
    }

    fn bot_token(&self) -> &str {
        ""
    }

    fn openai_key(&self) -> &str {
        ""
    }

    fn gemini_key(&self) -> &str {
        ""
    }

    async fn send_message(
        &self,
        channel: &str,
        message: Message<'_>,
        reply: Option<ReplyMessageEvent>,
        unfurl_links: Option<bool>,
    ) -> anyhow::Result<PostMessageResponse> {
        let mut messages = self
            .messages
            .write()
            .map_err(|e| anyhow!("write lock failed - {}", e))?;

        eprintln!(
            "{}",
            serde_json::to_string_pretty(&message.as_postmessage(channel, reply, unfurl_links))?
        );

        messages.push((channel.to_string(), message.into()));

        Ok(PostMessageResponse {
            ok: true,
            channel: None,
            error: None,
            ts: None,
        })
    }

    async fn edit_message(
        &self,
        channel: &str,
        message: Message<'_>,
        ts: &str,
    ) -> anyhow::Result<EditMessageResponse> {
        let mut messages = self
            .messages
            .write()
            .map_err(|e| anyhow!("write lock failed - {}", e))?;

        eprintln!(
            "{} {}",
            serde_json::to_string_pretty(&message.as_postmessage(channel, None, Some(false)))?,
            ts
        );

        messages.push((channel.to_string(), message.into()));

        Ok(EditMessageResponse {
            ok: true,
            channel: None,
            error: None,
            ts: None,
        })
    }

    async fn get_conversation_relies(
        &self,
        _channel: &str,
        _ts: &str,
    ) -> anyhow::Result<ConversationReplyResponse> {
        Err(anyhow!("Not implemented!"))
    }

    fn redis(&self) -> redis::Connection {
        todo!()
    }
}
