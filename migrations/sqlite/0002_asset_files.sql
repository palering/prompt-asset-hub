-- Physical renditions belonging to a logical asset.

CREATE TABLE asset_files (
    id                 TEXT PRIMARY KEY NOT NULL,
    tenant_id          TEXT NOT NULL,
    asset_id           TEXT NOT NULL,
    role               TEXT NOT NULL,
    storage_key        TEXT NOT NULL,
    mime_type          TEXT NOT NULL,
    byte_size          INTEGER NOT NULL CHECK (byte_size > 0),
    width              INTEGER CHECK (width IS NULL OR width > 0),
    height             INTEGER CHECK (height IS NULL OR height > 0),
    sha256             TEXT NOT NULL,
    generator_version  TEXT,
    created_at         TEXT NOT NULL,
    deleted_at         TEXT,
    CONSTRAINT asset_files_dimensions_are_paired CHECK (
        (width IS NULL AND height IS NULL) OR (width IS NOT NULL AND height IS NOT NULL)
    ),
    CONSTRAINT asset_files_tenant_fk FOREIGN KEY (tenant_id)
        REFERENCES tenants (id) ON UPDATE CASCADE ON DELETE RESTRICT,
    CONSTRAINT asset_files_asset_fk FOREIGN KEY (tenant_id, asset_id)
        REFERENCES assets (tenant_id, id) ON UPDATE CASCADE ON DELETE CASCADE,
    UNIQUE (tenant_id, id),
    UNIQUE (tenant_id, storage_key),
    UNIQUE (tenant_id, asset_id, role)
);

CREATE INDEX idx_asset_files_asset
    ON asset_files (tenant_id, asset_id, role, deleted_at);
