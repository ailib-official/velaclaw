//! Gateway webhook channel wiring (VL-REVIEW-004).

use crate::channels::{LinqChannel, NextcloudTalkChannel, WhatsAppChannel};
use crate::config::Config;
use std::sync::Arc;

/// Optional webhook channel handles + signing secrets for [`super::AppState`].
pub struct WebhookChannels {
    pub whatsapp: Option<Arc<WhatsAppChannel>>,
    pub whatsapp_app_secret: Option<Arc<str>>,
    pub linq: Option<Arc<LinqChannel>>,
    pub linq_signing_secret: Option<Arc<str>>,
    pub nextcloud_talk: Option<Arc<NextcloudTalkChannel>>,
    pub nextcloud_talk_webhook_secret: Option<Arc<str>>,
}

fn optional_secret_from_env_or_config(
    env_key: &str,
    from_config: Option<String>,
) -> Option<Arc<str>> {
    std::env::var(env_key)
        .ok()
        .and_then(|secret| {
            let secret = secret.trim();
            (!secret.is_empty()).then(|| secret.to_owned())
        })
        .or(from_config)
        .map(Arc::from)
}

/// Build WhatsApp / Linq / Nextcloud Talk webhook adapters from config + env secrets.
pub fn init_webhook_channels(config: &Config) -> WebhookChannels {
    let whatsapp = config
        .channels_config
        .whatsapp
        .as_ref()
        .filter(|wa| wa.is_cloud_config())
        .map(|wa| {
            Arc::new(WhatsAppChannel::new(
                wa.access_token.clone().unwrap_or_default(),
                wa.phone_number_id.clone().unwrap_or_default(),
                wa.verify_token.clone().unwrap_or_default(),
                wa.allowed_numbers.clone(),
            ))
        });

    let whatsapp_app_secret = optional_secret_from_env_or_config(
        "VELACLAW_WHATSAPP_APP_SECRET",
        config.channels_config.whatsapp.as_ref().and_then(|wa| {
            wa.app_secret
                .as_deref()
                .map(str::trim)
                .filter(|secret| !secret.is_empty())
                .map(ToOwned::to_owned)
        }),
    );

    let linq = config.channels_config.linq.as_ref().map(|lq| {
        Arc::new(LinqChannel::new(
            lq.api_token.clone(),
            lq.from_phone.clone(),
            lq.allowed_senders.clone(),
        ))
    });

    let linq_signing_secret = optional_secret_from_env_or_config(
        "VELACLAW_LINQ_SIGNING_SECRET",
        config.channels_config.linq.as_ref().and_then(|lq| {
            lq.signing_secret
                .as_deref()
                .map(str::trim)
                .filter(|secret| !secret.is_empty())
                .map(ToOwned::to_owned)
        }),
    );

    let nextcloud_talk = config.channels_config.nextcloud_talk.as_ref().map(|nc| {
        Arc::new(NextcloudTalkChannel::new(
            nc.base_url.clone(),
            nc.app_token.clone(),
            nc.allowed_users.clone(),
        ))
    });

    let nextcloud_talk_webhook_secret = optional_secret_from_env_or_config(
        "VELACLAW_NEXTCLOUD_TALK_WEBHOOK_SECRET",
        config
            .channels_config
            .nextcloud_talk
            .as_ref()
            .and_then(|nc| {
                nc.webhook_secret
                    .as_deref()
                    .map(str::trim)
                    .filter(|secret| !secret.is_empty())
                    .map(ToOwned::to_owned)
            }),
    );

    WebhookChannels {
        whatsapp,
        whatsapp_app_secret,
        linq,
        linq_signing_secret,
        nextcloud_talk,
        nextcloud_talk_webhook_secret,
    }
}
