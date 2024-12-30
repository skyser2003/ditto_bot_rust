use anyhow::Context as _;
use log::{error, info};

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
#[cfg(feature = "discord")]
mod discord;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    env_logger::init();

    let mut config: toml::Table = {
        let config_str = tokio::fs::read_to_string("config.toml")
            .await
            .context("Failed to open or read config.toml")?;
        toml::from_str(&config_str).context("Failed to parse config as TOML")?
    };
    info!("Config loaded");

    #[allow(unused_variables)]
    let (stop_sender, stop_receiver) = tokio::sync::watch::channel(false);

    #[cfg(feature = "slack")]
    let slack = {
        let stop_sender = stop_sender.clone();
        let stop_receiver = stop_receiver.clone();
        let config = config
            .remove("slack")
            .ok_or_else(|| anyhow::anyhow!("slack doesn't exist in config"))?;
        tokio::spawn(async move {
            if let Err(e) = slack::run(stop_receiver.clone()) {
                error!("slack stopped by error - {e:?}");
            }
            let _ = stop_sender.send(true);
        })
    };
    #[cfg(not(feature = "slack"))]
    let slack = async { Result::<(), anyhow::Error>::Ok(()) };
    #[cfg(feature = "discord")]
    let discord = {
        let stop_sender = stop_sender.clone();
        let stop_receiver = stop_receiver.clone();
        let config = config
            .remove("discord")
            .ok_or_else(|| anyhow::anyhow!("discord doesn't exist in config"))?;
        tokio::spawn(async move {
            if let Err(e) = discord::run(config, stop_receiver.clone()).await {
                error!("discord stopped by error - {e:?}");
            }
            let _ = stop_sender.send(true);
        })
    };
    #[cfg(not(feature = "discord"))]
    let discord = async { Result::<(), anyhow::Error>::Ok(()) };

    let stop_signal = tokio::spawn(async move {
        let mut stop_receiver = stop_receiver;
        #[cfg(target_family = "unix")]
        {
            use tokio::signal::unix::*;

            let mut sigterm = signal(SignalKind::terminate())?;
            tokio::select! {
                _ = sigterm.recv() => (),
                _ = stop_receiver.wait_for(|v| *v) => (),
            }
        };
        stop_sender.send(true)?;
        Result::<(), anyhow::Error>::Ok(())
    });

    let (slack, discord, stop_signal) = tokio::join!(slack, discord, stop_signal);

    slack?;
    discord?;
    stop_signal??;

    Ok(())
}

// curl 'https://slack.com/api/chat.postMessage' -H 'Authorization: Bearer SECRET' -H 'Content-type: application/json; charset=utf-8' -d '{"channel": "CS2AVF83X", "text": "hello, world"}'
