CREATE TABLE scim_directories (
    organization TEXT NOT NULL,
    project TEXT NOT NULL,
    environment TEXT NOT NULL,
    id TEXT NOT NULL DEFAULT 'directory',
    payload TEXT NOT NULL,
    PRIMARY KEY (organization, project, environment)
);
