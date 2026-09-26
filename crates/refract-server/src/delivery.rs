//! Durable at-least-once HTTP delivery. Database commits and outbox creation are atomic.
use anyhow::{Result, anyhow, ensure};
use chrono::Utc;
use hmac::{Hmac, Mac};
use refract_storage::{OutboxJob, Store};
use reqwest::{Client, Url};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::time::Duration;

type HmacSha256 = Hmac<Sha256>;
#[derive(Clone)]
struct Webhook {
    url: Url,
    secret: Option<String>,
}
#[derive(Clone)]
struct S3 {
    endpoint: Url,
    bucket: String,
    region: String,
    access_key: String,
    secret_key: String,
}
#[derive(Clone)]
pub struct Delivery {
    client: Client,
    webhook: Option<Webhook>,
    s3: Option<S3>,
}
fn variable(name: &str) -> Result<String> {
    crate::security::secret(name)?.ok_or_else(|| anyhow!("{name} is required"))
}
fn endpoint(value: &str) -> Result<Url> {
    let url = Url::parse(value)?;
    ensure!(
        matches!(url.scheme(), "http" | "https")
            && url.host_str().is_some()
            && url.username().is_empty()
            && url.password().is_none()
            && url.fragment().is_none(),
        "delivery URL must be HTTP(S), without credentials or fragment"
    );
    Ok(url)
}
fn hex(bytes: impl AsRef<[u8]>) -> String {
    bytes.as_ref().iter().map(|b| format!("{b:02x}")).collect()
}
fn hmac(key: &[u8], data: &str) -> Vec<u8> {
    let mut mac = HmacSha256::new_from_slice(key).expect("HMAC accepts arbitrary key sizes");
    mac.update(data.as_bytes());
    mac.finalize().into_bytes().to_vec()
}
impl Delivery {
    pub fn from_env() -> Result<Self> {
        let client = Client::builder()
            .timeout(Duration::from_secs(30))
            .redirect(reqwest::redirect::Policy::none())
            .build()?;
        let webhook = std::env::var("REFRACT_WEBHOOK_URL")
            .ok()
            .map(|value| -> Result<Webhook> {
                Ok(Webhook {
                    url: endpoint(&value)?,
                    secret: crate::security::secret("REFRACT_WEBHOOK_SECRET")?,
                })
            })
            .transpose()?;
        let s3 = std::env::var("REFRACT_S3_ENDPOINT")
            .ok()
            .map(|value| -> Result<S3> {
                let endpoint = endpoint(&value)?;
                ensure!(
                    endpoint.query().is_none(),
                    "S3 endpoint must not contain a query"
                );
                let bucket = variable("REFRACT_S3_BUCKET")?;
                ensure!(
                    !bucket.is_empty()
                        && bucket
                            .bytes()
                            .all(|b| b.is_ascii_alphanumeric() || b".-".contains(&b)),
                    "invalid S3 bucket name"
                );
                Ok(S3 {
                    endpoint,
                    bucket,
                    region: variable("REFRACT_S3_REGION")?,
                    access_key: variable("REFRACT_S3_ACCESS_KEY")?,
                    secret_key: variable("REFRACT_S3_SECRET_KEY")?,
                })
            })
            .transpose()?;
        Ok(Self {
            client,
            webhook,
            s3,
        })
    }
    pub fn validate_production(&self, mode: &str) -> Result<()> {
        if mode == "production" {
            if let Some(webhook) = &self.webhook {
                ensure!(
                    webhook.url.scheme() == "https",
                    "production webhooks require HTTPS"
                );
                ensure!(
                    webhook.secret.as_ref().is_some_and(|s| s.len() >= 32),
                    "production webhooks require a signing secret of at least 32 bytes"
                );
            }
            if let Some(s3) = &self.s3 {
                ensure!(
                    s3.endpoint.scheme() == "https",
                    "production object storage requires HTTPS"
                );
            }
        }
        Ok(())
    }
    pub fn targets(&self) -> Vec<String> {
        let mut targets = vec![];
        if self.webhook.is_some() {
            targets.push("webhook".into());
        }
        if self.s3.is_some() {
            targets.push("s3".into());
        }
        targets
    }
    pub async fn process_one(&self, store: &Store) -> Result<bool> {
        let Some(job) = store.claim_outbox().await? else {
            return Ok(false);
        };
        let result = self.deliver(store, &job).await;
        match result {
            Ok(()) => store.acknowledge(&job.id).await?,
            Err(_) => {
                // Avoid logging signed URLs, payloads or client secrets in transport errors.
                store.retry(&job).await?;
                return Err(anyhow!(
                    "delivery {} to {} failed; durable retry scheduled",
                    job.id,
                    job.target
                ));
            }
        }
        Ok(true)
    }
    async fn deliver(&self, store: &Store, job: &OutboxJob) -> Result<()> {
        let payload = store
            .scoped(job.scope.clone())
            .stored_payload(&job.run_id)
            .await?;
        // Retention may have removed a run after the worker claimed a put.
        if job.operation == "put" && payload.is_none() {
            return Ok(());
        }
        match job.target.as_str() {
            "webhook" => {
                let config = self
                    .webhook
                    .as_ref()
                    .ok_or_else(|| anyhow!("webhook is no longer configured"))?;
                let body = serde_json::to_string(
                    &json!({"id":job.id,"scope":job.scope,"operation":job.operation,"run_id":job.run_id,"payload":payload}),
                )?;
                let mut request = self
                    .client
                    .post(config.url.clone())
                    .header("content-type", "application/json")
                    .header("x-refract-delivery-id", &job.id)
                    .body(body.clone());
                if let Some(secret) = &config.secret {
                    request = request.header(
                        "x-refract-signature",
                        format!("sha256={}", hex(hmac(secret.as_bytes(), &body))),
                    );
                }
                let response = request.send().await?;
                ensure!(response.status().is_success(), "webhook rejected delivery");
            }
            "s3" => {
                let config = self
                    .s3
                    .as_ref()
                    .ok_or_else(|| anyhow!("S3 is no longer configured"))?;
                let mut url = config.endpoint.clone();
                url.path_segments_mut()
                    .map_err(|_| anyhow!("S3 endpoint cannot hold paths"))?
                    .pop_if_empty()
                    .extend([
                        &config.bucket,
                        &job.scope.organization,
                        &job.scope.project,
                        &job.scope.environment,
                        &format!("{}.json", hex(Sha256::digest(job.run_id.as_bytes()))),
                    ]);
                let method = if job.operation == "delete" {
                    reqwest::Method::DELETE
                } else {
                    reqwest::Method::PUT
                };
                let body = if job.operation == "delete" {
                    String::new()
                } else {
                    payload.unwrap_or_default()
                };
                let date = Utc::now().format("%Y%m%dT%H%M%SZ").to_string();
                let payload_hash = hex(Sha256::digest(body.as_bytes()));
                let host = match url.port() {
                    Some(port) => format!("{}:{port}", url.host_str().unwrap_or_default()),
                    None => url.host_str().unwrap_or_default().into(),
                };
                let scope = format!("{}/{}/s3/aws4_request", &date[..8], config.region);
                let headers = "host;x-amz-content-sha256;x-amz-date";
                let canonical = format!(
                    "{}\n{}\n\nhost:{host}\nx-amz-content-sha256:{payload_hash}\nx-amz-date:{date}\n\n{headers}\n{payload_hash}",
                    method.as_str(),
                    url.path()
                );
                let string_to_sign = format!(
                    "AWS4-HMAC-SHA256\n{date}\n{scope}\n{}",
                    hex(Sha256::digest(canonical.as_bytes()))
                );
                let key = hmac(format!("AWS4{}", config.secret_key).as_bytes(), &date[..8]);
                let key = hmac(&key, &config.region);
                let key = hmac(&key, "s3");
                let key = hmac(&key, "aws4_request");
                let auth = format!(
                    "AWS4-HMAC-SHA256 Credential={}/{scope}, SignedHeaders={headers}, Signature={}",
                    config.access_key,
                    hex(hmac(&key, &string_to_sign))
                );
                let response = self
                    .client
                    .request(method, url)
                    .header("authorization", auth)
                    .header("x-amz-date", date)
                    .header("x-amz-content-sha256", payload_hash)
                    .header("content-type", "application/octet-stream")
                    .body(body)
                    .send()
                    .await?;
                ensure!(
                    response.status().is_success(),
                    "object storage rejected delivery"
                );
            }
            _ => return Err(anyhow!("unknown outbox target")),
        }
        Ok(())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        Router,
        extract::State,
        http::{HeaderMap, StatusCode},
        routing::post,
    };
    use std::sync::{Arc, Mutex};
    #[tokio::test]
    async fn webhook_retries_and_delivers_signed_envelope() {
        let received = Arc::new(Mutex::new(Vec::<(HeaderMap, String)>::new()));
        let app = Router::new()
            .route(
                "/events",
                post(
                    |State(state): State<Arc<Mutex<Vec<(HeaderMap, String)>>>>,
                     headers: HeaderMap,
                     body: String| async move {
                        let mut events = state.lock().unwrap();
                        events.push((headers, body));
                        if events.len() == 1 {
                            StatusCode::SERVICE_UNAVAILABLE
                        } else {
                            StatusCode::OK
                        }
                    },
                ),
            )
            .with_state(received.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let delivery = Delivery {
            client: Client::new(),
            webhook: Some(Webhook {
                url: Url::parse(&format!("http://{addr}/events")).unwrap(),
                secret: Some("signing-secret".into()),
            }),
            s3: None,
        };
        let store = Store::open_with_options(
            "sqlite::memory:",
            refract_storage::StoreOptions {
                outbox_targets: delivery.targets(),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        store
            .insert(&refract_core::Run::new("delivery"))
            .await
            .unwrap();
        assert!(delivery.process_one(&store).await.is_err());
        assert_eq!(store.outbox_pending().await.unwrap(), 1);
        tokio::time::sleep(Duration::from_secs(3)).await;
        assert!(delivery.process_one(&store).await.unwrap());
        assert_eq!(store.outbox_pending().await.unwrap(), 0);
        let events = received.lock().unwrap();
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].1, events[1].1);
        assert_eq!(
            events[1].0["x-refract-signature"],
            format!("sha256={}", hex(hmac(b"signing-secret", &events[1].1)))
        );
        server.abort();
    }

    #[tokio::test]
    async fn object_storage_signs_scoped_put_and_retention_delete() {
        use axum::{http::Method, routing::any};
        let received = Arc::new(Mutex::new(Vec::<(Method, String, HeaderMap, String)>::new()));
        let app = Router::new()
            .route(
                "/{*path}",
                any(
                    |State(state): State<Arc<Mutex<Vec<(Method, String, HeaderMap, String)>>>>,
                     method: Method,
                     uri: axum::http::Uri,
                     headers: HeaderMap,
                     body: String| async move {
                        state
                            .lock()
                            .unwrap()
                            .push((method, uri.path().to_owned(), headers, body));
                        StatusCode::OK
                    },
                ),
            )
            .with_state(received.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let delivery = Delivery {
            client: Client::new(),
            webhook: None,
            s3: Some(S3 {
                endpoint: Url::parse(&format!("http://{address}")).unwrap(),
                bucket: "recordings".into(),
                region: "test-region".into(),
                access_key: "test-access".into(),
                secret_key: "test-secret".into(),
            }),
        };
        assert!(delivery.validate_production("production").is_err());
        let store = Store::open_with_options(
            "sqlite::memory:",
            refract_storage::StoreOptions {
                outbox_targets: delivery.targets(),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        let run = refract_core::Run::new("object-delivery");
        store.insert(&run).await.unwrap();
        assert!(delivery.process_one(&store).await.unwrap());
        store
            .retain_since(Utc::now() + chrono::Duration::seconds(1))
            .await
            .unwrap();
        assert!(delivery.process_one(&store).await.unwrap());
        assert_eq!(store.outbox_pending().await.unwrap(), 0);
        let events = received.lock().unwrap();
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].0, Method::PUT);
        assert_eq!(events[1].0, Method::DELETE);
        assert!(
            events[0]
                .1
                .starts_with("/recordings/local/default/development/")
        );
        assert_eq!(events[0].1, events[1].1);
        assert_eq!(
            events[0].2["x-amz-content-sha256"],
            hex(Sha256::digest(events[0].3.as_bytes()))
        );
        assert!(
            events[0].2["authorization"]
                .to_str()
                .unwrap()
                .starts_with("AWS4-HMAC-SHA256 Credential=test-access/")
        );
        assert!(events[1].3.is_empty());
        server.abort();
    }
}
