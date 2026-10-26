//! Managed native scopes. Live proof objects cannot be reconstructed from records.
use baton_protocol::*;
use std::{
    ffi::OsString,
    fs,
    io::{Seek, SeekFrom},
    path::{Path, PathBuf},
    process::{ChildStdin, ChildStdout, Command, ExitStatus, Stdio},
};
mod native;
fn error(code: ErrorCode) -> Diagnostic {
    Diagnostic::new(code)
}
/// Explicit trusted launch input. Never serialized, logged, or replayed on recovery.
pub struct LaunchSpec {
    program: PathBuf,
    program_file: fs::File,
    program_hash: blake3::Hash,
    cwd: PathBuf,
    args: Vec<OsString>,
    piped: bool,
    environment: Vec<(OsString, OsString)>,
}

fn open_program(path: &Path) -> Result<fs::File, Diagnostic> {
    let mut options = fs::OpenOptions::new();
    options.read(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        // Keep the checked executable unavailable for writes or replacement.
        options.share_mode(0x1);
    }
    let file = options.open(path).map_err(|_| error(ErrorCode::Io))?;
    if !file.metadata().map_err(|_| error(ErrorCode::Io))?.is_file() {
        return Err(error(ErrorCode::InvalidInput));
    }
    Ok(file)
}

fn program_hash(mut file: &fs::File) -> Result<blake3::Hash, Diagnostic> {
    file.seek(SeekFrom::Start(0))
        .map_err(|_| error(ErrorCode::Io))?;
    let mut hash = blake3::Hasher::new();
    hash.update_reader(file).map_err(|_| error(ErrorCode::Io))?;
    Ok(hash.finalize())
}
impl LaunchSpec {
    pub fn new(
        program: &Path,
        cwd: &Path,
        args: impl IntoIterator<Item = OsString>,
    ) -> Result<Self, Diagnostic> {
        if !program.is_absolute() || !cwd.is_absolute() {
            return Err(error(ErrorCode::InvalidInput));
        }
        let program = fs::canonicalize(program).map_err(|_| error(ErrorCode::Io))?;
        let cwd = fs::canonicalize(cwd).map_err(|_| error(ErrorCode::Io))?;
        if !program.is_file() || !cwd.is_dir() {
            return Err(error(ErrorCode::InvalidInput));
        }
        let args: Vec<_> = args.into_iter().collect();
        if args.len() > 128
            || args
                .iter()
                .any(|s| s.as_encoded_bytes().len() > 16_384 || s.as_encoded_bytes().contains(&0))
        {
            return Err(error(ErrorCode::InvalidInput));
        }
        let program_file = open_program(&program)?;
        let program_hash = program_hash(&program_file)?;
        Ok(Self {
            program,
            program_file,
            program_hash,
            cwd,
            args,
            piped: false,
            environment: vec![],
        })
    }
    /// Explicit trusted protocol transport; stderr stays disconnected.
    pub fn with_piped_stdio(mut self) -> Self {
        self.piped = true;
        self
    }
    /// Explicit values only, never inherited or persisted as a launch recipe.
    pub fn with_environment(
        mut self,
        mut values: Vec<(OsString, OsString)>,
    ) -> Result<Self, Diagnostic> {
        if values.len() > 128 {
            return Err(error(ErrorCode::InvalidInput));
        }
        for (key, value) in &values {
            let Some(key) = key.to_str() else {
                return Err(error(ErrorCode::InvalidInput));
            };
            if key.is_empty()
                || key.len() > 128
                || !key.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
                || value.as_encoded_bytes().len() > 16_384
                || value.as_encoded_bytes().contains(&0)
            {
                return Err(error(ErrorCode::InvalidInput));
            }
        }
        values.sort_by_key(|(key, _)| key.to_ascii_lowercase());
        if values
            .windows(2)
            .any(|pair| pair[0].0.eq_ignore_ascii_case(&pair[1].0))
        {
            return Err(error(ErrorCode::InvalidInput));
        }
        self.environment = values;
        Ok(self)
    }
    pub fn working_directory(&self) -> &Path {
        &self.cwd
    }
    pub fn fingerprint(&self) -> String {
        let mut hash = blake3::Hasher::new();
        if self.piped {
            hash.update(b"baton-launch-v2-env-clear-stdio-piped");
        } else {
            hash.update(b"baton-launch-v2-env-clear-stdio-null");
        }
        hash.update(self.program_hash.as_bytes());
        for field in [self.program.as_os_str(), self.cwd.as_os_str()]
            .into_iter()
            .chain(self.args.iter().map(|a| a.as_os_str()))
        {
            let bytes = field.as_encoded_bytes();
            hash.update(&(bytes.len() as u64).to_le_bytes());
            hash.update(bytes);
        }
        if !self.environment.is_empty() {
            hash.update(b"explicit-environment-v1:");
            hash.update(&(self.environment.len() as u64).to_le_bytes());
            for field in self
                .environment
                .iter()
                .flat_map(|(key, value)| [key, value])
            {
                let bytes = field.as_encoded_bytes();
                hash.update(&(bytes.len() as u64).to_le_bytes());
                hash.update(bytes);
            }
        }
        hash.finalize().to_hex().to_string()
    }
    fn validate_program(&self) -> Result<(), Diagnostic> {
        // shortcut: path-based exec can race ancestor changes; require containment for runtime attestation.
        let current =
            open_program(&self.program).map_err(|_| error(ErrorCode::IntegrityFailure))?;
        if crate::workspace::identity(&current)? != crate::workspace::identity(&self.program_file)?
            || program_hash(&current)? != self.program_hash
        {
            return Err(error(ErrorCode::IntegrityFailure));
        }
        Ok(())
    }
    fn command(&self) -> Command {
        let mut command = Command::new(&self.program);
        command
            .args(&self.args)
            .current_dir(&self.cwd)
            .env_clear()
            .envs(self.environment.iter().map(|(key, value)| (key, value)))
            .stdin(if self.piped {
                Stdio::piped()
            } else {
                Stdio::null()
            })
            .stdout(if self.piped {
                Stdio::piped()
            } else {
                Stdio::null()
            })
            .stderr(Stdio::null());
        command
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TreeStatus {
    Active,
    Empty,
    Unknown,
}

pub struct ProcessTree {
    native: native::Native,
    _program: fs::File,
    scope_id: Id,
    fingerprint: String,
    worktree: PathBuf,
}
impl ProcessTree {
    pub fn spawn(spec: LaunchSpec) -> Result<Self, Diagnostic> {
        spec.validate_program()?;
        let fingerprint = spec.fingerprint();
        let worktree = spec.working_directory().to_path_buf();
        let native = native::Native::spawn(spec.command())?;
        Ok(Self {
            native,
            _program: spec.program_file,
            scope_id: Id::new_v4(),
            fingerprint,
            worktree,
        })
    }
    pub fn scope_id(&self) -> Id {
        self.scope_id
    }
    pub fn take_stdio(&mut self) -> Result<(ChildStdin, ChildStdout), Diagnostic> {
        self.native.take_stdio()
    }
    pub fn launch_fingerprint(&self) -> &str {
        &self.fingerprint
    }
    pub fn working_directory(&self) -> &Path {
        &self.worktree
    }
    pub fn pid(&self) -> u32 {
        self.native.pid()
    }
    pub fn root_exited(&mut self) -> Result<bool, Diagnostic> {
        self.native.root_exited()
    }
    /// Root result only; it cannot establish descendant drain or settled effects.
    pub fn root_exit_status(&mut self) -> Result<Option<ExitStatus>, Diagnostic> {
        self.native.root_exit_status()
    }
    pub fn status(&mut self) -> Result<TreeStatus, Diagnostic> {
        self.native.status()
    }
    pub fn terminate(&mut self) -> Result<(), Diagnostic> {
        self.native.terminate()
    }
    pub fn drain_proof(&mut self) -> Result<NativeDrainProof<'_>, Diagnostic> {
        if self.status()? != TreeStatus::Empty {
            return Err(error(ErrorCode::Blocked));
        }
        Ok(NativeDrainProof { tree: self })
    }
    /// A sampled live root in this owned scope; neither future liveness nor adapter validity.
    pub fn running_proof(&mut self) -> Result<NativeRunningProof<'_>, Diagnostic> {
        if self.status()? != TreeStatus::Active || self.root_exited()? {
            return Err(error(ErrorCode::Blocked));
        }
        Ok(NativeRunningProof { tree: self })
    }
}
pub struct NativeRunningProof<'a> {
    tree: &'a ProcessTree,
}
impl NativeRunningProof<'_> {
    pub fn working_directory(&self) -> &Path {
        self.tree.working_directory()
    }
    pub fn scope_id(&self) -> Id {
        self.tree.scope_id()
    }
    pub fn launch_fingerprint(&self) -> &str {
        self.tree.launch_fingerprint()
    }
    pub fn pid(&self) -> u32 {
        self.tree.pid()
    }
}
/// Borrowed from the actual empty scope, never a serialized status assertion.
pub struct NativeDrainProof<'a> {
    tree: &'a ProcessTree,
}
impl NativeDrainProof<'_> {
    pub fn scope_id(&self) -> Id {
        self.tree.scope_id()
    }
    pub fn launch_fingerprint(&self) -> &str {
        self.tree.launch_fingerprint()
    }
    pub fn pid(&self) -> u32 {
        self.tree.pid()
    }
}
