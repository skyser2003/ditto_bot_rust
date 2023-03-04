use anyhow::anyhow;
use std::sync::RwLock;

use crate::Message;

pub enum MockMessage {
    Blocks(Vec<super::slack::BlockElement>),
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

    async fn send_message(&self, channel: &str, message: Message<'_>) -> anyhow::Result<()> {
        let mut messages = self
            .messages
            .write()
            .map_err(|e| anyhow!("write lock failed - {}", e))?;
        eprintln!(
            "{}",
            serde_json::to_string_pretty(&message.as_postmessage(channel))?
        );
        messages.push((channel.to_string(), message.into()));

        Ok(())
    }

    fn redis(&self) -> redis::Connection {
        todo!()
    }
}
