use crate::{slack, MessageEvent};
use lazy_static::lazy_static;
use rand::{thread_rng, Rng};

struct MonsterHunterData<'a> {
    keywords: Vec<&'a str>,
    text: &'a str,
    image_url: &'a str,
}

lazy_static! {
    static ref MHW_DATA: Vec<MonsterHunterData<'static>> = vec![
        MonsterHunterData {
            keywords: vec!["ㄷㄷ", "ㄷㄷ가마루", "도도가마루"],
            text: "도도가마루",
            image_url:
                "https://raw.githubusercontent.com/skyser2003/ditto_bot_rust/master/images/Dodogama.png"
        },
        MonsterHunterData {
            keywords: vec!["ㅊㅊ", "추천"],
            text: "치치야크",
            image_url:
                "https://raw.githubusercontent.com/skyser2003/ditto_bot_rust/master/images/Tzitzi_Ya_Ku.png"
        },
        MonsterHunterData {
            keywords: vec!["ㅈㄹ", "지랄"],
            text: "조라마그다라오스",
            image_url:
                "https://raw.githubusercontent.com/skyser2003/ditto_bot_rust/master/images/Zorah_Magdaros.png"
        },
        MonsterHunterData {
            keywords: vec!["ㄹㅇ", "리얼"],
            text: "로아루드로스",
            image_url:
                "https://raw.githubusercontent.com/skyser2003/ditto_bot_rust/master/images/Royal_Ludroth.png"
        },
        MonsterHunterData {
            keywords: vec!["ㅇㄷ"],
            text: "오도가론",
            image_url:
                "https://raw.githubusercontent.com/skyser2003/ditto_bot_rust/master/images/Odogaron.png"
        },
        MonsterHunterData {
            keywords: vec!["이불", "졸려", "잘래", "잠와", "이블조"],
            text: "이블조",
            image_url:
                "https://raw.githubusercontent.com/skyser2003/ditto_bot_rust/master/images/Evil_Jaw.png"
        },
    ];
}

pub fn handle(msg: &MessageEvent, blocks: &mut Vec<slack::BlockElement>) {
    // TODO: Remove hard coded value
    if thread_rng().gen_range(0, 100) < 35 {
        for data in &*MHW_DATA {
            for keyword in &data.keywords {
                if msg.text.contains(keyword) {
                    blocks.push(slack::BlockElement::Image(slack::ImageBlock {
                        ty: "image",
                        image_url: data.image_url,
                        alt_text: data.text,
                        title: None,
                        block_id: None,
                    }));
                }
            }
        }
    }
}
