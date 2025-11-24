use async_stream::stream;
use base64::{Engine as _, engine::general_purpose};
use bytestring::ByteString;
use chrono::Utc;
use eyre::{Context, Result};
use futures::Stream;
use hmac::{Hmac, Mac};
use reqwest::{Client, Method};
use sha2::Sha256;
use std::pin::Pin;

use crate::okx::{OKX_REST_API_BASE, model::HttpResponse};

type HmacSha256 = Hmac<Sha256>;

/// OKX API 认证信息
#[derive(Clone)]
pub struct OkxAuth {
    pub api_key: ByteString,
    pub secret_key: ByteString,
    pub passphrase: ByteString,
    pub simulated: bool, // 是否为模拟交易
}

impl OkxAuth {
    pub fn new(
        api_key: impl Into<ByteString>,
        secret_key: impl Into<ByteString>,
        passphrase: impl Into<ByteString>,
    ) -> Self {
        Self {
            api_key: api_key.into(),
            secret_key: secret_key.into(),
            passphrase: passphrase.into(),
            simulated: false,
        }
    }

    pub fn with_simulated(mut self, simulated: bool) -> Self {
        self.simulated = simulated;
        self
    }

    /// 生成签名
    fn sign(&self, timestamp: &str, method: &str, request_path: &str, body: &str) -> String {
        let prehash = format!("{}{}{}{}", timestamp, method, request_path, body);

        let mut mac = HmacSha256::new_from_slice(self.secret_key.as_bytes())
            .expect("HMAC can take key of any size");
        mac.update(prehash.as_bytes());

        let result = mac.finalize();
        general_purpose::STANDARD.encode(result.into_bytes())
    }

    /// 获取当前时间戳
    fn get_timestamp() -> String {
        Utc::now().format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string()
    }
}

/// 发送已签名的 HTTP 请求
pub(super) async fn signed_request<T: serde::de::DeserializeOwned>(
    auth: &OkxAuth,
    method: Method,
    endpoint: &str,
    body: &str,
) -> Result<T> {
    let client = Client::new();
    let timestamp = OkxAuth::get_timestamp();
    let signature = auth.sign(&timestamp, method.as_str(), endpoint, body);

    let url = format!("{}{}", OKX_REST_API_BASE, endpoint);

    let mut request_builder = client
        .request(method, &url)
        .header::<&str, &str>("OK-ACCESS-KEY", auth.api_key.as_ref())
        .header("OK-ACCESS-SIGN", signature)
        .header("OK-ACCESS-TIMESTAMP", timestamp)
        .header::<&str, &str>("OK-ACCESS-PASSPHRASE", auth.passphrase.as_ref())
        .header("Content-Type", "application/json");

    if auth.simulated {
        request_builder = request_builder.header("x-simulated-trading", "1");
    }

    if !body.is_empty() {
        request_builder = request_builder.body(body.to_string());
    }

    let response = request_builder
        .send()
        .await
        .context("Failed to send HTTP request")?;

    response.error_for_status_ref()?;

    let bytes = response
        .bytes()
        .await
        .context("Failed to read response bytes")?;
    let mut bytesmut = bytes.try_into_mut().expect("Should be unique");

    simd_json::serde::from_slice(&mut bytesmut).context("Failed to parse JSON response")
}

/// 创建一个已验证的认证流
///
/// 验证 API 凭证是否有效，返回一个包含已验证 auth 的 stream
///
/// # 示例
/// ```no_run
/// use ephemera_source::okx::{okx_verified_auth_stream, OkxAuth};
/// use futures::StreamExt;
///
/// # async fn example() -> eyre::Result<()> {
/// let auth = OkxAuth::new("api_key", "secret_key", "passphrase");
///
/// let mut auth_stream = okx_verified_auth_stream(auth);
///
/// if let Some(result) = auth_stream.next().await {
///     match result {
///         Ok(verified_auth) => {
///             println!("Auth verified successfully!");
///             // 使用 verified_auth 进行后续操作
///         }
///         Err(e) => eprintln!("Auth verification failed: {}", e),
///     }
/// }
/// # Ok(())
/// # }
/// ```
pub fn okx_verified_auth_stream(
    auth: OkxAuth,
) -> Pin<Box<dyn Stream<Item = Result<OkxAuth>> + Send>> {
    let stream = stream! {
        // 通过查询账户余额来验证凭证
        tracing::info!("Verifying OKX API credentials...");

        let response: Result<HttpResponse<simd_json::OwnedValue>> =
            signed_request(&auth, Method::GET, "/api/v5/account/balance", "").await;

        match response {
            Ok(api_resp) => {
                if api_resp.code == "0" {
                    tracing::info!("OKX API credentials verified successfully");
                    yield Ok(auth);
                } else {
                    let error = eyre::eyre!("API Error: code={}, msg={}", api_resp.code, api_resp.msg);
                    tracing::error!("Auth verification failed: {}", error);
                    yield Err(error);
                }
            }
            Err(e) => {
                tracing::error!("Auth verification failed: {}", e);
                yield Err(e);
            }
        }
    };

    Box::pin(stream)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_okx_auth_creation() {
        let auth = OkxAuth::new("test_key", "test_secret", "test_pass");
        assert_eq!(auth.api_key, "test_key");
        assert_eq!(auth.secret_key, "test_secret");
        assert_eq!(auth.passphrase, "test_pass");
        assert!(!auth.simulated);
    }

    #[test]
    fn test_okx_auth_with_simulated() {
        let auth = OkxAuth::new("test_key", "test_secret", "test_pass").with_simulated(true);
        assert!(auth.simulated);
    }
}
