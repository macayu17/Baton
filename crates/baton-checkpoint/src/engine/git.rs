use super::*;
use std::{
    collections::BTreeMap,
    io::{Read, Write},
    process::{Command, Stdio},
};
use unicode_normalization::UnicodeNormalization;

const OUTPUT_LIMIT: u64 = 64 * 1024 * 1024;

pub(super) fn git_path(path: &Path) -> Result<String, Diagnostic> {
    let text = path.to_str().ok_or_else(|| err(ErrorCode::UnsafePath))?;
    #[cfg(windows)]
    {
        if text.starts_with(r"\\?\UNC\") {
            return Err(err(ErrorCode::UnsafePath));
        }
        Ok(text
            .strip_prefix(r"\\?\")
            .unwrap_or(text)
            .replace('\\', "/"))
    }
    #[cfg(not(windows))]
    {
        Ok(text.to_owned())
    }
}

pub(super) fn git(
    root: &Path,
    hooks: &Path,
    args: &[&str],
    input: Option<&[u8]>,
) -> Result<Vec<u8>, Diagnostic> {
    let mut command = Command::new("git");
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000); // Background Git must not open a console.
    }
    for (key, _) in std::env::vars_os() {
        if key
            .to_string_lossy()
            .to_ascii_uppercase()
            .starts_with("GIT_")
        {
            command.env_remove(key);
        }
    }
    command
        .current_dir(root)
        .args([
            "--no-optional-locks",
            "-c",
            "core.fsmonitor=false",
            "-c",
            "core.untrackedCache=false",
            "-c",
            "core.autocrlf=false",
            "-c",
            "core.longpaths=true",
            "-c",
            "submodule.recurse=false",
        ])
        .arg("-c")
        .arg(format!("core.hooksPath={}", git_path(hooks)?))
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env(
            "GIT_CONFIG_GLOBAL",
            if cfg!(windows) { "NUL" } else { "/dev/null" },
        )
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_DISCOVERY_ACROSS_FILESYSTEM", "1")
        .env("LC_ALL", "C")
        .env("GIT_NO_LAZY_FETCH", "1")
        .args(args)
        .stdin(if input.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    let mut child = command.spawn().map_err(|e| {
        err(if e.kind() == std::io::ErrorKind::NotFound {
            ErrorCode::GitUnavailable
        } else {
            ErrorCode::Io
        })
    })?;
    if let Some(bytes) = input {
        if child.stdin.take().unwrap().write_all(bytes).is_err() {
            let _ = child.kill();
            let _ = child.wait();
            return Err(err(ErrorCode::Io));
        }
    }
    let mut output = Vec::new();
    if child
        .stdout
        .take()
        .unwrap()
        .take(OUTPUT_LIMIT + 1)
        .read_to_end(&mut output)
        .is_err()
        || output.len() as u64 > OUTPUT_LIMIT
    {
        let _ = child.kill();
        let _ = child.wait();
        return Err(err(ErrorCode::UnsupportedRepository));
    }
    if !child.wait().map_err(|_| err(ErrorCode::Io))?.success() {
        return Err(err(ErrorCode::GitFailed));
    }
    Ok(output)
}

pub(super) fn line(bytes: &[u8]) -> Result<&str, Diagnostic> {
    let value = std::str::from_utf8(bytes).map_err(|_| err(ErrorCode::UnsafePath))?;
    let value = value.strip_suffix('\n').unwrap_or(value);
    let value = value.strip_suffix('\r').unwrap_or(value);
    if value.contains(['\0', '\r', '\n']) {
        return Err(err(ErrorCode::UnsafePath));
    }
    Ok(value)
}
pub(super) fn portable(path: &str) -> Result<String, Diagnostic> {
    if path.is_empty() || path.len() > 4096 {
        return Err(err(ErrorCode::UnsafePath));
    }
    for component in path.split('/') {
        let folded: String = component.nfkc().flat_map(char::to_uppercase).collect();
        let stem = folded.split('.').next().unwrap_or("");
        if component.is_empty()
            || component == "."
            || component == ".."
            || component.ends_with([' ', '.'])
            || component
                .chars()
                .any(|c| c.is_control() || "\\:*?\"<>|".contains(c))
            || folded == ".GIT"
            || folded == "GIT~1"
            || ["CON", "PRN", "AUX", "NUL"].contains(&stem)
            || ((stem.starts_with("COM") || stem.starts_with("LPT"))
                && stem.len() == 4
                && matches!(stem.as_bytes()[3], b'1'..=b'9'))
        {
            return Err(err(ErrorCode::UnsafePath));
        }
    }
    Ok(path.nfkc().flat_map(char::to_uppercase).collect())
}
pub(super) fn paths(bytes: &[u8]) -> Result<Vec<String>, Diagnostic> {
    let mut result = Vec::new();
    for raw in bytes.split(|b| *b == 0).filter(|b| !b.is_empty()) {
        let path = std::str::from_utf8(raw).map_err(|_| err(ErrorCode::UnsafePath))?;
        portable(path)?;
        result.push(path.to_owned());
        if result.len() > FILE_LIMIT {
            return Err(err(ErrorCode::UnsupportedRepository));
        }
    }
    Ok(result)
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct GitEntry {
    pub mode: String,
    pub oid: String,
}
pub(super) fn entries(bytes: &[u8], index: bool) -> Result<BTreeMap<String, GitEntry>, Diagnostic> {
    let mut result = BTreeMap::new();
    for raw in bytes.split(|b| *b == 0).filter(|b| !b.is_empty()) {
        let text = std::str::from_utf8(raw).map_err(|_| err(ErrorCode::UnsafePath))?;
        let (header, path) = text
            .split_once('\t')
            .ok_or_else(|| err(ErrorCode::UnsupportedRepository))?;
        portable(path)?;
        let fields: Vec<_> = header.split(' ').collect();
        if fields.len() != 3 || (index && fields[2] != "0") {
            return Err(err(ErrorCode::UnsupportedRepository));
        }
        let oid = if index { fields[1] } else { fields[2] };
        if !hex(oid, 40) && !hex(oid, 64) {
            return Err(err(ErrorCode::UnsupportedRepository));
        }
        if result
            .insert(
                path.to_owned(),
                GitEntry {
                    mode: fields[0].into(),
                    oid: oid.into(),
                },
            )
            .is_some()
            || result.len() > FILE_LIMIT
        {
            return Err(err(ErrorCode::UnsupportedRepository));
        }
    }
    Ok(result)
}
pub(super) fn mode(mode: &str) -> Result<FileMode, Diagnostic> {
    match mode {
        "100644" => Ok(FileMode::Regular),
        "100755" => Ok(FileMode::Executable),
        _ => Err(err(ErrorCode::UnsupportedRepository)),
    }
}
pub(super) fn mode_text(mode: FileMode) -> &'static str {
    match mode {
        FileMode::Regular => "100644",
        FileMode::Executable => "100755",
    }
}

pub(super) fn source_root(path: &Path, hooks: &Path) -> Result<PathBuf, Diagnostic> {
    let root = fs::canonicalize(path).map_err(|_| err(ErrorCode::Io))?;
    let actual = git(
        &root,
        hooks,
        &["rev-parse", "--path-format=absolute", "--show-toplevel"],
        None,
    )?;
    let actual = fs::canonicalize(line(&actual)?).map_err(|_| err(ErrorCode::Io))?;
    if root != actual {
        return Err(err(ErrorCode::UnsafePath));
    }
    Ok(actual)
}

pub(super) fn guard(root: &Path, hooks: &Path) -> Result<(), Diagnostic> {
    let config = git(root, hooks, &["config", "--null", "--list"], None)?;
    for row in config.split(|b| *b == 0).filter(|r| !r.is_empty()) {
        let Some((key, value)) = std::str::from_utf8(row)
            .ok()
            .and_then(|r| r.split_once('\n'))
        else {
            return Err(err(ErrorCode::UnsupportedRepository));
        };
        let key = key.to_ascii_lowercase();
        let enabled = ["true", "yes", "on", "1", ""].contains(&value.to_ascii_lowercase().as_str());
        if key == "core.worktree"
            || key == "extensions.partialclone"
            || (enabled
                && ([
                    "core.sparsecheckout",
                    "core.splitindex",
                    "core.bare",
                    "extensions.worktreeconfig",
                ]
                .contains(&key.as_str())
                    || key.ends_with(".promisor")))
        {
            return Err(err(ErrorCode::UnsupportedRepository));
        }
    }
    let git_dir = git(root, hooks, &["rev-parse", "--absolute-git-dir"], None)?;
    let git_dir = Path::new(line(&git_dir)?);
    for name in [
        "index.lock",
        "MERGE_HEAD",
        "CHERRY_PICK_HEAD",
        "REVERT_HEAD",
        "rebase-merge",
        "rebase-apply",
    ] {
        if fs::symlink_metadata(git_dir.join(name)).is_ok() {
            return Err(err(ErrorCode::UnsupportedRepository));
        }
    }
    let shared = git(root, hooks, &["rev-parse", "--shared-index-path"], None)?;
    if !line(&shared)?.is_empty() {
        return Err(err(ErrorCode::UnsupportedRepository));
    }
    for row in git(root, hooks, &["ls-files", "-v", "-z"], None)?
        .split(|b| *b == 0)
        .filter(|b| !b.is_empty())
    {
        if !row.starts_with(b"H ") {
            return Err(err(ErrorCode::UnsupportedRepository));
        }
    }
    let visible = git(
        root,
        hooks,
        &[
            "diff",
            "--cached",
            "--name-only",
            "--no-ext-diff",
            "--no-textconv",
            "--ita-visible-in-index",
            "-z",
        ],
        None,
    )?;
    let invisible = git(
        root,
        hooks,
        &[
            "diff",
            "--cached",
            "--name-only",
            "--no-ext-diff",
            "--no-textconv",
            "--ita-invisible-in-index",
            "-z",
        ],
        None,
    )?;
    if visible != invisible {
        return Err(err(ErrorCode::UnsupportedRepository));
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct RawVersion {
    pub mode: FileMode,
    pub data: Vec<u8>,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct RawFile {
    pub path: String,
    pub index: Option<RawVersion>,
    pub index_oid: Option<String>,
    pub worktree: Option<RawVersion>,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Scan {
    pub base: String,
    pub format: GitObjectFormat,
    pub index_hash: String,
    pub files: Vec<RawFile>,
    pub gaps: Vec<CaptureGap>,
}

pub(super) fn sensitive_path(path: &str) -> bool {
    path.split('/').any(|part| {
        let lower = part.to_ascii_lowercase();
        lower == ".ssh"
            || lower == ".aws"
            || lower == ".env"
            || lower.starts_with(".env.")
            || lower.starts_with("credentials")
            || ["id_rsa", "id_ed25519", ".npmrc", ".pypirc"].contains(&lower.as_str())
            || [".pem", ".key", ".p12", ".pfx"]
                .iter()
                .any(|ext| lower.ends_with(ext))
    })
}
pub(super) fn sensitive_bytes(data: &[u8]) -> bool {
    if data.windows(16).any(|w| w == b"PRIVATE KEY-----") {
        return true;
    }
    [
        (b"ghp_".as_slice(), 36),
        (b"github_pat_".as_slice(), 20),
        (b"AKIA".as_slice(), 16),
        (b"sk-".as_slice(), 20),
    ]
    .iter()
    .any(|(prefix, minimum)| {
        data.windows(prefix.len() + minimum).any(|window| {
            window.starts_with(prefix)
                && window[prefix.len()..]
                    .iter()
                    .all(|b| b.is_ascii_alphanumeric() || *b == b'_' || *b == b'-')
        })
    })
}
fn lfs(data: &[u8]) -> bool {
    data.starts_with(b"version https://git-lfs.github.com/spec/v1")
}

// M2-POLICY; FR-16: recognize token-shaped values without dropping ordinary task text.
#[test]
fn policy_distinguishes_task_text_from_tokens() {
    assert!(!sensitive_bytes(b"a task-id is ordinary source text"));
    assert!(sensitive_bytes(b"sk-abcdefghijklmnopqrstuvwxyz"));
    assert!(sensitive_bytes(b"ghp_abcdefghijklmnopqrstuvwxyz0123456789"));
}

pub(super) fn walk_file(root: &Path, path: &str) -> Result<Option<fs::Metadata>, Diagnostic> {
    portable(path)?;
    let mut current = root.to_path_buf();
    let parts: Vec<_> = path.split('/').collect();
    for (i, part) in parts.iter().enumerate() {
        current.push(part);
        match fs::symlink_metadata(&current) {
            Ok(meta) if linked(&meta) => return Err(err(ErrorCode::UnsafePath)),
            Ok(meta) if i + 1 == parts.len() => return Ok(Some(meta)),
            Ok(meta) if meta.is_dir() => (),
            Ok(_) => return Err(err(ErrorCode::UnsafePath)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(_) => return Err(err(ErrorCode::Io)),
        }
    }
    Ok(None)
}

pub(super) fn scan(root: &Path, hooks: &Path, included: &[String]) -> Result<Scan, Diagnostic> {
    guard(root, hooks)?;
    let base = line(&git(
        root,
        hooks,
        &["rev-parse", "--verify", "HEAD^{commit}"],
        None,
    )?)?
    .to_owned();
    let format = match line(&git(
        root,
        hooks,
        &["rev-parse", "--show-object-format"],
        None,
    )?)? {
        "sha1" => GitObjectFormat::Sha1,
        "sha256" => GitObjectFormat::Sha256,
        _ => return Err(err(ErrorCode::UnsupportedRepository)),
    };
    let index_path = git(
        root,
        hooks,
        &["rev-parse", "--path-format=absolute", "--git-path", "index"],
        None,
    )?;
    let index_path = Path::new(line(&index_path)?);
    if linked(&fs::symlink_metadata(index_path).map_err(|_| err(ErrorCode::Io))?) {
        return Err(err(ErrorCode::UnsafePath));
    }
    let index_hash = digest(&read_bounded(index_path, MANIFEST_LIMIT)?);
    let head = entries(
        &git(root, hooks, &["ls-tree", "-r", "-z", "HEAD"], None)?,
        false,
    )?;
    let index = entries(
        &git(root, hooks, &["ls-files", "--stage", "-z"], None)?,
        true,
    )?;
    let untracked = paths(&git(
        root,
        hooks,
        &["ls-files", "--others", "--exclude-standard", "-z"],
        None,
    )?)?;
    let ignored = paths(&git(
        root,
        hooks,
        &[
            "ls-files",
            "--others",
            "--ignored",
            "--exclude-standard",
            "-z",
        ],
        None,
    )?)?;
    let mut names: BTreeSet<String> = head.keys().chain(index.keys()).cloned().collect();
    names.extend(untracked.iter().cloned());
    names.extend(ignored.iter().cloned());
    if names.len() > FILE_LIMIT || included.len() > FILE_LIMIT {
        return Err(err(ErrorCode::UnsupportedRepository));
    }
    let mut folded = BTreeSet::new();
    for path in &names {
        if !folded.insert(portable(path)?) {
            return Err(err(ErrorCode::UnsafePath));
        }
    }
    for path in included {
        portable(path)?;
        if !untracked.contains(path) {
            return Err(err(ErrorCode::InvalidInput));
        }
    }
    let mut files = Vec::new();
    let mut gaps = Vec::new();
    let mut total = 0u64;
    for path in names {
        let gap = if sensitive_path(&path) {
            Some(GapReason::Sensitive)
        } else if ignored.contains(&path) {
            Some(GapReason::Ignored)
        } else if untracked.contains(&path) && !included.contains(&path) {
            Some(GapReason::NotIncluded)
        } else {
            None
        };
        if let Some(reason) = gap {
            gaps.push(CaptureGap { path, reason });
            continue;
        }
        let entry = index.get(&path);
        if let Some(entry) = entry {
            let reason = match entry.mode.as_str() {
                "120000" => Some(GapReason::UnsupportedLink),
                "160000" => Some(GapReason::Submodule),
                _ => None,
            };
            if let Some(reason) = reason {
                gaps.push(CaptureGap { path, reason });
                continue;
            }
        }
        let metadata = match walk_file(root, &path) {
            Ok(meta) => meta,
            Err(e) if e.code == ErrorCode::UnsafePath => {
                gaps.push(CaptureGap {
                    path,
                    reason: GapReason::UnsupportedLink,
                });
                continue;
            }
            Err(e) => return Err(e),
        };
        if metadata.as_ref().is_some_and(|m| !m.is_file()) {
            gaps.push(CaptureGap {
                path,
                reason: GapReason::UnsupportedLink,
            });
            continue;
        }
        let size = if let Some(entry) = entry {
            line(&git(root, hooks, &["cat-file", "-s", &entry.oid], None)?)?
                .parse::<u64>()
                .map_err(|_| err(ErrorCode::UnsupportedRepository))?
        } else {
            0
        };
        if size > BLOB_LIMIT || metadata.as_ref().is_some_and(|m| m.len() > BLOB_LIMIT) {
            gaps.push(CaptureGap {
                path,
                reason: GapReason::Oversized,
            });
            continue;
        }
        let index_data = if let Some(entry) = entry {
            let data = git(root, hooks, &["cat-file", "blob", &entry.oid], None)?;
            let actual = git(
                root,
                hooks,
                &["hash-object", "--no-filters", "--stdin"],
                Some(&data),
            )?;
            if line(&actual)? != entry.oid {
                return Err(err(ErrorCode::IntegrityFailure));
            }
            Some(RawVersion {
                mode: mode(&entry.mode)?,
                data,
            })
        } else {
            None
        };
        let working = if let Some(meta) = metadata {
            #[cfg(unix)]
            let working_mode = {
                use std::os::unix::fs::PermissionsExt;
                if meta.permissions().mode() & 0o111 != 0 {
                    FileMode::Executable
                } else {
                    FileMode::Regular
                }
            };
            #[cfg(not(unix))]
            let working_mode = entry
                .map(|e| mode(&e.mode))
                .transpose()?
                .unwrap_or(FileMode::Regular);
            if !single_link(&root.join(&path), &meta)? {
                gaps.push(CaptureGap {
                    path,
                    reason: GapReason::UnsupportedLink,
                });
                continue;
            }
            Some(RawVersion {
                mode: working_mode,
                data: read_bounded(&root.join(&path), BLOB_LIMIT)?,
            })
        } else {
            None
        };
        let versions = index_data.iter().chain(working.iter());
        let secret = versions.clone().any(|v| sensitive_bytes(&v.data));
        let pointer = versions.clone().any(|v| lfs(&v.data));
        if secret || pointer {
            gaps.push(CaptureGap {
                path,
                reason: if secret {
                    GapReason::Sensitive
                } else {
                    GapReason::LfsPointer
                },
            });
            continue;
        }
        for v in versions {
            total = total
                .checked_add(v.data.len() as u64)
                .ok_or_else(|| err(ErrorCode::UnsupportedRepository))?;
        }
        if total > SNAPSHOT_LIMIT {
            return Err(err(ErrorCode::UnsupportedRepository));
        }
        #[cfg(not(unix))]
        if working
            .as_ref()
            .is_some_and(|v| v.mode == FileMode::Executable)
        {
            gaps.push(CaptureGap {
                path: path.clone(),
                reason: GapReason::ExecutableMetadata,
            });
        }
        files.push(RawFile {
            path,
            index: index_data,
            index_oid: entry.map(|e| e.oid.clone()),
            worktree: working,
        });
    }
    guard(root, hooks)?;
    Ok(Scan {
        base,
        format,
        index_hash,
        files,
        gaps,
    })
}
