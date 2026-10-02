use super::*;
use sqlx::{AnyConnection, pool::PoolConnection};

impl Store {
    /// Connect the HTTP service with a restricted PostgreSQL role after migrations.
    /// Background maintenance retains the separate migration/worker store.
    pub async fn runtime_pool(&self, url: &str) -> Result<Self> {
        ensure!(
            url.starts_with("postgres:") || url.starts_with("postgresql:"),
            "row security requires PostgreSQL"
        );
        let pool = AnyPoolOptions::new()
            .max_connections(10)
            .connect(url)
            .await?;
        let (superuser,bypass):(i32,i32)=sqlx::query_as("SELECT CAST(rolsuper AS INTEGER),CAST(rolbypassrls AS INTEGER) FROM pg_roles WHERE rolname=current_user")
            .fetch_one(&pool).await?;
        ensure!(
            superuser == 0 && bypass == 0,
            "runtime database role must not bypass row security"
        );
        let (protected,):(i64,)=sqlx::query_as("SELECT COUNT(*) FROM pg_class c JOIN pg_namespace n ON n.oid=c.relnamespace WHERE n.nspname=current_schema() AND c.relname IN ('runs','run_events','run_embeddings','audit_log','outbox','rate_buckets','trace_assemblies','trace_spans','project_embeddings','embedding_jobs','vector_generations','delivery_versions') AND c.relrowsecurity AND c.relforcerowsecurity")
            .fetch_one(&pool).await?;
        ensure!(
            protected == 12,
            "apply the row-security deployment SQL before starting the restricted runtime"
        );
        Ok(Self {
            pool,
            rls: true,
            ..self.clone()
        })
    }
    async fn apply_scope(&self, connection: &mut AnyConnection, local: bool) -> Result<()> {
        if self.rls {
            // Always overwrite all three settings on checkout; pooled connections can change tenants.
            sqlx::query("SELECT set_config('refract.organization',$1,$4),set_config('refract.project',$2,$4),set_config('refract.environment',$3,$4)")
                .bind(&self.scope.organization).bind(&self.scope.project).bind(&self.scope.environment)
                .bind(local).execute(connection).await?;
        }
        Ok(())
    }
    pub(crate) async fn connection(&self) -> Result<PoolConnection<Any>> {
        let mut connection = self.pool.acquire().await?;
        self.apply_scope(&mut connection, false).await?;
        Ok(connection)
    }
    pub(crate) async fn transaction(&self) -> Result<Transaction<'static, Any>> {
        let mut transaction = self.pool.begin().await?;
        self.apply_scope(&mut transaction, true).await?;
        Ok(transaction)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::{Engine, engine::general_purpose::STANDARD};
    use sqlx::ConnectOptions;
    use std::str::FromStr;

    #[tokio::test]
    #[ignore = "requires disposable PostgreSQL and permission to create test roles"]
    async fn postgres_row_security_contract() {
        let url = std::env::var("REFRACT_TEST_POSTGRES_URL").expect("disposable PostgreSQL URL");
        let store = Store::open_with_options(
            &url,
            StoreOptions {
                encryption: Some(Encryption::from_base64(&STANDARD.encode([9; 32])).unwrap()),
                ..Default::default()
            },
        )
        .await
        .unwrap();
        assert!(
            store.runtime_pool(&url).await.is_err(),
            "superuser must be rejected as API runtime"
        );
        let role = refract_core::id("refract_test").replace('-', "");
        let password = refract_core::id("local_test");
        sqlx::raw_sql(&format!(
            "CREATE ROLE {role} LOGIN NOSUPERUSER NOBYPASSRLS PASSWORD '{password}'"
        ))
        .execute(&store.pool)
        .await
        .unwrap();
        let setup = include_str!("../../../deploy/production/row-security.sql")
            .replace(":\"runtime_role\"", &role);
        sqlx::raw_sql(&setup).execute(&store.pool).await.unwrap();
        let runtime_url = sqlx::postgres::PgConnectOptions::from_str(&url)
            .unwrap()
            .username(&role)
            .password(&password)
            .to_url_lossy()
            .to_string();
        let runtime = store.runtime_pool(&runtime_url).await.unwrap();
        let a = runtime.scoped(Scope {
            organization: role.clone(),
            project: "a".into(),
            environment: "test".into(),
        });
        let b = runtime.scoped(Scope {
            project: "b".into(),
            ..a.scope.clone()
        });
        let first = Run::new("tenant-a");
        let second = Run::new("tenant-b");
        a.insert(&first).await.unwrap();
        b.insert(&second).await.unwrap();
        assert!(a.get(&second.id).await.unwrap().is_none());
        let replica = store.runtime_pool(&runtime_url).await.unwrap();
        let peer = replica.scoped(a.scope.clone());
        let mut requests = tokio::task::JoinSet::new();
        for index in 0..20 {
            let scoped = if index % 2 == 0 {
                a.clone()
            } else {
                peer.clone()
            };
            requests
                .spawn(async move { scoped.allow_request("replica-fixture", 5).await.unwrap() });
        }
        let mut accepted = 0;
        while let Some(result) = requests.join_next().await {
            accepted += usize::from(result.unwrap());
        }
        assert_eq!(accepted, 5, "quota must span independent connection pools");
        let vector = Embedding {
            model: "pg-local".into(),
            values: vec![1.0, 0.0],
        };
        a.put_embedding(&first.id, &vector).await.unwrap();
        assert_eq!(peer.vector_search(&vector, 1).await.unwrap().len(), 1);
        assert!(b.vector_search(&vector, 1).await.unwrap().is_empty());
        assert_eq!(peer.rotate_encryption(10).await.unwrap(), 0);
        // Deliberately omit application scope filters: the database must still isolate rows.
        for scoped in [&a, &b, &a, &b] {
            let mut connection = scoped.connection().await.unwrap();
            let (count,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM runs")
                .fetch_one(&mut *connection)
                .await
                .unwrap();
            assert_eq!(count, 1);
        }
        let mut conn = a.connection().await.unwrap();
        assert!(
            sqlx::query("UPDATE runs SET project='b'")
                .execute(&mut *conn)
                .await
                .is_err()
        );
        drop(conn);
        replica.pool.close().await;
        runtime.pool.close().await;
        sqlx::raw_sql(&format!("DROP OWNED BY {role}; DROP ROLE {role};"))
            .execute(&store.pool)
            .await
            .unwrap();
        store.pool.close().await;
    }
}
