use crate::slack;
use crate::Message;
use rand::{thread_rng, Rng};

struct MonsterHunterData<'a> {
    keywords: &'a [&'a str],
    text: &'a str,
    image_url: &'a str,
}

macro_rules! url_prefix {
    () => {
        "https://raw.githubusercontent.com/skyser2003/ditto_bot_rust/master/images/"
    };
}

const MHW_DATA: &[MonsterHunterData<'static>] = &[
    MonsterHunterData {
        keywords: &["ㄷㄷ", "ㄷㄷ가마루", "도도가마루"],
        text: "도도가마루",
        image_url: concat!(url_prefix!(), "Dodogama.png"),
    },
    MonsterHunterData {
        keywords: &["ㅊㅊ", "추천"],
        text: "치치야크",
        image_url: concat!(url_prefix!(), "Tzitzi_Ya_Ku.png"),
    },
    MonsterHunterData {
        keywords: &["ㅈㄹ", "지랄"],
        text: "조라마그다라오스",
        image_url: concat!(url_prefix!(), "Zorah_Magdaros.png"),
    },
    MonsterHunterData {
        keywords: &["ㄹㅇ", "리얼"],
        text: "로아루드로스",
        image_url: concat!(url_prefix!(), "Royal_Ludroth.png"),
    },
    MonsterHunterData {
        keywords: &["ㅇㄷ"],
        text: "오도가론",
        image_url: concat!(url_prefix!(), "Odogaron.png"),
    },
    MonsterHunterData {
        keywords: &["이불", "졸려", "잘래", "잠와", "이블조"],
        text: "이블조",
        image_url: concat!(url_prefix!(), "Evil_Jaw.png"),
    },
];

pub async fn handle<B: crate::Bot>(bot: &B, msg: &crate::MessageEvent) -> anyhow::Result<()> {
    // TODO: Remove hard coded value
    if thread_rng().gen_range(0..100) < 35 {
        for data in MHW_DATA {
            for keyword in data.keywords {
                if msg.text.contains(keyword) {
                    bot.send_message(
                        &msg.channel,
                        Message::Blocks(&[slack::BlockElement::Image(slack::ImageBlock {
                            ty: "image".to_string(),
                            image_url: data.image_url.to_string(),
                            alt_text: data.text.to_string(),
                            title: None,
                            block_id: None,
                        })]),
                        None,
                    )
                    .await?;
                }
            }
        }
    }

    Ok(())
}
