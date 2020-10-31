use crate::slack;
use log::debug;
use redis::{Commands, Connection};
use std::borrow::Cow;
use std::time::{SystemTime, UNIX_EPOCH};

pub async fn handle<'a>(
    text: &String,
    user: &String,
    blocks: &mut Vec<slack::BlockElement<'a>>,
    conn: &mut Connection,
    bot_id: &String,
) -> redis::RedisResult<()> {
    let slices = text.split_whitespace().collect::<Vec<&str>>();
    let slack_bot_format = format!("<@{}>", bot_id);

    if 2 <= slices.len() && slices[0] == slack_bot_format {
        let call_type = slices[1];

        if call_type == "잉여" {
            let mut table = std::collections::HashMap::<String, i32>::new();

            let records: Vec<String> = conn.zrangebyscore("ditto-archive", "-inf", "+inf").unwrap();

            if records.len() == 0 {
                blocks.push(slack::BlockElement::Section(slack::SectionBlock {
                    text: slack::TextObject {
                        ty: slack::TextObjectType::PlainText,
                        text: Cow::from("[There is no chat record.]"),
                        emoji: None,
                        verbatim: None,
                    },
                    block_id: None,
                    fields: None,
                }));

                return Ok(());
            }

            for record in records {
                let user_id = record.split(":").nth(1).unwrap().to_string();

                let prev_count = table.get(&user_id);
                let next_count = match prev_count {
                    Some(val) => val + 1,
                    None => 1,
                };

                table.insert(user_id, next_count);
            }

            let mut vec_table = Vec::<(String, i32)>::new();

            for pair in table.iter_mut() {
                vec_table.push((pair.0.to_string(), *pair.1));
            }

            vec_table.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());

            if slices.len() <= 2 || slices[2] != "all" {
                // Top 5 user's chat count only
                vec_table.truncate(5);
            }

            blocks.push(slack::BlockElement::Section(slack::SectionBlock {
                text: slack::TextObject {
                    ty: slack::TextObjectType::PlainText,
                    text: Cow::from("[You guys are all ingyeo.]"),
                    emoji: None,
                    verbatim: None,
                },
                block_id: None,
                fields: None,
            }));

            debug!("Blocks: {:?}", blocks);
        }
    } else {
        let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap();
        let score = now.as_millis();
        let member = format!("{}:{}", score, user);

        conn.zadd("ditto-archive", member, score as i64)?;
    }

    Ok(())
}
