-- Run as the migration owner with psql, after migrations:
-- psql "$MIGRATION_URL" -v runtime_role=refract_api -f deploy/production/row-security.sql
-- Create refract_api separately as a LOGIN NOSUPERUSER NOBYPASSRLS role with a secret password.
-- The migration/worker role must retain BYPASSRLS for global maintenance operations.
BEGIN;
GRANT USAGE ON SCHEMA public TO :"runtime_role";
GRANT SELECT, INSERT, UPDATE, DELETE ON ALL TABLES IN SCHEMA public TO :"runtime_role";
REVOKE ALL ON _sqlx_migrations FROM :"runtime_role";
DO $$
DECLARE relation text;
BEGIN
    FOREACH relation IN ARRAY ARRAY['runs','run_events','run_embeddings','audit_log','outbox',
                                    'rate_buckets','trace_assemblies','trace_spans','project_embeddings','embedding_jobs','vector_generations'] LOOP
        EXECUTE format('ALTER TABLE %I ENABLE ROW LEVEL SECURITY', relation);
        EXECUTE format('ALTER TABLE %I FORCE ROW LEVEL SECURITY', relation);
        EXECUTE format('DROP POLICY IF EXISTS refract_scope ON %I', relation);
        EXECUTE format('CREATE POLICY refract_scope ON %I USING (
            organization = current_setting(''refract.organization'', true) AND
            project = current_setting(''refract.project'', true) AND
            environment = current_setting(''refract.environment'', true)
        ) WITH CHECK (
            organization = current_setting(''refract.organization'', true) AND
            project = current_setting(''refract.project'', true) AND
            environment = current_setting(''refract.environment'', true)
        )', relation);
    END LOOP;
END $$;
COMMIT;
