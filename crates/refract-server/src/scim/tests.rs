use super::*;
use crate::tests::request;

#[tokio::test]
async fn scim_filter_patch_groups_deactivation_and_admin_boundary() {
    let store = Store::open("sqlite::memory:").await.unwrap();
    let mut security = crate::tests::keys();
    security
        .identities
        .iter_mut()
        .find(|i| i.id == "writer-b")
        .unwrap()
        .role = Role::Admin;
    security.oidc = Some(
        crate::oidc::Oidc::from_jwks("https://issuer.invalid", "refract", r#"{"keys":[]}"#)
            .unwrap(),
    );
    security
        .scim_group_roles
        .insert("Writers".into(), "writer".into());
    let app = router_with_security(store.clone(), security);
    let admin = Some("zzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzz");
    let reader = Some("rrrrrrrrrrrrrrrrrrrrrrrrrrrrrrrr");
    assert_eq!(
        request(&app, "GET", "/scim/v2/Users", reader, Value::Null)
            .await
            .0,
        StatusCode::FORBIDDEN
    );
    let (status,user) = request(&app,"POST","/scim/v2/Users",admin,json!({"userName":"ada@example.invalid","externalId":"subject-ada","active":true,"roles":[{"value":"admin"}]})).await;
    assert_eq!(status, StatusCode::CREATED, "{user}");
    let id = user["id"].as_str().unwrap();
    assert!(
        store
            .resolve_subject("https://issuer.invalid", "subject-ada")
            .await
            .unwrap()
            .is_none()
    );
    let (status, group) = request(
        &app,
        "POST",
        "/scim/v2/Groups",
        admin,
        json!({"displayName":"Writers","members":[{"value":id}]}),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{group}");
    assert_eq!(
        store
            .resolve_subject("https://issuer.invalid", "subject-ada")
            .await
            .unwrap()
            .unwrap()
            .role,
        "writer"
    );
    let (_, list) = request(
        &app,
        "GET",
        "/scim/v2/Users?filter=userName%20eq%20%22ADA%40example.invalid%22&count=1",
        admin,
        Value::Null,
    )
    .await;
    assert_eq!(list["totalResults"], 1);
    assert_eq!(
        request(
            &app,
            "GET",
            &format!("/scim/v2/Users/{id}"),
            Some("bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"),
            Value::Null
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
    let patch = |ops: Value| json!({"schemas":["urn:ietf:params:scim:api:messages:2.0:PatchOp"],"Operations":ops});
    let group_path = format!("/scim/v2/Groups/{}", group["id"].as_str().unwrap());
    let (status, _) = request(
        &app,
        "PATCH",
        &group_path,
        admin,
        patch(json!([
            {"op":"remove","path":format!("members[value eq \"{id}\"]")}
        ])),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        store
            .resolve_subject("https://issuer.invalid", "subject-ada")
            .await
            .unwrap()
            .is_none()
    );
    let user_path = format!("/scim/v2/Users/{id}");
    assert_eq!(
        request(
            &app,
            "PATCH",
            &user_path,
            admin,
            patch(json!([
                {"op":"replace","path":"active","value":false},
                {"op":"replace","path":"externalId","value":"another-user"}
            ]))
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        request(&app, "GET", &user_path, admin, Value::Null).await.1["active"],
        true,
        "failed multi-operation patch must roll back"
    );
    assert_eq!(
        request(
            &app,
            "PATCH",
            &user_path,
            admin,
            patch(json!([{"op":"replace","path":"active","value":false}]))
        )
        .await
        .0,
        StatusCode::OK
    );
    assert_eq!(
        request(&app, "DELETE", &user_path, admin, Value::Null)
            .await
            .0,
        StatusCode::NO_CONTENT
    );
}
