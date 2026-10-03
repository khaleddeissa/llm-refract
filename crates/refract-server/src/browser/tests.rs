use super::*;
use axum::{
    body::{Body, to_bytes},
    http::Request,
};
use base64::engine::general_purpose::STANDARD;
use jsonwebtoken::{Algorithm, EncodingKey, Header, encode};
use p256::{
    SecretKey,
    elliptic_curve::{rand_core::OsRng, sec1::ToEncodedPoint},
    pkcs8::{EncodePrivateKey, LineEnding},
};
use std::sync::{Arc, Mutex};
use tower::ServiceExt;

async fn call(
    app: &Router,
    method: &str,
    path: &str,
    cookie: &str,
    origin: Option<&str>,
    value: Value,
) -> Response {
    let mut request = Request::builder()
        .method(method)
        .uri(path)
        .header("content-type", "application/json")
        .header("cookie", cookie);
    if let Some(origin) = origin {
        request = request.header("origin", origin);
    }
    app.clone()
        .oneshot(request.body(Body::from(value.to_string())).unwrap())
        .await
        .unwrap()
}
fn response_cookie(response: &Response, name: &str) -> String {
    response
        .headers()
        .get_all("set-cookie")
        .iter()
        .map(|v| v.to_str().unwrap())
        .find(|v| v.starts_with(name))
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .to_owned()
}
#[tokio::test]
async fn encrypted_sessions_survive_restart_refresh_once_and_revoke_immediately() {
    let key = SecretKey::random(&mut OsRng);
    let point = key.public_key().to_encoded_point(false);
    let jwks = json!({"keys":[{"kty":"EC","crv":"P-256","kid":"session","alg":"ES256","use":"sig","x":URL_SAFE_NO_PAD.encode(point.x().unwrap()),"y":URL_SAFE_NO_PAD.encode(point.y().unwrap())}]}).to_string();
    let verifier = oidc::Oidc::from_jwks("https://issuer.invalid", "refract", &jwks).unwrap();
    let pem = key.to_pkcs8_pem(LineEnding::LF).unwrap();
    let signing = EncodingKey::from_ec_pem(pem.as_bytes()).unwrap();
    let mut header = Header::new(Algorithm::ES256);
    header.kid = Some("session".into());
    let mint = |seconds| {
        encode(&header,&json!({"sub":"ada","iss":"https://issuer.invalid","aud":"refract","exp":chrono::Utc::now().timestamp()+seconds}),&signing).unwrap()
    };
    let initial = mint(15);
    let refreshed = mint(600);
    let requests = Arc::new(Mutex::new(
        Vec::<std::collections::HashMap<String, String>>::new(),
    ));
    let observed = requests.clone();
    let provider = Router::new().route("/token",post(move |axum::Form(form): axum::Form<std::collections::HashMap<String,String>>| {
        let initial = initial.clone();let refreshed = refreshed.clone();let observed = observed.clone();
        async move {
            let refresh = form.get("grant_type").unwrap() == "refresh_token";
            if refresh { assert_eq!(form["refresh_token"],"refresh-one"); }
            observed.lock().unwrap().push(form);
            Json(json!({"access_token":if refresh {refreshed} else {initial},"token_type":"Bearer","refresh_token":if refresh {"refresh-two"} else {"refresh-one"}}))
        }
    }));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}/token", listener.local_addr().unwrap());
    let task = tokio::spawn(async move { axum::serve(listener, provider).await.unwrap() });
    let store = Store::open_with_options(
        "sqlite::memory:",
        StoreOptions {
            encryption: Some(Encryption::from_base64(&STANDARD.encode([7; 32])).unwrap()),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    store
        .provision("https://issuer.invalid", "ada", "reader", true)
        .await
        .unwrap();
    let security = Security {
        oidc: Some(verifier),
        login: Some(login::Login {
            issuer: "https://issuer.invalid".into(),
            client_id: "inspector".into(),
            authorization_endpoint: "https://issuer.invalid/authorize".into(),
            token_endpoint: endpoint,
            redirect_uri: "https://ui.invalid/".into(),
            scope: "openid offline_access".into(),
            authorization_params: Default::default(),
            client_secret: None,
        }),
        ..Security::default()
    };
    let app = router_with_security(store.clone(), security.clone());
    assert_eq!(
        call(
            &app,
            "POST",
            "/v1/auth/start",
            "",
            Some("https://evil.invalid"),
            json!({})
        )
        .await
        .status(),
        StatusCode::FORBIDDEN
    );
    let response = call(
        &app,
        "POST",
        "/v1/auth/start",
        "",
        Some("https://ui.invalid"),
        json!({}),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    assert!(
        response.headers()["set-cookie"]
            .to_str()
            .unwrap()
            .contains("HttpOnly; Secure; SameSite=Lax")
    );
    let login_cookie = response_cookie(&response, LOGIN);
    let value: Value =
        serde_json::from_slice(&to_bytes(response.into_body(), 65536).await.unwrap()).unwrap();
    let url = reqwest::Url::parse(value["url"].as_str().unwrap()).unwrap();
    let params: std::collections::HashMap<_, _> = url.query_pairs().into_owned().collect();
    let callback = json!({"state":params["state"],"code":"one-use-code"});
    assert_eq!(
        call(
            &app,
            "POST",
            "/v1/auth/complete",
            "__Host-refract.login=wrong",
            Some("https://ui.invalid"),
            callback.clone()
        )
        .await
        .status(),
        StatusCode::BAD_REQUEST
    );
    let response = call(
        &app,
        "POST",
        "/v1/auth/complete",
        &login_cookie,
        Some("https://ui.invalid"),
        callback.clone(),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let session = response_cookie(&response, SESSION);
    assert_eq!(
        call(
            &app,
            "POST",
            "/v1/auth/complete",
            &login_cookie,
            Some("https://ui.invalid"),
            callback
        )
        .await
        .status(),
        StatusCode::BAD_REQUEST
    );
    let replacement = router_with_security(store.clone(), security);
    let (a, b) = tokio::join!(
        call(
            &replacement,
            "GET",
            "/v1/auth/me",
            &session,
            None,
            Value::Null
        ),
        call(&app, "GET", "/v1/auth/me", &session, None, Value::Null)
    );
    assert_eq!(a.status(), StatusCode::OK);
    assert_eq!(b.status(), StatusCode::OK);
    {
        let requests = requests.lock().unwrap();
        assert_eq!(
            requests.len(),
            2,
            "one authorization exchange and one refresh across replicas"
        );
        assert_eq!(
            URL_SAFE_NO_PAD.encode(Sha256::digest(requests[0]["code_verifier"].as_bytes())),
            params["code_challenge"]
        );
    }
    assert_eq!(
        call(&app, "POST", "/v1/logs", &session, None, json!({}))
            .await
            .status(),
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        call(
            &app,
            "POST",
            "/v1/auth/logout",
            &session,
            Some("https://evil.invalid"),
            json!({})
        )
        .await
        .status(),
        StatusCode::FORBIDDEN
    );
    store
        .provision("https://issuer.invalid", "ada", "reader", false)
        .await
        .unwrap();
    assert_eq!(
        call(&app, "GET", "/v1/auth/me", &session, None, Value::Null)
            .await
            .status(),
        StatusCode::UNAUTHORIZED
    );
    let response = call(
        &app,
        "POST",
        "/v1/auth/logout",
        &session,
        Some("https://ui.invalid"),
        json!({}),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    assert!(
        response.headers()["set-cookie"]
            .to_str()
            .unwrap()
            .contains("Max-Age=0")
    );
    task.abort();
}
