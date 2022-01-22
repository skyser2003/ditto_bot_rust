use crate::Message;

pub async fn handle<'a, B: crate::Bot>(bot: &B, msg: &crate::MessageEvent) -> anyhow::Result<()> {
    if msg.text != "ㅍㅎ" {
        return Ok(());
    }

    bot.send_message(&msg.channel, Message::Text(":angdev:ㅊㅎ"))
        .await?;

    Ok(())
}

#[tokio::test]
#[cfg(test)]
async fn test_ph() -> anyhow::Result<()> {
    use crate::{test::MockMessage, MessageEvent};

    let bot: crate::test::MockBot = Default::default();

    handle(
        &bot,
        &MessageEvent {
            user: "".to_string(),
            channel: "".to_string(),
            text: "".to_string(),
            link: None,
        },
    )
    .await?;

    assert!(bot.dump_messages()?.is_empty());

    handle(
        &bot,
        &MessageEvent {
            user: "".to_string(),
            channel: "".to_string(),
            text: "ㅍㅎ".to_string(),
            link: None,
        },
    )
    .await?;

    let messages = bot.dump_messages()?;
    assert_eq!(messages.len(), 1);
    if let MockMessage::Text(text) = &messages[0].1 {
        assert_eq!(text, ":angdev:ㅊㅎ");
    } else {
        panic!("Wrong response");
    }

    Ok(())
}
