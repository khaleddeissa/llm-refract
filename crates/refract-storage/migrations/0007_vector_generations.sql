CREATE TABLE vector_generations (
    organization TEXT NOT NULL,
    project TEXT NOT NULL,
    environment TEXT NOT NULL,
    model TEXT NOT NULL,
    generation BIGINT NOT NULL DEFAULT 1,
    PRIMARY KEY (organization,project,environment,model)
);
CREATE INDEX vector_page ON run_embeddings(organization,project,environment,model,dimensions,run_id);
