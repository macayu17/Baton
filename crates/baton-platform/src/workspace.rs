//! Rooted file primitives, not execution authority. Hosts must separately fence each operation.
use crate::{failure, validate_parent};
use baton_protocol::{Diagnostic, ErrorCode};
use cap_std::fs::{Dir, OpenOptions};
use std::{
    fs::File,
    io::{Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
};

/// A held directory capability. It grants no Baton lease or tool permission.
pub struct Workspace {
    directory: Dir,
    path: PathBuf,
    identity: (u64, u64),
}

fn open_directory(path: &Path) -> Result<File, Diagnostic> {
    let mut options = std::fs::OpenOptions::new();
    options.read(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        // Directory handle, no reparse traversal, no sharing deletion/rename.
        options
            .custom_flags(0x0200_0000 | 0x0020_0000)
            .share_mode(0x1 | 0x2);
    }
    let file = options.open(path).map_err(|_| failure(ErrorCode::Io))?;
    let meta = file.metadata().map_err(|_| failure(ErrorCode::Io))?;
    if !meta.is_dir() {
        return Err(failure(ErrorCode::UnsafePath));
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if meta.file_attributes() & 0x400 != 0 {
            return Err(failure(ErrorCode::UnsafePath));
        }
    }
    Ok(file)
}

pub(crate) fn identity(file: &File) -> Result<(u64, u64), Diagnostic> {
    #[cfg(windows)]
    {
        let info = winapi_util::file::information(file).map_err(|_| failure(ErrorCode::Io))?;
        if info.file_index() == 0 {
            return Err(failure(ErrorCode::Blocked));
        }
        Ok((info.volume_serial_number(), info.file_index()))
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let meta = file.metadata().map_err(|_| failure(ErrorCode::Io))?;
        Ok((meta.dev(), meta.ino()))
    }
    #[cfg(not(any(windows, unix)))]
    {
        let _ = file;
        Err(failure(ErrorCode::Blocked))
    }
}

impl Workspace {
    pub fn open(path: &Path) -> Result<Self, Diagnostic> {
        validate_parent(path)?;
        let path = std::fs::canonicalize(path).map_err(|_| failure(ErrorCode::Io))?;
        let file = open_directory(&path)?;
        let identity = identity(&file)?;
        let workspace = Self {
            directory: Dir::from_std_file(file),
            path,
            identity,
        };
        workspace.verify()?;
        Ok(workspace)
    }
    pub fn verify(&self) -> Result<(), Diagnostic> {
        validate_parent(&self.path)?;
        if identity(&open_directory(&self.path)?)? != self.identity {
            return Err(failure(ErrorCode::SourceChanged));
        }
        Ok(())
    }
    pub fn validate_name(name: &str) -> Result<(), Diagnostic> {
        let stem = name.split('.').next().unwrap_or("").to_ascii_uppercase();
        let reserved = ["CON", "PRN", "AUX", "NUL"].contains(&stem.as_str())
            || (stem.len() == 4
                && (stem.starts_with("COM") || stem.starts_with("LPT"))
                && matches!(stem.as_bytes()[3], b'1'..=b'9'));
        if name.is_empty()
            || name.len() > 128
            || name.starts_with('.')
            || name.ends_with('.')
            || reserved
            || !name
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'.'))
        {
            return Err(failure(ErrorCode::UnsafePath));
        }
        Ok(())
    }
    /// Create only; failure may leave a partial file, which is preserved for reconciliation.
    pub fn create_file(&self, name: &str, bytes: &[u8]) -> Result<String, Diagnostic> {
        Self::validate_name(name)?;
        if bytes.len() > 16_384 {
            return Err(failure(ErrorCode::InvalidInput));
        }
        self.verify()?;
        let mut options = OpenOptions::new();
        options.read(true).write(true).create_new(true);
        #[cfg(windows)]
        {
            use cap_std::fs::OpenOptionsExt;
            options.share_mode(0x1);
        }
        let mut file = self
            .directory
            .open_with(name, &options)
            .map_err(|_| failure(ErrorCode::Io))?;
        file.write_all(bytes).map_err(|_| failure(ErrorCode::Io))?;
        file.sync_all().map_err(|_| failure(ErrorCode::Io))?;
        file.seek(SeekFrom::Start(0))
            .map_err(|_| failure(ErrorCode::Io))?;
        let mut actual = Vec::new();
        (&mut file)
            .take(16_385)
            .read_to_end(&mut actual)
            .map_err(|_| failure(ErrorCode::Io))?;
        if actual != bytes {
            return Err(failure(ErrorCode::IntegrityFailure));
        }
        self.verify()?;
        Ok(blake3::hash(&actual).to_hex().to_string())
    }
}
