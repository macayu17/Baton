//! Local capture, immutable CAS, and isolated forensic restoration.

use baton_protocol::*;
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
};
mod git;
use git::*;

const BLOB_LIMIT: u64 = 16 * 1024 * 1024;
const MANIFEST_LIMIT: u64 = 8 * 1024 * 1024;
const SNAPSHOT_LIMIT: u64 = 256 * 1024 * 1024;
const FILE_LIMIT: usize = 10_000;
const RECEIPT_LIMIT: u64 = 65_536;
fn err(code: ErrorCode) -> Diagnostic {
    Diagnostic::new(code)
}
fn digest(data: &[u8]) -> String {
    blake3::hash(data).to_hex().to_string()
}
fn hex(value: &str, len: usize) -> bool {
    value.len() == len
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
fn encode_blob(data: &[u8]) -> Result<(BlobRef, Vec<u8>), Diagnostic> {
    if data.len() as u64 > BLOB_LIMIT {
        return Err(err(ErrorCode::UnsupportedRepository));
    }
    let compressed = if data.len() >= 1024 {
        zstd::stream::encode_all(data, 1).map_err(|_| err(ErrorCode::Io))?
    } else {
        Vec::new()
    };
    let (encoding, stored) = if !compressed.is_empty() && compressed.len() < data.len() {
        (BlobEncoding::Zstd, compressed)
    } else {
        (BlobEncoding::Plain, data.to_vec())
    };
    Ok((
        BlobRef {
            hash: digest(data),
            bytes: data.len() as u64,
            encoding,
        },
        stored,
    ))
}
fn read_bounded(path: &Path, max: u64) -> Result<Vec<u8>, Diagnostic> {
    let mut bytes = Vec::new();
    fs::File::open(path)
        .map_err(|_| err(ErrorCode::Io))?
        .take(max + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| err(ErrorCode::Io))?;
    if bytes.len() as u64 > max {
        return Err(err(ErrorCode::IntegrityFailure));
    }
    Ok(bytes)
}
fn linked(meta: &fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        meta.file_attributes() & 0x400 != 0
    }
    #[cfg(not(windows))]
    {
        meta.file_type().is_symlink()
    }
}
fn regular(path: &Path) -> Result<(), Diagnostic> {
    let meta = fs::symlink_metadata(path).map_err(|_| err(ErrorCode::IntegrityFailure))?;
    if !meta.is_file() || linked(&meta) {
        return Err(err(ErrorCode::UnsafePath));
    }
    Ok(())
}
fn single_link(path: &Path, meta: &fs::Metadata) -> Result<bool, Diagnostic> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let _ = path;
        Ok(meta.nlink() == 1)
    }
    #[cfg(windows)]
    {
        let _ = meta;
        let file = fs::File::open(path).map_err(|_| err(ErrorCode::Io))?;
        Ok(winapi_util::file::information(&file)
            .map_err(|_| err(ErrorCode::Io))?
            .number_of_links()
            == 1)
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = (path, meta);
        Err(err(ErrorCode::UnsupportedRepository))
    }
}
fn directory(path: &Path) -> Result<(), Diagnostic> {
    let meta = fs::symlink_metadata(path).map_err(|_| err(ErrorCode::Io))?;
    if !meta.is_dir() || linked(&meta) {
        return Err(err(ErrorCode::UnsafePath));
    }
    Ok(())
}
fn make_directory(path: &Path) -> Result<(), Diagnostic> {
    let builder = fs::DirBuilder::new();
    #[cfg(unix)]
    let builder = {
        use std::os::unix::fs::DirBuilderExt;
        let mut builder = builder;
        builder.mode(0o700);
        builder
    };
    builder.create(path).map_err(|_| err(ErrorCode::Io))
}
fn sync_directory(path: &Path) -> Result<(), Diagnostic> {
    #[cfg(unix)]
    {
        fs::File::open(path)
            .and_then(|f| f.sync_all())
            .map_err(|_| err(ErrorCode::Io))?;
    }
    // shortcut: Windows directory durability is unproven; keep continuation BLOCKED until an audited native flush strategy exists.
    #[cfg(not(unix))]
    let _ = path;
    Ok(())
}
fn create_file(path: &Path, data: &[u8]) -> Result<(), Diagnostic> {
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(path).map_err(|e| {
        err(if e.kind() == std::io::ErrorKind::AlreadyExists {
            ErrorCode::DestinationExists
        } else {
            ErrorCode::Io
        })
    })?;
    file.write_all(data)
        .and_then(|_| file.sync_all())
        .map_err(|_| err(ErrorCode::Io))
}

pub struct Vault {
    root: PathBuf,
}

impl Vault {
    /// The embedding engine must provide a private local directory outside Git.
    pub fn create(root: &Path) -> Result<Self, Diagnostic> {
        if !root.is_absolute() {
            return Err(err(ErrorCode::UnsafePath));
        }
        directory(root.parent().ok_or_else(|| err(ErrorCode::UnsafePath))?)?;
        make_directory(root)?;
        for name in [
            "blobs",
            "manifests",
            "staging",
            "restores",
            "receipts",
            "hooks",
        ] {
            make_directory(&root.join(name))?;
        }
        sync_directory(root)?;
        sync_directory(root.parent().unwrap())?;
        Self::open(root)
    }
    pub fn open(root: &Path) -> Result<Self, Diagnostic> {
        if !root.is_absolute() {
            return Err(err(ErrorCode::UnsafePath));
        }
        let vault = Self {
            root: fs::canonicalize(root).map_err(|_| err(ErrorCode::Io))?,
        };
        if linked(&fs::symlink_metadata(root).map_err(|_| err(ErrorCode::Io))?) {
            return Err(err(ErrorCode::UnsafePath));
        }
        vault.layout()?;
        Ok(vault)
    }
    fn layout(&self) -> Result<(), Diagnostic> {
        directory(&self.root)?;
        for name in [
            "blobs",
            "manifests",
            "staging",
            "restores",
            "receipts",
            "hooks",
        ] {
            directory(&self.root.join(name))?;
        }
        if fs::read_dir(self.hooks())
            .map_err(|_| err(ErrorCode::Io))?
            .next()
            .is_some()
        {
            return Err(err(ErrorCode::UnsafePath));
        }
        Ok(())
    }
    fn hooks(&self) -> PathBuf {
        self.root.join("hooks")
    }
    fn immutable(&self, directory: &str, hash: &str, bytes: &[u8]) -> Result<(), Diagnostic> {
        if !hex(hash, 64) {
            return Err(err(ErrorCode::IntegrityFailure));
        }
        let target = self.root.join(directory).join(hash);
        if fs::symlink_metadata(&target).is_ok() {
            regular(&target)?;
            if read_bounded(&target, BLOB_LIMIT + MANIFEST_LIMIT)? != bytes {
                return Err(err(ErrorCode::IntegrityFailure));
            }
            return Ok(());
        }
        let temporary = self
            .root
            .join("staging")
            .join(format!("{}.tmp", Id::new_v4()));
        #[cfg(test)]
        fault("before_object_write");
        create_file(&temporary, bytes)?;
        #[cfg(test)]
        fault(if directory == "blobs" {
            "after_blob_flush"
        } else {
            "after_manifest_flush"
        });
        match fs::hard_link(&temporary, &target) {
            Ok(()) => (),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                regular(&target)?;
                if read_bounded(&target, BLOB_LIMIT + MANIFEST_LIMIT)? != bytes {
                    return Err(err(ErrorCode::IntegrityFailure));
                }
            }
            Err(_) => return Err(err(ErrorCode::Io)),
        }
        #[cfg(test)]
        fault(if directory == "blobs" {
            "after_blob_link"
        } else {
            "after_manifest_link"
        });
        sync_directory(&self.root.join(directory))?;
        fs::remove_file(&temporary).map_err(|_| err(ErrorCode::Io))?;
        sync_directory(&self.root.join("staging"))?;
        regular(&target)?;
        if read_bounded(&target, BLOB_LIMIT + MANIFEST_LIMIT)? != bytes {
            return Err(err(ErrorCode::IntegrityFailure));
        }
        Ok(())
    }
    #[cfg(test)]
    fn put(&self, data: &[u8]) -> Result<BlobRef, Diagnostic> {
        let (blob, stored) = encode_blob(data)?;
        self.immutable("blobs", &blob.hash, &stored)?;
        self.blob(&blob)?;
        Ok(blob)
    }
    fn blob(&self, blob: &BlobRef) -> Result<Vec<u8>, Diagnostic> {
        if !hex(&blob.hash, 64) || blob.bytes > BLOB_LIMIT {
            return Err(err(ErrorCode::IntegrityFailure));
        }
        let path = self.root.join("blobs").join(&blob.hash);
        regular(&path)?;
        let stored =
            read_bounded(&path, BLOB_LIMIT + 1).map_err(|_| err(ErrorCode::IntegrityFailure))?;
        let bytes = match blob.encoding {
            BlobEncoding::Plain => stored,
            BlobEncoding::Zstd => {
                let mut decoder = zstd::stream::read::Decoder::new(stored.as_slice())
                    .map_err(|_| err(ErrorCode::IntegrityFailure))?;
                decoder
                    .window_log_max(24)
                    .map_err(|_| err(ErrorCode::IntegrityFailure))?;
                let mut bytes = Vec::new();
                decoder
                    .take(blob.bytes + 1)
                    .read_to_end(&mut bytes)
                    .map_err(|_| err(ErrorCode::IntegrityFailure))?;
                bytes
            }
        };
        if bytes.len() as u64 != blob.bytes || digest(&bytes) != blob.hash {
            return Err(err(ErrorCode::IntegrityFailure));
        }
        Ok(bytes)
    }
    pub fn capture(
        &self,
        source: &Path,
        binding: SnapshotBinding,
        included: &[String],
    ) -> Result<String, Diagnostic> {
        self.capture_path(source, binding, included, None)
    }
    /// Read a completed receiver as a source; its receipt is provenance, never writer authority.
    pub fn capture_restored(
        &self,
        report: &RestoreReport,
        request_fingerprint: &str,
        binding: SnapshotBinding,
        included: &[String],
    ) -> Result<String, Diagnostic> {
        self.verify_receiver_identity(report, request_fingerprint)?;
        let original = self.manifest(&report.manifest_hash)?;
        if binding.task_id != original.binding.task_id
            || binding.repository_id != original.binding.repository_id
        {
            return Err(err(ErrorCode::IntegrityFailure));
        }
        if Some(binding.epoch) != original.binding.epoch.checked_add(1) {
            return Err(err(ErrorCode::StaleEpoch));
        }
        self.capture_path(
            Path::new(&report.worktree),
            binding,
            included,
            Some(report.restore_id),
        )
    }
    fn capture_path(
        &self,
        source: &Path,
        binding: SnapshotBinding,
        included: &[String],
        owned_restore: Option<Id>,
    ) -> Result<String, Diagnostic> {
        self.layout()?;
        valid_binding(&binding)?;
        let source = source_root(source, &self.hooks())?;
        if self.root.starts_with(&source)
            || (source.starts_with(&self.root) && owned_restore.is_none())
            || owned_restore
                .is_some_and(|id| source != self.root.join("restores").join(id.to_string()))
        {
            return Err(err(ErrorCode::UnsafePath));
        }
        let first = scan(&source, &self.hooks(), included)?;
        #[cfg(test)]
        fault("between_scans");
        let second = scan(&source, &self.hooks(), included)?;
        if first != second {
            return Err(err(ErrorCode::SourceChanged));
        }
        drop(second);
        let reasons = reasons(&first.gaps);
        let mut manifest = SnapshotManifest {
            schema_version: SchemaVersion::V0_1_0,
            policy: CapturePolicy::LocalDefaultV1,
            binding,
            base_commit: first.base,
            object_format: first.format,
            files: Vec::new(),
            gaps: first.gaps,
            reasons,
        };
        // The staging manifest lists every required object before any CAS publication.
        let mut objects = BTreeMap::new();
        for file in &first.files {
            let mut descriptor = |version: &RawVersion| -> Result<FileVersion, Diagnostic> {
                let hash = digest(&version.data);
                let (blob, _) = match objects.entry(hash) {
                    std::collections::btree_map::Entry::Occupied(entry) => entry.into_mut(),
                    std::collections::btree_map::Entry::Vacant(entry) => {
                        entry.insert(encode_blob(&version.data)?)
                    }
                };
                Ok(FileVersion {
                    mode: version.mode,
                    blob: blob.clone(),
                })
            };
            manifest.files.push(SnapshotFile {
                path: file.path.clone(),
                index: file.index.as_ref().map(&mut descriptor).transpose()?,
                index_oid: file.index_oid.clone(),
                worktree: file.worktree.as_ref().map(&mut descriptor).transpose()?,
            });
        }
        drop(first.files);
        validate_manifest(&manifest)?;
        let json = serde_json::to_vec(&manifest).map_err(|_| err(ErrorCode::InvalidInput))?;
        if json.len() as u64 > MANIFEST_LIMIT {
            return Err(err(ErrorCode::UnsupportedRepository));
        }
        let hash = digest(&json);
        let stage = self
            .root
            .join("staging")
            .join(format!("{}.json", manifest.binding.operation_id));
        if fs::symlink_metadata(&stage).is_ok() {
            regular(&stage)?;
            if read_bounded(&stage, MANIFEST_LIMIT)? != json {
                return Err(err(ErrorCode::SourceChanged));
            }
        } else {
            create_file(&stage, &json)?;
            sync_directory(&self.root.join("staging"))?;
        }
        #[cfg(test)]
        fault("after_staging_manifest");
        for (blob, stored) in objects.values() {
            self.immutable("blobs", &blob.hash, stored)?;
            self.blob(blob)?;
        }
        #[cfg(test)]
        fault("before_manifest_write");
        self.immutable("manifests", &hash, &json)?;
        self.manifest(&hash)?;
        Ok(hash)
    }
    /// Validates schema, paths, all required uncompressed hashes and byte lengths.
    pub fn manifest(&self, hash: &str) -> Result<SnapshotManifest, Diagnostic> {
        self.layout()?;
        if !hex(hash, 64) {
            return Err(err(ErrorCode::IntegrityFailure));
        }
        let path = self.root.join("manifests").join(hash);
        regular(&path)?;
        let json =
            read_bounded(&path, MANIFEST_LIMIT).map_err(|_| err(ErrorCode::IntegrityFailure))?;
        if digest(&json) != hash {
            return Err(err(ErrorCode::IntegrityFailure));
        }
        let manifest: SnapshotManifest =
            serde_json::from_slice(&json).map_err(|_| err(ErrorCode::IntegrityFailure))?;
        validate_manifest(&manifest)?;
        for file in &manifest.files {
            for version in file.index.iter().chain(file.worktree.iter()) {
                let bytes = self.blob(&version.blob)?;
                if sensitive_bytes(&bytes) {
                    return Err(err(ErrorCode::IntegrityFailure));
                }
            }
        }
        Ok(manifest)
    }
    pub fn restore(
        &self,
        source: &Path,
        hash: &str,
        restore_id: Id,
    ) -> Result<RestoreReport, Diagnostic> {
        let manifest = self.manifest(hash)?;
        if restore_id.is_nil() {
            return Err(err(ErrorCode::InvalidInput));
        }
        let source = source_root(source, &self.hooks())?;
        let format = git(
            &source,
            &self.hooks(),
            &["rev-parse", "--show-object-format"],
            None,
        )?;
        if line(&format)?
            != match manifest.object_format {
                GitObjectFormat::Sha1 => "sha1",
                GitObjectFormat::Sha256 => "sha256",
            }
        {
            return Err(err(ErrorCode::UnsupportedRepository));
        }
        git(
            &source,
            &self.hooks(),
            &[
                "cat-file",
                "-e",
                &format!("{}^{{commit}}", manifest.base_commit),
            ],
            None,
        )?;
        let target = self.root.join("restores").join(restore_id.to_string());
        if fs::symlink_metadata(&target).is_ok() {
            return Err(err(ErrorCode::DestinationExists));
        }
        make_directory(&target)?;
        sync_directory(&self.root.join("restores"))?;
        let target_text = target.to_str().ok_or_else(|| err(ErrorCode::UnsafePath))?;
        git(
            &source,
            &self.hooks(),
            &[
                "worktree",
                "add",
                "--no-checkout",
                "--detach",
                "--lock",
                &git_path(&target)?,
                &manifest.base_commit,
            ],
            None,
        )?;
        #[cfg(test)]
        fault("after_worktree_add");
        let mut index = Vec::new();
        for file in &manifest.files {
            if let Some(version) = &file.index {
                let bytes = self.blob(&version.blob)?;
                let oid = git(
                    &target,
                    &self.hooks(),
                    &["hash-object", "-w", "--stdin", "--no-filters"],
                    Some(&bytes),
                )?;
                if Some(line(&oid)?) != file.index_oid.as_deref() {
                    return Err(err(ErrorCode::IntegrityFailure));
                }
                index.extend_from_slice(
                    format!(
                        "{} {}\t{}\0",
                        mode_text(version.mode),
                        line(&oid)?,
                        file.path
                    )
                    .as_bytes(),
                );
            }
            if let Some(version) = &file.worktree {
                let mut parent = target.clone();
                let parts: Vec<_> = file.path.split('/').collect();
                for part in &parts[..parts.len() - 1] {
                    parent.push(part);
                    if !parent.exists() {
                        make_directory(&parent)?;
                    }
                    directory(&parent)?;
                }
                let path = target.join(&file.path);
                create_file(&path, &self.blob(&version.blob)?)?;
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    fs::set_permissions(
                        &path,
                        fs::Permissions::from_mode(if version.mode == FileMode::Executable {
                            0o700
                        } else {
                            0o600
                        }),
                    )
                    .map_err(|_| err(ErrorCode::Io))?;
                    fs::File::open(&path)
                        .and_then(|f| f.sync_all())
                        .map_err(|_| err(ErrorCode::Io))?;
                }
                sync_directory(&parent)?;
            }
        }
        git(
            &target,
            &self.hooks(),
            &["update-index", "-z", "--index-info"],
            Some(&index),
        )?;
        let report = RestoreReport {
            schema_version: SchemaVersion::V0_1_0,
            checkpoint_id: manifest.binding.checkpoint_id,
            restore_id,
            manifest_hash: hash.into(),
            worktree: target_text.into(),
            quality: RecoveryQuality::Blocked,
            reasons: manifest.reasons.clone(),
        };
        self.verify_restore(&source, hash, &report)?;
        Ok(report)
    }
    /// Trusted engine use after durable intent. Receipts never confer writer authority.
    pub fn restore_recoverable(
        &self,
        source: &Path,
        hash: &str,
        restore_id: Id,
        request_fingerprint: &str,
    ) -> Result<RestoreReport, Diagnostic> {
        self.layout()?;
        if restore_id.is_nil() || !hex(request_fingerprint, 64) {
            return Err(err(ErrorCode::InvalidInput));
        }
        // The key locates an immutable operation receipt; it is not a content hash.
        let key = digest(restore_id.as_bytes());
        let path = self.root.join("receipts").join(&key);
        match fs::symlink_metadata(&path) {
            Ok(_) => {
                let receipt = self.completed_receipt(restore_id, request_fingerprint)?;
                self.verify_restore(source, hash, &receipt.report)?;
                return Ok(receipt.report);
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => (),
            Err(_) => return Err(err(ErrorCode::Io)),
        }
        let report = self.restore(source, hash, restore_id)?;
        #[cfg(test)]
        fault("before_restore_receipt");
        let receipt = RestoreReceipt {
            schema_version: SchemaVersion::V0_1_0,
            format_version: 2,
            git_dir: Some(self.worktree_git_dir(Path::new(&report.worktree))?),
            request_fingerprint: request_fingerprint.into(),
            report: report.clone(),
        };
        let bytes = serde_json::to_vec(&receipt).map_err(|_| err(ErrorCode::InvalidInput))?;
        if bytes.len() as u64 > RECEIPT_LIMIT {
            return Err(err(ErrorCode::UnsupportedRepository));
        }
        self.immutable("receipts", &key, &bytes)?;
        #[cfg(test)]
        fault("after_restore_receipt");
        Ok(report)
    }

    fn worktree_git_dir(&self, target: &Path) -> Result<String, Diagnostic> {
        let bytes = git(
            target,
            &self.hooks(),
            &["rev-parse", "--absolute-git-dir"],
            None,
        )?;
        fs::canonicalize(line(&bytes)?)
            .map_err(|_| err(ErrorCode::Io))?
            .to_str()
            .map(str::to_owned)
            .ok_or_else(|| err(ErrorCode::UnsupportedRepository))
    }
    fn completed_receipt(
        &self,
        restore_id: Id,
        request_fingerprint: &str,
    ) -> Result<RestoreReceipt, Diagnostic> {
        if restore_id.is_nil() || !hex(request_fingerprint, 64) {
            return Err(err(ErrorCode::InvalidInput));
        }
        self.layout()?;
        let path = self
            .root
            .join("receipts")
            .join(digest(restore_id.as_bytes()));
        regular(&path)?;
        let receipt: RestoreReceipt = serde_json::from_slice(&read_bounded(&path, RECEIPT_LIMIT)?)
            .map_err(|_| err(ErrorCode::IntegrityFailure))?;
        if receipt.format_version != 2 || receipt.git_dir.is_none() {
            return Err(err(ErrorCode::UnsupportedSchema));
        }
        if receipt.request_fingerprint != request_fingerprint {
            return Err(err(ErrorCode::IdempotencyConflict));
        }
        let target = self.root.join("restores").join(restore_id.to_string());
        if receipt.report.restore_id != restore_id || Path::new(&receipt.report.worktree) != target
        {
            return Err(err(ErrorCode::IntegrityFailure));
        }
        directory(&target)?;
        if source_root(&target, &self.hooks())? != target
            || Some(self.worktree_git_dir(&target)?) != receipt.git_dir
        {
            return Err(err(ErrorCode::IntegrityFailure));
        }
        Ok(receipt)
    }
    pub fn verify_receiver_identity(
        &self,
        report: &RestoreReport,
        request_fingerprint: &str,
    ) -> Result<(), Diagnostic> {
        if self
            .completed_receipt(report.restore_id, request_fingerprint)?
            .report
            != *report
        {
            return Err(err(ErrorCode::IntegrityFailure));
        }
        Ok(())
    }
    pub fn verify_restored_source(
        &self,
        report: &RestoreReport,
        request_fingerprint: &str,
        hash: &str,
    ) -> Result<(), Diagnostic> {
        let manifest = self.manifest(hash)?;
        let untracked: Vec<_> = manifest
            .files
            .iter()
            .filter(|file| file.index.is_none() && file.worktree.is_some())
            .map(|file| file.path.clone())
            .collect();
        if self.capture_restored(report, request_fingerprint, manifest.binding, &untracked)? != hash
        {
            return Err(err(ErrorCode::SourceChanged));
        }
        Ok(())
    }

    /// Re-capture into private CAS for comparison; never modifies source worktree bytes.
    pub fn verify_source(&self, source: &Path, hash: &str) -> Result<(), Diagnostic> {
        let manifest = self.manifest(hash)?;
        let untracked: Vec<_> = manifest
            .files
            .iter()
            .filter(|file| file.index.is_none() && file.worktree.is_some())
            .map(|file| file.path.clone())
            .collect();
        if self.capture(source, manifest.binding, &untracked)? != hash {
            return Err(err(ErrorCode::SourceChanged));
        }
        Ok(())
    }

    pub fn verify_restore(
        &self,
        source: &Path,
        hash: &str,
        report: &RestoreReport,
    ) -> Result<(), Diagnostic> {
        let manifest = self.manifest(hash)?;
        let target = self
            .root
            .join("restores")
            .join(report.restore_id.to_string());
        directory(&target)?;
        if report.manifest_hash != hash
            || report.checkpoint_id != manifest.binding.checkpoint_id
            || report.quality != RecoveryQuality::Blocked
            || report.reasons != manifest.reasons
            || Path::new(&report.worktree) != target
        {
            return Err(err(ErrorCode::IntegrityFailure));
        }
        let actual_root = source_root(&target, &self.hooks())?;
        if actual_root != target {
            return Err(err(ErrorCode::UnsafePath));
        }
        let source_common = git(
            source,
            &self.hooks(),
            &["rev-parse", "--path-format=absolute", "--git-common-dir"],
            None,
        )?;
        let target_common = git(
            &target,
            &self.hooks(),
            &["rev-parse", "--path-format=absolute", "--git-common-dir"],
            None,
        )?;
        if fs::canonicalize(line(&source_common)?).map_err(|_| err(ErrorCode::Io))?
            != fs::canonicalize(line(&target_common)?).map_err(|_| err(ErrorCode::Io))?
        {
            return Err(err(ErrorCode::IntegrityFailure));
        }
        if line(&git(&target, &self.hooks(), &["rev-parse", "HEAD"], None)?)?
            != manifest.base_commit
        {
            return Err(err(ErrorCode::IntegrityFailure));
        }
        let actual_index = entries(
            &git(&target, &self.hooks(), &["ls-files", "--stage", "-z"], None)?,
            true,
        )?;
        let mut working_names = BTreeSet::new();
        let mut indexed = 0usize;
        for file in &manifest.files {
            if let Some(version) = &file.index {
                indexed += 1;
                let entry = actual_index
                    .get(&file.path)
                    .ok_or_else(|| err(ErrorCode::IntegrityFailure))?;
                let bytes = git(
                    &target,
                    &self.hooks(),
                    &["cat-file", "blob", &entry.oid],
                    None,
                )?;
                if entry.mode != mode_text(version.mode)
                    || Some(entry.oid.as_str()) != file.index_oid.as_deref()
                    || bytes != self.blob(&version.blob)?
                {
                    return Err(err(ErrorCode::IntegrityFailure));
                }
            }
            if let Some(version) = &file.worktree {
                let meta = walk_file(&target, &file.path)?
                    .ok_or_else(|| err(ErrorCode::IntegrityFailure))?;
                if !meta.is_file()
                    || !single_link(&target.join(&file.path), &meta)?
                    || read_bounded(&target.join(&file.path), BLOB_LIMIT)?
                        != self.blob(&version.blob)?
                {
                    return Err(err(ErrorCode::IntegrityFailure));
                }
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    if (meta.permissions().mode() & 0o111 != 0)
                        != (version.mode == FileMode::Executable)
                    {
                        return Err(err(ErrorCode::IntegrityFailure));
                    }
                }
                working_names.insert(file.path.clone());
            }
        }
        if actual_index.len() != indexed || list_working(&target, &target)? != working_names {
            return Err(err(ErrorCode::IntegrityFailure));
        }
        Ok(())
    }
    /// Retains every published, unpublished, and staged manifest; no live retention.
    pub fn collect_orphans(&self, published: &[String]) -> Result<u64, Diagnostic> {
        self.layout()?;
        let mut keep = BTreeSet::new();
        for hash in published {
            let manifest = self.manifest(hash)?;
            add_references(&manifest, &mut keep);
        }
        for item in fs::read_dir(self.root.join("manifests")).map_err(|_| err(ErrorCode::Io))? {
            let item = item.map_err(|_| err(ErrorCode::Io))?;
            let hash = item
                .file_name()
                .to_str()
                .ok_or_else(|| err(ErrorCode::IntegrityFailure))?
                .to_owned();
            let manifest = self.manifest(&hash)?;
            add_references(&manifest, &mut keep);
        }
        for item in fs::read_dir(self.root.join("staging")).map_err(|_| err(ErrorCode::Io))? {
            let item = item.map_err(|_| err(ErrorCode::Io))?;
            if item.path().extension().is_some_and(|ext| ext == "json") {
                regular(&item.path())?;
                let bytes = read_bounded(&item.path(), MANIFEST_LIMIT)?;
                let manifest: SnapshotManifest =
                    serde_json::from_slice(&bytes).map_err(|_| err(ErrorCode::IntegrityFailure))?;
                validate_manifest(&manifest)?;
                add_references(&manifest, &mut keep);
            }
        }
        let mut remove = Vec::new();
        for item in fs::read_dir(self.root.join("blobs")).map_err(|_| err(ErrorCode::Io))? {
            let item = item.map_err(|_| err(ErrorCode::Io))?;
            regular(&item.path())?;
            let hash = item
                .file_name()
                .to_str()
                .ok_or_else(|| err(ErrorCode::IntegrityFailure))?
                .to_owned();
            if !hex(&hash, 64) {
                return Err(err(ErrorCode::IntegrityFailure));
            }
            if !keep.contains(&hash) {
                remove.push(item.path());
            }
        }
        let count = remove.len() as u64;
        for path in remove {
            fs::remove_file(path).map_err(|_| err(ErrorCode::Io))?;
        }
        sync_directory(&self.root.join("blobs"))?;
        Ok(count)
    }
}
fn reasons(gaps: &[CaptureGap]) -> Vec<CheckpointReason> {
    let mut reasons = vec![CheckpointReason::SupervisionUnavailable];
    if !gaps.is_empty() {
        reasons.push(CheckpointReason::CaptureGaps);
    }
    if cfg!(not(unix)) {
        reasons.push(CheckpointReason::DurabilityUnproven);
    }
    reasons
}
fn valid_binding(binding: &SnapshotBinding) -> Result<(), Diagnostic> {
    if [
        binding.checkpoint_id,
        binding.repository_id,
        binding.task_id,
        binding.actor_id,
        binding.operation_id,
    ]
    .iter()
    .any(Id::is_nil)
        || binding.epoch == 0
        || binding.epoch > i64::MAX as u64
        || binding.event_cursor == 0
        || binding.parent_id == Some(binding.checkpoint_id)
    {
        return Err(err(ErrorCode::InvalidInput));
    }
    Ok(())
}
fn validate_manifest(manifest: &SnapshotManifest) -> Result<(), Diagnostic> {
    valid_binding(&manifest.binding).map_err(|_| err(ErrorCode::IntegrityFailure))?;
    let len = match manifest.object_format {
        GitObjectFormat::Sha1 => 40,
        GitObjectFormat::Sha256 => 64,
    };
    if !hex(&manifest.base_commit, len)
        || manifest.files.len() + manifest.gaps.len() > FILE_LIMIT * 2
        || !manifest
            .reasons
            .contains(&CheckpointReason::SupervisionUnavailable)
        || manifest.reasons.contains(&CheckpointReason::CaptureGaps) == manifest.gaps.is_empty()
    {
        return Err(err(ErrorCode::IntegrityFailure));
    }
    let mut paths = BTreeSet::new();
    let mut working = BTreeSet::new();
    let mut indexing = BTreeSet::new();
    let mut total = 0u64;
    for file in &manifest.files {
        let key = portable(&file.path)?;
        if !paths.insert(key.clone()) {
            return Err(err(ErrorCode::UnsafePath));
        }
        if sensitive_path(&file.path) {
            return Err(err(ErrorCode::IntegrityFailure));
        }
        if file.index.is_some() != file.index_oid.is_some()
            || file.index_oid.as_ref().is_some_and(|oid| !hex(oid, len))
        {
            return Err(err(ErrorCode::IntegrityFailure));
        }
        if file.index.is_some() {
            indexing.insert(key.clone());
        }
        if file.worktree.is_some() {
            working.insert(key);
        }
        for version in file.index.iter().chain(file.worktree.iter()) {
            if !hex(&version.blob.hash, 64) || version.blob.bytes > BLOB_LIMIT {
                return Err(err(ErrorCode::IntegrityFailure));
            }
            total = total
                .checked_add(version.blob.bytes)
                .ok_or_else(|| err(ErrorCode::IntegrityFailure))?;
        }
    }
    for names in [working, indexing] {
        for path in &names {
            let mut prefix = String::new();
            for part in path.split('/').take(path.split('/').count() - 1) {
                if !prefix.is_empty() {
                    prefix.push('/');
                }
                prefix.push_str(part);
                if names.contains(&prefix) {
                    return Err(err(ErrorCode::UnsafePath));
                }
            }
        }
    }
    for gap in &manifest.gaps {
        let key = portable(&gap.path)?;
        if gap.reason != GapReason::ExecutableMetadata && paths.contains(&key) {
            return Err(err(ErrorCode::IntegrityFailure));
        }
    }
    if total > SNAPSHOT_LIMIT {
        return Err(err(ErrorCode::IntegrityFailure));
    }
    Ok(())
}
fn add_references(manifest: &SnapshotManifest, keep: &mut BTreeSet<String>) {
    for file in &manifest.files {
        for version in file.index.iter().chain(file.worktree.iter()) {
            keep.insert(version.blob.hash.clone());
        }
    }
}
fn list_working(root: &Path, current: &Path) -> Result<BTreeSet<String>, Diagnostic> {
    let mut result = BTreeSet::new();
    for entry in fs::read_dir(current).map_err(|_| err(ErrorCode::Io))? {
        let entry = entry.map_err(|_| err(ErrorCode::Io))?;
        if current == root && entry.file_name() == ".git" {
            continue;
        }
        let meta = fs::symlink_metadata(entry.path()).map_err(|_| err(ErrorCode::Io))?;
        if linked(&meta) {
            return Err(err(ErrorCode::UnsafePath));
        }
        if meta.is_dir() {
            result.extend(list_working(root, &entry.path())?);
        } else if meta.is_file() {
            result.insert(
                entry
                    .path()
                    .strip_prefix(root)
                    .map_err(|_| err(ErrorCode::UnsafePath))?
                    .to_str()
                    .ok_or_else(|| err(ErrorCode::UnsafePath))?
                    .replace('\\', "/"),
            );
        } else {
            return Err(err(ErrorCode::UnsafePath));
        }
        if result.len() > FILE_LIMIT {
            return Err(err(ErrorCode::IntegrityFailure));
        }
    }
    Ok(result)
}

#[cfg(test)]
fn fault(point: &str) {
    if std::env::var("BATON_CAS_TEST_POINT").as_deref() == Ok(point) {
        fs::File::create(std::env::var_os("BATON_CAS_TEST_READY").unwrap())
            .unwrap()
            .sync_all()
            .unwrap();
        if let Some(release) = std::env::var_os("BATON_CAS_TEST_RELEASE") {
            while !Path::new(&release).exists() {
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
        } else {
            loop {
                std::thread::park();
            }
        }
    }
}

#[cfg(test)]
mod tests;
