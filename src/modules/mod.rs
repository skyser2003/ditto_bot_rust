pub mod chatgpt;
pub mod gemini;
pub mod mhw;
pub mod namuwiki;
pub mod surplus;
pub mod twitter;

pub async fn invoke_all_modules<B: super::Bot>(bot: &B, message: crate::MessageEvent) {
    macro_rules! invoke_modules {
        (($bot:ident, $msg:ident) => [$($(#[cfg($meta:meta)])? $module:path),*]) => {
            tokio::join!($(invoke_modules!(@mod $($meta,)? $bot, $msg, $module)),*)
        };
        (@mod $meta:meta, $bot:ident, $msg:ident, $module:path) => {{
            #[cfg($meta)]
            let m = $module($bot, &$msg);
            #[cfg(not($meta))]
            let m = futures::future::ok::<(), anyhow::Error>(());
            invoke_modules!(@log_error $module => m)
        }};
        (@mod $bot:ident, $msg:ident, $module:path) => {
            invoke_modules!(@log_error $module => $module($bot, &$msg))
        };
        (@log_error $module:path => $($body:tt)+) => {
            futures::TryFutureExt::unwrap_or_else($($body)+, |e| {
                log::error!("Module {} returned error - {}", stringify!($module), e);
            })
        };
    }

    invoke_modules!(
        (bot, message) => [
            surplus::handle,
            mhw::handle,
            namuwiki::handle,
            chatgpt::handle,
            twitter::handle,
            gemini::handle
        ]
    );
}
