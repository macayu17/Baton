//! Native privacy and identity primitives for local Baton state.
use baton_protocol::{Diagnostic, ErrorCode};
use std::{
    fs::{self, File, OpenOptions},
    path::{Path, PathBuf},
};
#[cfg(feature = "ipc")]
pub mod ipc;
#[cfg(feature = "process")]
pub mod process;
#[cfg(windows)]
mod windows;

fn failure(code: ErrorCode) -> Diagnostic {
    Diagnostic::new(code)
}

/// OS-derived subject, never a caller-supplied account name.
pub fn current_principal() -> Result<String, Diagnostic> {
    #[cfg(windows)]
    {
        windows::principal()
    }
    #[cfg(unix)]
    {
        Ok(format!("uid:{}", rustix::process::geteuid().as_raw()))
    }
    #[cfg(not(any(windows, unix)))]
    {
        Err(failure(ErrorCode::Blocked))
    }
}

fn ordinary(path: &Path) -> Result<fs::Metadata, Diagnostic> {
    if !path.is_absolute() {
        return Err(failure(ErrorCode::UnsafePath));
    }
    #[cfg(windows)]
    {
        use std::path::{Component, Prefix};
        let remote = |value: &Path| matches!(value.components().next(), Some(Component::Prefix(prefix)) if !matches!(prefix.kind(), Prefix::Disk(_) | Prefix::VerbatimDisk(_)));
        if remote(path) || remote(&fs::canonicalize(path).map_err(|_| failure(ErrorCode::Io))?) {
            return Err(failure(ErrorCode::UnsafePath));
        }
    }
    for part in path.ancestors() {
        let meta = fs::symlink_metadata(part).map_err(|_| failure(ErrorCode::Io))?;
        #[cfg(windows)]
        let link = {
            use std::os::windows::fs::MetadataExt;
            meta.file_attributes() & 0x400 != 0
        };
        #[cfg(not(windows))]
        let link = meta.file_type().is_symlink();
        if link {
            #[cfg(unix)]
            {
                use std::os::unix::fs::MetadataExt;
                // OS-owned ancestor aliases (for example macOS /var) may resolve
                // an approved state root; the leaf itself must never be a link.
                let parent = part.parent().and_then(|p| fs::metadata(p).ok());
                if part != path
                    && meta.uid() == 0
                    && parent.is_some_and(|p| p.uid() == 0 && p.mode() & 0o022 == 0)
                {
                    continue;
                }
            }
            return Err(failure(ErrorCode::UnsafePath));
        }
    }
    let meta = fs::symlink_metadata(path).map_err(|_| failure(ErrorCode::Io))?;
    if !meta.is_file() && !meta.is_dir() {
        return Err(failure(ErrorCode::UnsafePath));
    }
    Ok(meta)
}

fn permissions(path: &Path, private: bool) -> Result<(), Diagnostic> {
    let meta = ordinary(path)?;
    #[cfg(windows)]
    {
        windows::validate(path, private)?;
        if meta.is_file() {
            let file = File::open(path).map_err(|_| failure(ErrorCode::Io))?;
            if winapi_util::file::information(&file)
                .map_err(|_| failure(ErrorCode::Io))?
                .number_of_links()
                != 1
            {
                return Err(failure(ErrorCode::UnsafePath));
            }
        }
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if meta.uid() != rustix::process::geteuid().as_raw()
            || meta.mode() & if private { 0o077 } else { 0o022 } != 0
        {
            return Err(failure(ErrorCode::Forbidden));
        }
        if meta.is_file() && meta.nlink() != 1 {
            return Err(failure(ErrorCode::UnsafePath));
        }
    }
    #[cfg(not(any(windows, unix)))]
    {
        let _ = (meta, private);
        return Err(failure(ErrorCode::Blocked));
    }
    Ok(())
}

/// Refuse broad or uncertain existing state; never silently change its ACL.
pub fn validate_private(path: &Path) -> Result<(), Diagnostic> {
    permissions(path, true)
}
pub fn validate_parent(path: &Path) -> Result<(), Diagnostic> {
    permissions(path, false)
}

/// Create only a new empty directory beneath a non-writable-by-others parent.
pub fn create_private_dir(path: &Path) -> Result<(), Diagnostic> {
    if !path.is_absolute() {
        return Err(failure(ErrorCode::UnsafePath));
    }
    let parent = path
        .parent()
        .ok_or_else(|| failure(ErrorCode::UnsafePath))?;
    permissions(parent, false)?;
    if !parent.is_dir() {
        return Err(failure(ErrorCode::UnsafePath));
    }
    let builder = fs::DirBuilder::new();
    #[cfg(unix)]
    let builder = {
        use std::os::unix::fs::DirBuilderExt;
        let mut builder = builder;
        builder.mode(0o700);
        builder
    };
    builder.create(path).map_err(|_| failure(ErrorCode::Io))?;
    #[cfg(windows)]
    windows::protect_new(path)?;
    validate_private(path)
}

/// New sensitive file only, with privacy inherited from an already private root.
pub fn private_file(path: &Path) -> Result<File, Diagnostic> {
    let parent = path
        .parent()
        .ok_or_else(|| failure(ErrorCode::UnsafePath))?;
    validate_private(parent)?;
    let mut options = OpenOptions::new();
    options.read(true).write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let file = options.open(path).map_err(|_| failure(ErrorCode::Io))?;
    validate_private(path)?;
    Ok(file)
}

/// Exclusive OS-owned coordinator scope. Closing/crashing releases the lock.
pub struct LocalAuthority {
    _lock: File,
    root: PathBuf,
    subject: String,
}
impl LocalAuthority {
    pub fn acquire(root: &Path) -> Result<Self, Diagnostic> {
        validate_private(root)?;
        let path = root.join("coordinator.lock");
        let lock = if path.try_exists().map_err(|_| failure(ErrorCode::Io))? {
            validate_private(&path)?;
            OpenOptions::new()
                .read(true)
                .write(true)
                .open(&path)
                .map_err(|_| failure(ErrorCode::Io))?
        } else {
            private_file(&path)?
        };
        lock.try_lock().map_err(|_| failure(ErrorCode::StoreBusy))?;
        Ok(Self {
            _lock: lock,
            root: fs::canonicalize(root).map_err(|_| failure(ErrorCode::Io))?,
            subject: current_principal()?,
        })
    }
    pub fn root(&self) -> &Path {
        &self.root
    }
    pub fn subject(&self) -> &str {
        &self.subject
    }
}
