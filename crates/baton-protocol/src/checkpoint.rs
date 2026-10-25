//! Immutable local snapshot contracts, separate from execution continuity.
use crate::{Id, RecoveryQuality, SchemaVersion};
use serde::{Deserialize, Serialize};

fn required_nullable<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::<T>::deserialize(deserializer)
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SnapshotBinding {
    pub checkpoint_id: Id,
    pub parent_id: Option<Id>,
    pub repository_id: Id,
    pub task_id: Id,
    pub actor_id: Id,
    pub operation_id: Id,
    pub epoch: u64,
    pub event_cursor: u64,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum BlobEncoding {
    Plain,
    Zstd,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BlobRef {
    pub hash: String,
    pub bytes: u64,
    pub encoding: BlobEncoding,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum FileMode {
    Regular,
    Executable,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FileVersion {
    pub mode: FileMode,
    pub blob: BlobRef,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SnapshotFile {
    pub path: String,
    #[serde(deserialize_with = "required_nullable")]
    pub index: Option<FileVersion>,
    #[serde(deserialize_with = "required_nullable")]
    pub index_oid: Option<String>,
    #[serde(deserialize_with = "required_nullable")]
    pub worktree: Option<FileVersion>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum GapReason {
    Ignored,
    NotIncluded,
    Sensitive,
    UnsupportedLink,
    Submodule,
    LfsPointer,
    Oversized,
    ExecutableMetadata,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CaptureGap {
    pub path: String,
    pub reason: GapReason,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum CheckpointReason {
    SupervisionUnavailable,
    CaptureGaps,
    DurabilityUnproven,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CapturePolicy {
    #[serde(rename = "local-default-v1")]
    LocalDefaultV1,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum GitObjectFormat {
    Sha1,
    Sha256,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SnapshotManifest {
    pub schema_version: SchemaVersion,
    pub policy: CapturePolicy,
    pub binding: SnapshotBinding,
    pub base_commit: String,
    pub object_format: GitObjectFormat,
    pub files: Vec<SnapshotFile>,
    pub gaps: Vec<CaptureGap>,
    pub reasons: Vec<CheckpointReason>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CheckpointRecord {
    pub schema_version: SchemaVersion,
    pub binding: SnapshotBinding,
    pub manifest_hash: String,
    pub quality: RecoveryQuality,
    pub reasons: Vec<CheckpointReason>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RestoreReport {
    pub schema_version: SchemaVersion,
    pub checkpoint_id: Id,
    pub restore_id: Id,
    pub manifest_hash: String,
    pub worktree: String,
    pub quality: RecoveryQuality,
    pub reasons: Vec<CheckpointReason>,
}

/// Immutable local completion evidence; never a permission or launch grant.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RestoreReceipt {
    pub schema_version: SchemaVersion,
    #[serde(default)]
    pub format_version: u32,
    #[serde(default)]
    pub git_dir: Option<String>,
    pub request_fingerprint: String,
    pub report: RestoreReport,
}
