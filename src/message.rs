use crate::slack;
use log::error;

pub struct MessageEvent {
    pub is_bot: bool,
    pub user: String,
    pub channel: String,
    pub text: String,
    pub ts: String,
    pub thread_ts: Option<String>,
    pub link: Option<String>,
}

#[derive(Clone)]
pub struct ReplyMessageEvent {
    pub msg: String,
    pub broadcast: bool,
}

#[derive(Debug, thiserror::Error)]
pub enum ConvertMessageEventError {
    #[error("Invalid message type")]
    InvalidMessageType,
}

pub enum Message<'a> {
    Blocks(&'a [slack::BlockElement]),
    Text(&'a str),
}
