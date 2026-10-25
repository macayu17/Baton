//! Disposable Git fixtures for tests only; never enrolls a user repository.

use baton_protocol::{Diagnostic, ErrorCode};
use std::{io, process::Command};
use tempfile::TempDir;

/// Trusted test code only. Drop removes only the allocated temporary directory.
pub struct GitFixture {
    directory: TempDir,
}

impl GitFixture {
    pub fn new() -> Result<Self, Diagnostic> {
        let directory = tempfile::tempdir().map_err(|_| Diagnostic::new(ErrorCode::Io))?;
        let fixture = Self { directory };
        std::fs::create_dir(fixture.path()).map_err(|_| Diagnostic::new(ErrorCode::Io))?;
        std::fs::create_dir(fixture.directory.path().join("empty-template"))
            .map_err(|_| Diagnostic::new(ErrorCode::Io))?;
        let template = fixture.directory.path().join("empty-template");
        let template = template
            .to_str()
            .ok_or_else(|| Diagnostic::new(ErrorCode::InvalidInput))?;
        fixture.git(&["init", "--initial-branch=main", "--template", template])?;
        fixture.git(&["config", "core.autocrlf", "false"])?;
        Ok(fixture)
    }

    pub fn path(&self) -> std::path::PathBuf {
        self.directory.path().join("repo with spaces-λ")
    }

    /// Runs Git directly, never a shell; stdout bytes are retained without decoding.
    pub fn git(&self, args: &[&str]) -> Result<Vec<u8>, Diagnostic> {
        let mut command = Command::new("git");
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x0800_0000); // Background Git must not open a console.
        }
        // Inherited Git configuration must not redirect fixtures or execute user hooks.
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
            .current_dir(self.path())
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env(
                "GIT_CONFIG_GLOBAL",
                if cfg!(windows) { "NUL" } else { "/dev/null" },
            )
            .env("GIT_ATTR_NOSYSTEM", "1")
            .env("GIT_TERMINAL_PROMPT", "0")
            .env("GIT_AUTHOR_DATE", "2000-01-01T00:00:00Z")
            .env("GIT_COMMITTER_DATE", "2000-01-01T00:00:00Z")
            .args([
                "-c",
                "user.name=Baton Fixture",
                "-c",
                "user.email=fixture@example.invalid",
                "-c",
                "commit.gpgsign=false",
                "-c",
                "core.quotePath=false",
            ])
            .arg("-c")
            .arg(format!(
                "core.hooksPath={}",
                self.directory.path().join("empty-template").display()
            ))
            .args(args);
        let output = command.output().map_err(|error| {
            Diagnostic::new(if error.kind() == io::ErrorKind::NotFound {
                ErrorCode::GitUnavailable
            } else {
                ErrorCode::Io
            })
        })?;
        if !output.status.success() {
            return Err(Diagnostic::new(ErrorCode::GitFailed));
        }
        Ok(output.stdout)
    }
}
