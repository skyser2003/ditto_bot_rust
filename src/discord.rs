use log::error;

pub async fn run() {
    let (stop_sender, _) = tokio::sync::broadcast::channel(1);

    let discord_join = tokio::task::spawn({
        let db_pool = db_pool.clone();
        let stop_receiver = stop_sender.subscribe();
        let stop_sender = stop_sender.clone();
        let config = config.clone();
        async move {
            type BoxedHandler = Box<dyn discord::SubApplication + Send + Sync>;
            if let Err(e) = discord::start(
                &config,
                IntoIterator::into_iter([
                    Box::new(eueoeo::DiscordHandler::new(db_pool.clone(), &config).await)
                        as BoxedHandler,
                    Box::new(
                        events::DiscordHandler::new(db_pool.clone(), &config)
                            .await
                            .unwrap(),
                    ) as BoxedHandler,
                    Box::new(
                        user::DiscordHandler::new(db_pool.clone(), &config)
                            .await
                            .unwrap(),
                    ) as BoxedHandler,
                    Box::new(link_rewriter::DiscordHandler::new()) as BoxedHandler,
                    Box::new(
                        llm::DiscordHandler::new(db_pool.clone(), &config)
                            .await
                            .unwrap(),
                    ) as BoxedHandler,
                ])
                .collect(),
                stop_receiver,
            )
            .await
            {
                error!("Discord task failed with - {e:?}");
                let _ = stop_sender.send(());
            }
        }
    });
}
