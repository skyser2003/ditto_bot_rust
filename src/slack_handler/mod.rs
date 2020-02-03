extern crate slack;

mod slack_handler;
mod client_wrapper;

pub use slack_handler::SlackHandler;
pub use client_wrapper::{SlackClientWrapper, SlackClientWrapperFunc};