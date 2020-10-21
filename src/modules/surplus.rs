use crate::MessageEvent;
use redis::{Client, Commands};
use std::time::{SystemTime, UNIX_EPOCH};

pub fn handle<'a>(
    msg: &'a MessageEvent,
    redis_client: &Client,
    bot_id: &String,
) -> redis::RedisResult<()> {
    let slices = msg.text.split_whitespace().collect::<Vec<&str>>();
    let slack_bot_format = format!("@<{}>", bot_id);

    if slices[1] == slack_bot_format {
        let call_type = slices[0];

        if call_type == "잉여" {
            if slices.len() == 3 && slices[2] == "all" {
                // TODO: calculate all user's chat count
            } else {
                // TODO: calculate top 5 user's chat count
            }
        }
    } else {
        let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap();
        let score = now.as_millis();
        let member = format!("{}:{}", score, msg.user);

        let mut conn = redis_client.get_connection().unwrap();
        conn.zadd("ditto-archive", member, score as i64)?;
    }

    Ok(())
}
