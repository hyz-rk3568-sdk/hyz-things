//! Forward temporary browser images to the LAN-restricted PC-side receiver.
use crate::application::question_bank::practice::AiAttachmentRelay;
use serde::Deserialize;
use std::time::Duration;
use url::Url;

const UPLOAD_TIMEOUT: Duration = Duration::from_secs(20);
const DEFAULT_RECEIVER_URL: &str = "http://192.168.8.202:8099";

#[derive(Clone)]
pub struct HttpAttachmentRelay {
    client: reqwest::Client,
    url: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct UploadResponse {
    path: String,
}

/// The receiver address is fixed so no browser, AI output or `.env` value can
/// redirect image uploads.
pub fn load_attachment_relay() -> Result<Option<HttpAttachmentRelay>, &'static str> {
    let url = Url::parse(DEFAULT_RECEIVER_URL)
        .map_err(|_| "fixed AI upload URL is invalid")?
        .to_string();
    let client = reqwest::Client::builder()
        .timeout(UPLOAD_TIMEOUT)
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|_| "AI upload HTTP client unavailable")?;
    Ok(Some(HttpAttachmentRelay { client, url }))
}

#[async_trait::async_trait]
impl AiAttachmentRelay for HttpAttachmentRelay {
    async fn upload(&self, media_type: &str, bytes: &[u8]) -> Result<String, String> {
        let response = self
            .client
            .post(format!("{}/v1/attachments", self.url))
            .header(reqwest::header::CONTENT_TYPE, media_type)
            .body(bytes.to_vec())
            .send()
            .await
            .map_err(|_| "电脑临时图片接收端不可用".to_owned())?;
        if !response.status().is_success() {
            return Err("电脑临时图片接收端拒绝了图片".into());
        }
        response
            .json::<UploadResponse>()
            .await
            .map(|body| body.path)
            .map_err(|_| "电脑临时图片接收端响应无效".to_owned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uses_the_fixed_receiver_address() {
        assert_eq!(DEFAULT_RECEIVER_URL, "http://192.168.8.202:8099");
        assert!(load_attachment_relay().unwrap().is_some());
    }
}
