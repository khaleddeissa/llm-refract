//! Exercise the real startup path with mounted secret files, without mutating process environment.
use reqwest::StatusCode;
use serde_json::json;
use std::{
    path::PathBuf,
    process::{Child, Command, Stdio},
    time::Duration,
};

struct Service {
    child: Option<Child>,
    directory: PathBuf,
}
impl Service {
    fn fixture() -> Self {
        let directory = std::env::temp_dir().join(refract_core::id("production-fixture"));
        std::fs::create_dir(&directory).unwrap();
        std::fs::write(
            directory.join("keys.json"),
            json!([{
                "id":"integration", "key":"integration-key-not-for-real-use-12345",
                "role":"writer", "organization":"test", "project":"test", "environment":"test"
            }])
            .to_string(),
        )
        .unwrap();
        std::fs::write(
            directory.join("encryption"),
            "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=\n",
        )
        .unwrap();
        std::fs::write(
            directory.join("database"),
            format!("sqlite://{}\n", directory.join("test.db").display()),
        )
        .unwrap();
        Self {
            child: None,
            directory,
        }
    }
    fn command(&self) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_refract-server"));
        command
            .env_clear()
            .env("REFRACT_MODE", "production")
            .env("REFRACT_TLS_TERMINATED", "1")
            .env("REFRACT_API_KEYS_FILE", self.directory.join("keys.json"))
            .env(
                "REFRACT_ENCRYPTION_KEY_FILE",
                self.directory.join("encryption"),
            )
            .env("REFRACT_DATABASE_URL_FILE", self.directory.join("database"))
            .env("REFRACT_BIND", "127.0.0.1:0")
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        command
    }
}
impl Drop for Service {
    fn drop(&mut self) {
        if let Some(child) = &mut self.child {
            let _ = child.kill();
            let _ = child.wait();
        }
        let _ = std::fs::remove_dir_all(&self.directory);
    }
}

#[test]
fn invalid_mounted_secret_configuration_exits_before_listening() {
    for missing in [
        "REFRACT_API_KEYS_FILE",
        "REFRACT_ENCRYPTION_KEY_FILE",
        "REFRACT_TLS_TERMINATED",
    ] {
        let mut fixture = Service::fixture();
        fixture.child = Some(fixture.command().env_remove(missing).spawn().unwrap());
        for _ in 0..100 {
            if fixture
                .child
                .as_mut()
                .unwrap()
                .try_wait()
                .unwrap()
                .is_some()
            {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        let status = fixture
            .child
            .as_mut()
            .unwrap()
            .try_wait()
            .unwrap()
            .expect("invalid production configuration must exit, not start a service");
        assert!(!status.success(), "missing {missing}");
    }
    let mut fixture = Service::fixture();
    fixture.child = Some(
        fixture
            .command()
            .env("REFRACT_API_KEYS", "[]")
            .spawn()
            .unwrap(),
    );
    for _ in 0..100 {
        if fixture
            .child
            .as_mut()
            .unwrap()
            .try_wait()
            .unwrap()
            .is_some()
        {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(
        !fixture
            .child
            .as_mut()
            .unwrap()
            .try_wait()
            .unwrap()
            .expect("ambiguous direct and file secrets must fail")
            .success()
    );
}

#[tokio::test]
async fn mounted_secrets_enable_authenticated_encrypted_ingestion() {
    let mut fixture = Service::fixture();
    let socket = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let address = socket.local_addr().unwrap();
    drop(socket);
    fixture.child = Some(
        fixture
            .command()
            .env("REFRACT_BIND", address.to_string())
            .spawn()
            .unwrap(),
    );
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(2))
        .build()
        .unwrap();
    let base = format!("http://{address}");
    for _ in 0..100 {
        if client.get(format!("{base}/v1/ready")).send().await.is_ok() {
            break;
        }
        assert!(
            fixture
                .child
                .as_mut()
                .unwrap()
                .try_wait()
                .unwrap()
                .is_none(),
            "service startup failed"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert_eq!(
        client
            .get(format!("{base}/v1/ready"))
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::OK
    );
    assert_eq!(
        client
            .get(format!("{base}/v1/runs"))
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::UNAUTHORIZED
    );
    let run = refract_core::Run::new("secret-file integration");
    assert_eq!(
        client
            .post(format!("{base}/v1/runs"))
            .bearer_auth("integration-key-not-for-real-use-12345")
            .json(&run)
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::CREATED
    );
    let fetched: refract_core::Run = client
        .get(format!("{base}/v1/runs/{}", run.id))
        .bearer_auth("integration-key-not-for-real-use-12345")
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(fetched, run);
    let bytes = std::fs::read(fixture.directory.join("test.db")).unwrap();
    assert!(bytes.windows(7).any(|window| window == b"enc:v1:"));
    // Run names remain indexed; full event payload encryption is tested in the storage crate.
}
