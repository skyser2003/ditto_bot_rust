use anyhow::anyhow;
use std::sync::RwLock;

#[derive(Default)]
pub struct MockBot {
    messages: RwLock<Vec<(String, Vec<super::slack::BlockElement>)>>,
}

impl MockBot {
    pub fn dump_messages(&self) -> anyhow::Result<Vec<(String, Vec<super::slack::BlockElement>)>> {
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

    async fn send_message(
        &self,
        channel: &str,
        blocks: &[super::slack::BlockElement],
    ) -> anyhow::Result<()> {
        let mut messages = self
            .messages
            .write()
            .map_err(|e| anyhow!("write lock failed - {}", e))?;
        messages.push((channel.to_string(), blocks.iter().cloned().collect()));

        Ok(())
    }

    fn redis(&self) -> redis::Connection {
        todo!()
    }
}
