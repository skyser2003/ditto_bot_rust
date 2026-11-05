use anyhow::anyhow;
use std::{
    collections::{HashMap, HashSet, VecDeque},
    sync::RwLock,
};

use crate::{
    slack::{ConversationReplyResponse, EditMessageResponse, PostMessageResponse},
    Message, ReplyMessageEvent,
};

pub enum MockMessage {
    Blocks(Vec<super::slack::BlockElement>),
    BlocksWithText {
        text: String,
        blocks: Vec<super::slack::BlockElement>,
    },
    Text(String),
}

impl<'a> From<Message<'a>> for MockMessage {
    fn from(msg: Message<'a>) -> Self {
        match msg {
            Message::BlocksWithText { text, blocks } => MockMessage::BlocksWithText {
                text: text.to_string(),
                blocks: blocks.to_vec(),
            },
            Message::Blocks(blocks) => MockMessage::Blocks(blocks.iter().cloned().collect()),
            Message::Text(text) => MockMessage::Text(text.to_string()),
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub struct MockReaction {
    pub action: &'static str,
    pub channel: String,
    pub ts: String,
    pub name: String,
}

#[derive(Default)]
pub struct MockBot {
    messages: RwLock<Vec<(String, MockMessage)>>,
    pub files: RwLock<HashMap<String, bytes::Bytes>>,
    pub forbid_tool_metadata: bool,
    pub reactions: RwLock<Vec<MockReaction>>,
    pub fail_add_reaction: bool,
    pub fail_remove_reaction: bool,
    pub post_results: RwLock<VecDeque<anyhow::Result<PostMessageResponse>>>,
    pub edit_results: RwLock<VecDeque<anyhow::Result<EditMessageResponse>>>,
    pub activity: RwLock<Vec<&'static str>>,
    pub sent_replies: RwLock<Vec<Option<ReplyMessageEvent>>>,
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
        self.activity.write().unwrap().push("send_message");
        self.sent_replies.write().unwrap().push(reply.clone());
        let mut messages = self
            .messages
            .write()
            .map_err(|e| anyhow!("write lock failed - {}", e))?;

        eprintln!(
            "{}",
            serde_json::to_string_pretty(&message.as_postmessage(channel, reply, unfurl_links))?
        );

        messages.push((channel.to_string(), message.into()));

        if let Some(result) = self.post_results.write().unwrap().pop_front() {
            return result;
        }
        Ok(PostMessageResponse {
            ok: true,
            channel: None,
            error: None,
            ts: Some(serde_json::from_value(serde_json::json!("1673464746.000001")).unwrap()),
        })
    }

    async fn edit_message(
        &self,
        channel: &str,
        message: Message<'_>,
        ts: &str,
    ) -> anyhow::Result<EditMessageResponse> {
        self.activity.write().unwrap().push("edit_message");
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

        if let Some(result) = self.edit_results.write().unwrap().pop_front() {
            return result;
        }
        Ok(EditMessageResponse {
            ok: true,
            channel: None,
            error: None,
            ts: None,
        })
    }

    async fn add_reaction(&self, channel: &str, ts: &str, name: &str) -> anyhow::Result<()> {
        self.activity.write().unwrap().push("add_reaction");
        self.reactions.write().unwrap().push(MockReaction {
            action: "add",
            channel: channel.to_string(),
            ts: ts.to_string(),
            name: name.to_string(),
        });
        if self.fail_add_reaction {
            return Err(anyhow!("Could not add reaction"));
        }
        Ok(())
    }

    async fn remove_reaction(&self, channel: &str, ts: &str, name: &str) -> anyhow::Result<()> {
        self.activity.write().unwrap().push("remove_reaction");
        self.reactions.write().unwrap().push(MockReaction {
            action: "remove",
            channel: channel.to_string(),
            ts: ts.to_string(),
            name: name.to_string(),
        });
        if self.fail_remove_reaction {
            return Err(anyhow!("Could not remove reaction"));
        }
        Ok(())
    }

    async fn get_conversation_replies(
        &self,
        _channel: &str,
        _ts: &str,
    ) -> anyhow::Result<ConversationReplyResponse> {
        Err(anyhow!("Not implemented!"))
    }

    async fn download_file(&self, file: &crate::slack::SlackFile) -> anyhow::Result<bytes::Bytes> {
        self.files
            .read()
            .map_err(|e| anyhow!("read lock failed - {}", e))?
            .get(&file.id)
            .cloned()
            .ok_or_else(|| anyhow!("File {} is unavailable", file.id))
    }

    async fn call_mcp_tool(
        &self,
        _unified_name: &str,
        _arguments: HashMap<String, serde_json::Value>,
    ) -> anyhow::Result<String> {
        Err(anyhow!("Not implemented!"))
    }

    async fn get_all_tools_metadata(
        &self,
    ) -> anyhow::Result<Vec<(String, HashMap<String, (String, String)>, HashSet<String>)>> {
        if self.forbid_tool_metadata {
            return Err(anyhow!("Chat tool metadata must not be requested"));
        }
        Ok(vec![])
    }
}

#[tokio::test]
async fn test_mcp_client1() -> anyhow::Result<()> {
    use std::borrow::Cow;

    use rmcp::{transport::TokioChildProcess, ServiceExt};
    use tokio::process::Command;

    let client = ()
        .serve(TokioChildProcess::new(
            Command::new("npx")
                .arg("-y")
                .arg("@modelcontextprotocol/server-everything"),
        )?)
        .await?;

    let tools = client.list_all_tools().await?;
    let resources = client.list_all_resources().await?;

    for tool in tools {
        println!("{:?}", tool);
    }

    for resource in resources {
        println!("{:?}", resource);
    }

    let mut echo_args = serde_json::Map::new();
    echo_args.insert(
        "message".to_string(),
        serde_json::Value::String("Hello world!".to_string()),
    );

    let res = client
        .call_tool(rmcp::model::CallToolRequestParam {
            name: Cow::Borrowed("echo"),
            arguments: Some(echo_args),
        })
        .await?;

    println!("{:?}", res);

    Ok(())
}

#[tokio::test]
async fn test_mcp_client2() -> anyhow::Result<()> {
    use std::borrow::Cow;

    use rmcp::{transport::TokioChildProcess, ServiceExt};
    use tokio::process::Command;

    let tz = "Asia/Seoul";

    let client = ()
        .serve(TokioChildProcess::new(
            Command::new("uvx")
                .arg("mcp-server-time")
                .arg("--local-timezone")
                .arg(tz),
        )?)
        .await?;

    let tools = client.list_all_tools().await?;

    for tool in tools {
        println!("{:?}", tool);
    }

    let mut echo_args = serde_json::Map::new();
    echo_args.insert(
        "timezone".to_string(),
        serde_json::Value::String(tz.to_string()),
    );

    let res = client
        .call_tool(rmcp::model::CallToolRequestParam {
            name: Cow::Borrowed("get_current_time"),
            arguments: Some(echo_args),
        })
        .await?;

    println!("{:?}", res);

    Ok(())
}
