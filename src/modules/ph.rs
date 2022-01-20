use crate::slack::BlockElement;

pub async fn handle<'a, B: crate::Bot>(bot: &B, msg: &crate::MessageEvent) -> anyhow::Result<()> {
    if msg.text != "ㅍㅎ" {
        return Ok(());
    }

    bot.send_message(
        &msg.channel,
        &[BlockElement::Text {
            text: ":angdev:ㅊㅎ".to_string(),
        }],
    )
    .await?;

    Ok(())
}
