#[cfg(feature = "slack")]
mod slack;
// TODO: decouple slack these modules
#[cfg(feature = "slack")]
mod bot;
#[cfg(feature = "slack")]
mod message;
#[cfg(feature = "slack")]
mod modules;
#[cfg(feature = "slack")]
pub use message::*;
#[cfg(feature = "slack")]
#[cfg(test)]
pub mod test;
#[cfg(feature = "slack")]
pub use bot::*;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    env_logger::init();

    #[cfg(feature = "slack")]
    let slack = slack::run();
    #[cfg(not(feature = "slack"))]
    let slack = async { Result::<(), anyhow::Error>::Ok(()) };

    let (slack,) = tokio::join!(slack);

    slack?;

    Ok(())
}

// curl 'https://slack.com/api/chat.postMessage' -H 'Authorization: Bearer SECRET' -H 'Content-type: application/json; charset=utf-8' -d '{"channel": "CS2AVF83X", "text": "hello, world"}'
