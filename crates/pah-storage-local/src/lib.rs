use std::{
    io::Cursor,
    path::{Component, Path, PathBuf},
};

use async_trait::async_trait;
use image::{GenericImageView, ImageFormat};
use pah_application::{
    BlobStore as ApplicationBlobStore, BlobStoreError, StagedBlob, StoreBlobRequest, StoredBlob,
    StoredDerivative,
};
use pah_domain::{AssetFileRole, TenantId};
use sha2::{Digest, Sha256};
use thiserror::Error;
use tokio::{fs, io::AsyncWriteExt};

#[derive(Debug, Error)]
pub enum LocalStorageError {
    #[error("storage key is empty or unsafe: {0}")]
    UnsafeKey(String),
    #[error("storage object does not exist: {0}")]
    NotFound(String),
    #[error("storage I/O failed: {0}")]
    Io(#[from] std::io::Error),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredObject {
    pub key: String,
    pub size: u64,
    pub sha256: String,
}

#[async_trait]
pub trait BlobStore: Send + Sync {
    async fn put_bytes(&self, key: &str, bytes: &[u8]) -> Result<StoredObject, LocalStorageError>;
    async fn read_bytes(&self, key: &str) -> Result<Vec<u8>, LocalStorageError>;
    async fn remove(&self, key: &str) -> Result<(), LocalStorageError>;
    async fn exists(&self, key: &str) -> Result<bool, LocalStorageError>;
}

#[derive(Debug, Clone)]
pub struct LocalBlobStore {
    root: PathBuf,
    temporary_root: PathBuf,
}

impl LocalBlobStore {
    pub async fn open(root: impl Into<PathBuf>) -> Result<Self, LocalStorageError> {
        let root = root.into();
        let temporary_root = root.join(".tmp");
        fs::create_dir_all(&temporary_root).await?;
        Ok(Self {
            root,
            temporary_root,
        })
    }

    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Stages bytes behind an opaque token. Desktop file-picker commands use
    /// this method before handing a `StagedBlob` to the application service.
    pub async fn stage_bytes(
        &self,
        bytes: &[u8],
        original_filename: Option<String>,
        declared_mime_type: impl Into<String>,
    ) -> Result<StagedBlob, LocalStorageError> {
        let declared_mime_type = declared_mime_type.into();
        if declared_mime_type.trim().is_empty() {
            return Err(LocalStorageError::UnsafeKey(
                "declared MIME type is empty".to_owned(),
            ));
        }

        let token = uuid::Uuid::new_v4().to_string();
        let path = self.temporary_root.join(format!("{token}.stage"));
        let mut file = fs::File::create(path).await?;
        file.write_all(bytes).await?;
        file.sync_all().await?;

        Ok(StagedBlob {
            token,
            original_filename,
            declared_mime_type,
        })
    }

    fn resolve(&self, key: &str) -> Result<PathBuf, LocalStorageError> {
        if key.trim().is_empty() {
            return Err(LocalStorageError::UnsafeKey(key.to_owned()));
        }

        let path = Path::new(key);
        if path.is_absolute()
            || path.components().any(|component| {
                matches!(
                    component,
                    Component::ParentDir | Component::RootDir | Component::Prefix(_)
                )
            })
        {
            return Err(LocalStorageError::UnsafeKey(key.to_owned()));
        }

        Ok(self.root.join(path))
    }
}

#[async_trait]
impl ApplicationBlobStore for LocalBlobStore {
    async fn store(
        &self,
        tenant_id: TenantId,
        request: StoreBlobRequest,
    ) -> Result<StoredBlob, BlobStoreError> {
        if request.staged.token.is_empty()
            || request.staged.token.contains('/')
            || request.staged.token.contains('\\')
            || request.staged.token.contains("..")
        {
            return Err(BlobStoreError::InvalidBlob(
                "staging token is malformed".to_owned(),
            ));
        }

        let staged_path = self
            .temporary_root
            .join(format!("{}.stage", request.staged.token));
        let bytes = match fs::read(&staged_path).await {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Err(BlobStoreError::StagedBlobNotFound(request.staged.token));
            }
            Err(error) => return Err(BlobStoreError::Unavailable(error.to_string())),
        };
        if bytes.is_empty() {
            return Err(BlobStoreError::InvalidBlob(
                "empty files cannot be imported".to_owned(),
            ));
        }

        let declared_mime_type = request
            .staged
            .declared_mime_type
            .trim()
            .to_ascii_lowercase();
        let mime_type = detect_mime_type(&bytes).ok_or_else(|| {
            BlobStoreError::InvalidBlob("unsupported or unrecognized media bytes".to_owned())
        })?;
        if declared_mime_type != "application/octet-stream" && declared_mime_type != mime_type {
            return Err(BlobStoreError::InvalidBlob(format!(
                "declared MIME type {declared_mime_type} does not match detected {mime_type}"
            )));
        }
        let extension = extension_for_mime(mime_type);
        let derived_image = if mime_type.starts_with("image/") {
            Some(create_thumbnail(&bytes)?)
        } else {
            None
        };
        let storage_key = format!(
            "tenants/{tenant_id}/assets/{}/original.{extension}",
            request.asset_id
        );
        let stored = self
            .put_bytes(&storage_key, &bytes)
            .await
            .map_err(map_storage_error)?;
        fs::remove_file(staged_path)
            .await
            .map_err(|error| BlobStoreError::Unavailable(error.to_string()))?;

        let (width, height, derivatives) = if let Some(thumbnail) = derived_image {
            let thumbnail_key = format!(
                "tenants/{tenant_id}/assets/{}/preview/thumbnail-480.webp",
                request.asset_id
            );
            let thumbnail_object = match self.put_bytes(&thumbnail_key, &thumbnail.bytes).await {
                Ok(stored) => stored,
                Err(error) => {
                    let _ = self.remove(&storage_key).await;
                    return Err(map_storage_error(error));
                }
            };
            (
                Some(thumbnail.original_width),
                Some(thumbnail.original_height),
                vec![StoredDerivative {
                    role: AssetFileRole::Thumbnail,
                    storage_key: thumbnail_key,
                    detected_mime_type: "image/webp".to_owned(),
                    byte_size: thumbnail_object.size,
                    sha256: thumbnail_object.sha256,
                    width: Some(thumbnail.width),
                    height: Some(thumbnail.height),
                    generator_version: Some("pah-image-webp-v1".to_owned()),
                }],
            )
        } else {
            (None, None, Vec::new())
        };

        Ok(StoredBlob {
            storage_key,
            detected_mime_type: mime_type.to_owned(),
            byte_size: stored.size,
            sha256: stored.sha256,
            width,
            height,
            duration_ms: None,
            derivatives,
        })
    }

    async fn delete(&self, tenant_id: TenantId, storage_key: &str) -> Result<(), BlobStoreError> {
        let expected_prefix = format!("tenants/{tenant_id}/");
        if !storage_key.starts_with(&expected_prefix) {
            return Err(BlobStoreError::InvalidBlob(
                "storage key is outside the active tenant".to_owned(),
            ));
        }
        self.remove(storage_key).await.map_err(map_storage_error)
    }
}

struct GeneratedThumbnail {
    bytes: Vec<u8>,
    original_width: u32,
    original_height: u32,
    width: u32,
    height: u32,
}

fn create_thumbnail(bytes: &[u8]) -> Result<GeneratedThumbnail, BlobStoreError> {
    let image = image::load_from_memory(bytes).map_err(|error| {
        BlobStoreError::InvalidBlob(format!("image cannot be decoded safely: {error}"))
    })?;
    let (original_width, original_height) = image.dimensions();
    let thumbnail = image.thumbnail(480, 480);
    let (width, height) = thumbnail.dimensions();
    let mut output = Vec::new();
    thumbnail
        .write_to(&mut Cursor::new(&mut output), ImageFormat::WebP)
        .map_err(|error| BlobStoreError::Other(format!("cannot encode WebP thumbnail: {error}")))?;
    Ok(GeneratedThumbnail {
        bytes: output,
        original_width,
        original_height,
        width,
        height,
    })
}

#[async_trait]
impl BlobStore for LocalBlobStore {
    async fn put_bytes(&self, key: &str, bytes: &[u8]) -> Result<StoredObject, LocalStorageError> {
        let destination = self.resolve(key)?;
        if let Some(parent) = destination.parent() {
            fs::create_dir_all(parent).await?;
        }

        let digest = hex_digest(bytes);
        let temporary = self.temporary_root.join(format!("{digest}.part"));
        let mut file = fs::File::create(&temporary).await?;
        file.write_all(bytes).await?;
        file.sync_all().await?;
        drop(file);

        if fs::try_exists(&destination).await? {
            fs::remove_file(&destination).await?;
        }
        fs::rename(&temporary, &destination).await?;

        Ok(StoredObject {
            key: key.to_owned(),
            size: bytes.len() as u64,
            sha256: digest,
        })
    }

    async fn read_bytes(&self, key: &str) -> Result<Vec<u8>, LocalStorageError> {
        let path = self.resolve(key)?;
        match fs::read(path).await {
            Ok(bytes) => Ok(bytes),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                Err(LocalStorageError::NotFound(key.to_owned()))
            }
            Err(error) => Err(error.into()),
        }
    }

    async fn remove(&self, key: &str) -> Result<(), LocalStorageError> {
        let path = self.resolve(key)?;
        match fs::remove_file(path).await {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error.into()),
        }
    }

    async fn exists(&self, key: &str) -> Result<bool, LocalStorageError> {
        Ok(fs::try_exists(self.resolve(key)?).await?)
    }
}

fn hex_digest(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    format!("{digest:x}")
}

fn extension_for_mime(mime_type: &str) -> &'static str {
    match mime_type {
        "image/avif" => "avif",
        "image/gif" => "gif",
        "image/jpeg" => "jpg",
        "image/png" => "png",
        "image/webp" => "webp",
        "video/mp4" => "mp4",
        "video/quicktime" => "mov",
        "video/webm" => "webm",
        _ => "bin",
    }
}

fn detect_mime_type(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        return Some("image/png");
    }
    if bytes.starts_with(&[0xff, 0xd8, 0xff]) {
        return Some("image/jpeg");
    }
    if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        return Some("image/gif");
    }
    if bytes.len() >= 12 && bytes.starts_with(b"RIFF") && &bytes[8..12] == b"WEBP" {
        return Some("image/webp");
    }
    if bytes.starts_with(&[0x1a, 0x45, 0xdf, 0xa3]) {
        return Some("video/webm");
    }
    if bytes.len() >= 12 && &bytes[4..8] == b"ftyp" {
        let brand = &bytes[8..12];
        if brand == b"avif" || brand == b"avis" {
            return Some("image/avif");
        }
        if brand == b"qt  " {
            return Some("video/quicktime");
        }
        return Some("video/mp4");
    }
    None
}

fn map_storage_error(error: LocalStorageError) -> BlobStoreError {
    match error {
        LocalStorageError::NotFound(key) => BlobStoreError::StagedBlobNotFound(key),
        LocalStorageError::UnsafeKey(message) => BlobStoreError::InvalidBlob(message),
        LocalStorageError::Io(error) => BlobStoreError::Unavailable(error.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use pah_application::StoreBlobRequest;
    use pah_domain::{AssetId, TenantId};

    use super::{BlobStore, LocalBlobStore, LocalStorageError};

    fn workspace_tempdir() -> tempfile::TempDir {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../.local/test");
        std::fs::create_dir_all(&root).expect("workspace test directory");
        tempfile::Builder::new()
            .prefix("storage-")
            .tempdir_in(root)
            .expect("temporary directory")
    }

    #[tokio::test]
    async fn writes_reads_and_removes_an_object() {
        let temp = workspace_tempdir();
        let store = LocalBlobStore::open(temp.path()).await.expect("open store");

        let stored = store
            .put_bytes("tenants/local/assets/asset-1/original.txt", b"prompt asset")
            .await
            .expect("put object");

        assert_eq!(stored.size, 12);
        assert!(store.exists(&stored.key).await.expect("object existence"));
        assert_eq!(
            store.read_bytes(&stored.key).await.expect("read object"),
            b"prompt asset"
        );
        store.remove(&stored.key).await.expect("remove object");
        assert!(!store.exists(&stored.key).await.expect("removed object"));
    }

    #[tokio::test]
    async fn rejects_path_traversal() {
        let temp = workspace_tempdir();
        let store = LocalBlobStore::open(temp.path()).await.expect("open store");

        let error = store
            .put_bytes("../outside.txt", b"no")
            .await
            .expect_err("unsafe key rejected");

        assert!(matches!(error, LocalStorageError::UnsafeKey(_)));
    }

    #[tokio::test]
    async fn staged_import_uses_tenant_scoped_immutable_key() {
        let temp = workspace_tempdir();
        let store = LocalBlobStore::open(temp.path()).await.expect("open store");
        let mut png = Vec::new();
        image::DynamicImage::new_rgba8(1, 1)
            .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
            .expect("encode test PNG");
        let staged = store
            .stage_bytes(&png, Some("image.png".into()), "image/png")
            .await
            .expect("stage bytes");
        let tenant_id = TenantId::new();
        let asset_id = AssetId::new();

        let stored = pah_application::BlobStore::store(
            &store,
            tenant_id,
            StoreBlobRequest { asset_id, staged },
        )
        .await
        .expect("store staged blob");

        assert_eq!(
            stored.storage_key,
            format!("tenants/{tenant_id}/assets/{asset_id}/original.png")
        );
        assert!(
            store
                .exists(&stored.storage_key)
                .await
                .expect("stored file")
        );
        assert_eq!(stored.width, Some(1));
        assert_eq!(stored.height, Some(1));
        assert_eq!(stored.derivatives.len(), 1);
        assert!(
            store
                .exists(&stored.derivatives[0].storage_key)
                .await
                .expect("stored thumbnail")
        );
    }
}
