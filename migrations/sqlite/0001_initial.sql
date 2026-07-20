-- Prompt Asset Hub local schema.
--
-- UUID/ULID values and RFC 3339 UTC timestamps are stored as TEXT so an
-- exported local library can be imported into PostgreSQL without changing
-- identifiers or timestamp semantics. JSON values are likewise stored as
-- canonical JSON text and validated by the application layer.

CREATE TABLE tenants (
    id          TEXT PRIMARY KEY NOT NULL,
    slug        TEXT NOT NULL,
    name        TEXT NOT NULL,
    status      TEXT NOT NULL DEFAULT 'active',
    created_at  TEXT NOT NULL,
    updated_at  TEXT NOT NULL,
    deleted_at  TEXT,
    UNIQUE (slug)
);

CREATE TABLE assets (
    id                 TEXT PRIMARY KEY NOT NULL,
    tenant_id          TEXT NOT NULL,
    media_type         TEXT NOT NULL,
    storage_key        TEXT,
    external_url       TEXT,
    original_filename  TEXT,
    mime_type          TEXT,
    byte_size          INTEGER CHECK (byte_size IS NULL OR byte_size > 0),
    width              INTEGER CHECK (width IS NULL OR width > 0),
    height             INTEGER CHECK (height IS NULL OR height > 0),
    duration_ms        INTEGER CHECK (duration_ms IS NULL OR duration_ms > 0),
    sha256             TEXT,
    thumbnail_key      TEXT,
    visibility         TEXT NOT NULL DEFAULT 'private',
    status             TEXT NOT NULL DEFAULT 'active',
    metadata_json      TEXT NOT NULL DEFAULT '{}',
    created_at         TEXT NOT NULL,
    updated_at         TEXT NOT NULL,
    deleted_at         TEXT,
    CONSTRAINT assets_have_one_location CHECK (
        (storage_key IS NOT NULL) != (external_url IS NOT NULL)
    ),
    CONSTRAINT assets_stored_metadata CHECK (
        storage_key IS NULL OR (mime_type IS NOT NULL AND byte_size IS NOT NULL AND sha256 IS NOT NULL)
    ),
    CONSTRAINT assets_dimensions_are_paired CHECK (
        (width IS NULL AND height IS NULL) OR (width IS NOT NULL AND height IS NOT NULL)
    ),
    CONSTRAINT assets_tenant_fk FOREIGN KEY (tenant_id)
        REFERENCES tenants (id) ON UPDATE CASCADE ON DELETE RESTRICT,
    UNIQUE (tenant_id, id)
);

CREATE TABLE projects (
    id              TEXT PRIMARY KEY NOT NULL,
    tenant_id       TEXT NOT NULL,
    name            TEXT NOT NULL,
    description     TEXT,
    cover_asset_id  TEXT,
    status          TEXT NOT NULL DEFAULT 'active',
    created_at      TEXT NOT NULL,
    updated_at      TEXT NOT NULL,
    deleted_at      TEXT,
    CONSTRAINT projects_tenant_fk FOREIGN KEY (tenant_id)
        REFERENCES tenants (id) ON UPDATE CASCADE ON DELETE RESTRICT,
    CONSTRAINT projects_cover_fk FOREIGN KEY (tenant_id, cover_asset_id)
        REFERENCES assets (tenant_id, id) ON UPDATE CASCADE ON DELETE RESTRICT,
    UNIQUE (tenant_id, id)
);

CREATE TABLE prompts (
    id                    TEXT PRIMARY KEY NOT NULL,
    tenant_id             TEXT NOT NULL,
    project_id            TEXT NOT NULL,
    title                 TEXT NOT NULL,
    creative_intent       TEXT NOT NULL DEFAULT '',
    base_prompt           TEXT NOT NULL,
    negative_constraints  TEXT,
    status                TEXT NOT NULL DEFAULT 'draft',
    current_version       INTEGER NOT NULL DEFAULT 1 CHECK (current_version > 0),
    created_by            TEXT NOT NULL,
    created_at            TEXT NOT NULL,
    updated_at            TEXT NOT NULL,
    deleted_at            TEXT,
    CONSTRAINT prompts_tenant_fk FOREIGN KEY (tenant_id)
        REFERENCES tenants (id) ON UPDATE CASCADE ON DELETE RESTRICT,
    CONSTRAINT prompts_project_fk FOREIGN KEY (tenant_id, project_id)
        REFERENCES projects (tenant_id, id) ON UPDATE CASCADE ON DELETE RESTRICT,
    UNIQUE (tenant_id, id)
);

CREATE TABLE prompt_variants (
    id                TEXT PRIMARY KEY NOT NULL,
    tenant_id         TEXT NOT NULL,
    prompt_id         TEXT NOT NULL,
    provider          TEXT NOT NULL,
    model             TEXT NOT NULL,
    model_version     TEXT,
    prompt_text       TEXT NOT NULL,
    parameters_json   TEXT NOT NULL DEFAULT '{}',
    adaptation_notes  TEXT,
    success_tips      TEXT,
    failure_patterns  TEXT,
    status            TEXT NOT NULL DEFAULT 'draft',
    revision          INTEGER NOT NULL DEFAULT 1 CHECK (revision > 0),
    created_at        TEXT NOT NULL,
    updated_at        TEXT NOT NULL,
    deleted_at        TEXT,
    CONSTRAINT prompt_variants_tenant_fk FOREIGN KEY (tenant_id)
        REFERENCES tenants (id) ON UPDATE CASCADE ON DELETE RESTRICT,
    CONSTRAINT prompt_variants_prompt_fk FOREIGN KEY (tenant_id, prompt_id)
        REFERENCES prompts (tenant_id, id) ON UPDATE CASCADE ON DELETE RESTRICT,
    UNIQUE (tenant_id, id)
);

CREATE TABLE generation_runs (
    id                        TEXT PRIMARY KEY NOT NULL,
    tenant_id                 TEXT NOT NULL,
    variant_id                TEXT NOT NULL,
    source                    TEXT NOT NULL,
    input_snapshot_json       TEXT NOT NULL,
    parameters_snapshot_json  TEXT NOT NULL DEFAULT '{}',
    external_url              TEXT,
    started_at                TEXT,
    completed_at              TEXT,
    status                    TEXT NOT NULL DEFAULT 'pending',
    failure_message           TEXT,
    cost_amount_micros        INTEGER CHECK (cost_amount_micros IS NULL OR cost_amount_micros >= 0),
    cost_currency             TEXT,
    created_by                TEXT NOT NULL,
    created_at                TEXT NOT NULL,
    updated_at                TEXT NOT NULL,
    deleted_at                TEXT,
    CONSTRAINT generation_runs_tenant_fk FOREIGN KEY (tenant_id)
        REFERENCES tenants (id) ON UPDATE CASCADE ON DELETE RESTRICT,
    CONSTRAINT generation_runs_variant_fk FOREIGN KEY (tenant_id, variant_id)
        REFERENCES prompt_variants (tenant_id, id) ON UPDATE CASCADE ON DELETE RESTRICT,
    UNIQUE (tenant_id, id)
);

CREATE TABLE asset_links (
    id                 TEXT PRIMARY KEY NOT NULL,
    tenant_id          TEXT NOT NULL,
    asset_id           TEXT NOT NULL,
    generation_run_id  TEXT,
    prompt_id          TEXT,
    related_asset_id   TEXT,
    relation_type      TEXT NOT NULL,
    sort_order         INTEGER NOT NULL DEFAULT 0,
    note               TEXT,
    created_at         TEXT NOT NULL,
    updated_at         TEXT NOT NULL,
    deleted_at         TEXT,
    CONSTRAINT asset_links_have_target CHECK (
        generation_run_id IS NOT NULL OR prompt_id IS NOT NULL OR related_asset_id IS NOT NULL
    ),
    CONSTRAINT asset_links_tenant_fk FOREIGN KEY (tenant_id)
        REFERENCES tenants (id) ON UPDATE CASCADE ON DELETE RESTRICT,
    CONSTRAINT asset_links_asset_fk FOREIGN KEY (tenant_id, asset_id)
        REFERENCES assets (tenant_id, id) ON UPDATE CASCADE ON DELETE RESTRICT,
    CONSTRAINT asset_links_run_fk FOREIGN KEY (tenant_id, generation_run_id)
        REFERENCES generation_runs (tenant_id, id) ON UPDATE CASCADE ON DELETE RESTRICT,
    CONSTRAINT asset_links_prompt_fk FOREIGN KEY (tenant_id, prompt_id)
        REFERENCES prompts (tenant_id, id) ON UPDATE CASCADE ON DELETE RESTRICT,
    CONSTRAINT asset_links_related_asset_fk FOREIGN KEY (tenant_id, related_asset_id)
        REFERENCES assets (tenant_id, id) ON UPDATE CASCADE ON DELETE RESTRICT,
    UNIQUE (tenant_id, id)
);

CREATE TABLE evaluations (
    id                     TEXT PRIMARY KEY NOT NULL,
    tenant_id              TEXT NOT NULL,
    asset_id               TEXT NOT NULL,
    generation_run_id      TEXT,
    overall_score          INTEGER NOT NULL CHECK (overall_score BETWEEN 1 AND 5),
    is_featured            INTEGER NOT NULL DEFAULT 0 CHECK (is_featured IN (0, 1)),
    is_reusable            INTEGER NOT NULL DEFAULT 0 CHECK (is_reusable IN (0, 1)),
    dimension_scores_json  TEXT NOT NULL DEFAULT '{}',
    strengths              TEXT,
    issues                 TEXT,
    lesson                 TEXT,
    created_by             TEXT NOT NULL,
    created_at             TEXT NOT NULL,
    updated_at             TEXT NOT NULL,
    deleted_at             TEXT,
    CONSTRAINT evaluations_tenant_fk FOREIGN KEY (tenant_id)
        REFERENCES tenants (id) ON UPDATE CASCADE ON DELETE RESTRICT,
    CONSTRAINT evaluations_asset_fk FOREIGN KEY (tenant_id, asset_id)
        REFERENCES assets (tenant_id, id) ON UPDATE CASCADE ON DELETE RESTRICT,
    CONSTRAINT evaluations_run_fk FOREIGN KEY (tenant_id, generation_run_id)
        REFERENCES generation_runs (tenant_id, id) ON UPDATE CASCADE ON DELETE RESTRICT,
    UNIQUE (tenant_id, id)
);

CREATE TABLE tags (
    id          TEXT PRIMARY KEY NOT NULL,
    tenant_id   TEXT NOT NULL,
    name        TEXT NOT NULL,
    category    TEXT NOT NULL,
    parent_id   TEXT,
    created_at  TEXT NOT NULL,
    updated_at  TEXT NOT NULL,
    deleted_at  TEXT,
    CONSTRAINT tags_tenant_fk FOREIGN KEY (tenant_id)
        REFERENCES tenants (id) ON UPDATE CASCADE ON DELETE RESTRICT,
    CONSTRAINT tags_parent_fk FOREIGN KEY (tenant_id, parent_id)
        REFERENCES tags (tenant_id, id) ON UPDATE CASCADE ON DELETE RESTRICT,
    UNIQUE (tenant_id, id)
);

CREATE TABLE prompt_tags (
    tenant_id  TEXT NOT NULL,
    prompt_id  TEXT NOT NULL,
    tag_id     TEXT NOT NULL,
    created_at TEXT NOT NULL,
    CONSTRAINT prompt_tags_tenant_fk FOREIGN KEY (tenant_id)
        REFERENCES tenants (id) ON UPDATE CASCADE ON DELETE RESTRICT,
    CONSTRAINT prompt_tags_prompt_fk FOREIGN KEY (tenant_id, prompt_id)
        REFERENCES prompts (tenant_id, id) ON UPDATE CASCADE ON DELETE CASCADE,
    CONSTRAINT prompt_tags_tag_fk FOREIGN KEY (tenant_id, tag_id)
        REFERENCES tags (tenant_id, id) ON UPDATE CASCADE ON DELETE CASCADE,
    PRIMARY KEY (tenant_id, prompt_id, tag_id)
);

CREATE TABLE asset_tags (
    tenant_id  TEXT NOT NULL,
    asset_id   TEXT NOT NULL,
    tag_id     TEXT NOT NULL,
    created_at TEXT NOT NULL,
    CONSTRAINT asset_tags_tenant_fk FOREIGN KEY (tenant_id)
        REFERENCES tenants (id) ON UPDATE CASCADE ON DELETE RESTRICT,
    CONSTRAINT asset_tags_asset_fk FOREIGN KEY (tenant_id, asset_id)
        REFERENCES assets (tenant_id, id) ON UPDATE CASCADE ON DELETE CASCADE,
    CONSTRAINT asset_tags_tag_fk FOREIGN KEY (tenant_id, tag_id)
        REFERENCES tags (tenant_id, id) ON UPDATE CASCADE ON DELETE CASCADE,
    PRIMARY KEY (tenant_id, asset_id, tag_id)
);

CREATE TABLE audit_events (
    id            TEXT PRIMARY KEY NOT NULL,
    tenant_id     TEXT NOT NULL,
    actor_type    TEXT NOT NULL,
    actor_id      TEXT,
    action        TEXT NOT NULL,
    entity_type   TEXT NOT NULL,
    entity_id     TEXT,
    payload_json  TEXT NOT NULL DEFAULT '{}',
    created_at    TEXT NOT NULL,
    CONSTRAINT audit_events_tenant_fk FOREIGN KEY (tenant_id)
        REFERENCES tenants (id) ON UPDATE CASCADE ON DELETE RESTRICT,
    UNIQUE (tenant_id, id)
);

CREATE INDEX idx_assets_library
    ON assets (tenant_id, deleted_at, status, media_type, created_at DESC);
CREATE UNIQUE INDEX uq_assets_storage_key_active
    ON assets (tenant_id, storage_key)
    WHERE storage_key IS NOT NULL AND deleted_at IS NULL;
CREATE UNIQUE INDEX uq_assets_sha256_active
    ON assets (tenant_id, sha256)
    WHERE sha256 IS NOT NULL AND deleted_at IS NULL;

CREATE INDEX idx_projects_library
    ON projects (tenant_id, deleted_at, status, updated_at DESC);
CREATE INDEX idx_prompts_project
    ON prompts (tenant_id, project_id, deleted_at, updated_at DESC);
CREATE INDEX idx_prompts_library
    ON prompts (tenant_id, deleted_at, status, updated_at DESC);

CREATE INDEX idx_prompt_variants_prompt
    ON prompt_variants (tenant_id, prompt_id, deleted_at, updated_at DESC);
CREATE INDEX idx_prompt_variants_model
    ON prompt_variants (tenant_id, provider, model, model_version, deleted_at);

CREATE INDEX idx_generation_runs_variant
    ON generation_runs (tenant_id, variant_id, deleted_at, created_at DESC);
CREATE INDEX idx_generation_runs_status
    ON generation_runs (tenant_id, status, deleted_at, created_at DESC);

CREATE INDEX idx_asset_links_asset
    ON asset_links (tenant_id, asset_id, relation_type, deleted_at, sort_order);
CREATE INDEX idx_asset_links_run
    ON asset_links (tenant_id, generation_run_id, relation_type, deleted_at, sort_order);
CREATE INDEX idx_asset_links_prompt
    ON asset_links (tenant_id, prompt_id, relation_type, deleted_at, sort_order);
CREATE INDEX idx_asset_links_related_asset
    ON asset_links (tenant_id, related_asset_id, relation_type, deleted_at);

CREATE INDEX idx_evaluations_asset
    ON evaluations (tenant_id, asset_id, deleted_at, created_at DESC);
CREATE INDEX idx_evaluations_featured
    ON evaluations (tenant_id, is_featured, is_reusable, deleted_at, overall_score DESC);

CREATE INDEX idx_tags_parent
    ON tags (tenant_id, parent_id, deleted_at, category, name);
CREATE UNIQUE INDEX uq_tags_name_active
    ON tags (tenant_id, category, name)
    WHERE deleted_at IS NULL;
CREATE INDEX idx_prompt_tags_tag ON prompt_tags (tenant_id, tag_id, prompt_id);
CREATE INDEX idx_asset_tags_tag ON asset_tags (tenant_id, tag_id, asset_id);

CREATE INDEX idx_audit_events_entity
    ON audit_events (tenant_id, entity_type, entity_id, created_at DESC);
CREATE INDEX idx_audit_events_time
    ON audit_events (tenant_id, created_at DESC);
