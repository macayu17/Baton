//! Local-only IPC. Access comes from the OS principal, never an actor ID in JSON.
use super::*;
use baton_protocol::Id;
use interprocess::local_socket::{
    tokio::{Listener, Stream},
    traits::tokio::Listener as _,
    GenericFilePath, ListenerOptions, ToFsName,
};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

pub struct LocalListener {
    listener: Listener,
    authority: LocalAuthority,
}
impl LocalListener {
    pub fn authority(&self) -> &LocalAuthority {
        &self.authority
    }
    pub async fn accept(&self) -> Result<Stream, Diagnostic> {
        let stream = self
            .listener
            .accept()
            .await
            .map_err(|_| failure(ErrorCode::Io))?;
        validate_peer(&stream)?;
        Ok(stream)
    }
}

fn endpoint(root: &Path, session: Id) -> Result<PathBuf, Diagnostic> {
    validate_private(root)?;
    if session.is_nil() {
        return Err(failure(ErrorCode::InvalidInput));
    }
    #[cfg(windows)]
    {
        Ok(PathBuf::from(format!(r"\\.\pipe\baton-v1-{session}")))
    }
    #[cfg(unix)]
    {
        // The locator is not an authentication secret; compact names fit macOS
        // temporary roots, and bind never overwrites a colliding socket.
        let path = root.join(format!("s{}", &session.simple().to_string()[..16]));
        if path.as_os_str().as_encoded_bytes().len() >= 100 {
            return Err(failure(ErrorCode::UnsafePath));
        }
        Ok(path)
    }
    #[cfg(not(any(windows, unix)))]
    {
        Err(failure(ErrorCode::Blocked))
    }
}

pub fn bind(authority: LocalAuthority, session: Id) -> Result<LocalListener, Diagnostic> {
    let path = endpoint(authority.root(), session)?;
    let name = path
        .to_fs_name::<GenericFilePath>()
        .map_err(|_| failure(ErrorCode::UnsafePath))?;
    let options = ListenerOptions::new().name(name).try_overwrite(false);
    #[cfg(windows)]
    let options = {
        use interprocess::os::windows::{
            local_socket::ListenerOptionsExt, security_descriptor::SecurityDescriptor,
        };
        let sid = authority
            .subject()
            .strip_prefix("windows:")
            .ok_or_else(|| failure(ErrorCode::Forbidden))?;
        let sddl = widestring::U16CString::from_str(format!("O:{sid}D:P(A;;GA;;;{sid})"))
            .map_err(|_| failure(ErrorCode::Forbidden))?;
        let descriptor = SecurityDescriptor::deserialize(sddl.as_ucstr())
            .map_err(|_| failure(ErrorCode::Forbidden))?;
        options.security_descriptor(descriptor)
    };
    #[cfg(unix)]
    let options = {
        use interprocess::os::unix::local_socket::ListenerOptionsExt;
        options.mode(0o600)
    };
    let listener = options.create_tokio().map_err(|_| failure(ErrorCode::Io))?;
    Ok(LocalListener {
        listener,
        authority,
    })
}

pub async fn connect(root: &Path, session: Id) -> Result<Stream, Diagnostic> {
    let path = endpoint(root, session)?;
    #[cfg(windows)]
    {
        use std::os::windows::{fs::OpenOptionsExt, io::OwnedHandle};
        use windows_permissions::{
            constants::{SeObjectType, SecurityInformation},
            utilities, wrappers,
        };
        let mut options = OpenOptions::new();
        options
            .read(true)
            .write(true)
            .access_mode(0xC002_0000) // GENERIC_READ | GENERIC_WRITE | READ_CONTROL
            .custom_flags(0x4000_0000) // FILE_FLAG_OVERLAPPED for Tokio
            .security_qos_flags(0x0011_0000); // SQOS_PRESENT | IDENTIFICATION: no impersonation
        let until = tokio::time::Instant::now() + std::time::Duration::from_secs(3);
        let file = loop {
            match options.open(&path) {
                Ok(file) => break file,
                Err(e) if e.raw_os_error() == Some(231) && tokio::time::Instant::now() < until => {
                    // ERROR_PIPE_BUSY: no request has been sent; wait for an instance.
                    tokio::time::sleep(std::time::Duration::from_millis(10)).await;
                }
                Err(_) => return Err(failure(ErrorCode::Io)),
            }
        };
        let sd = wrappers::GetSecurityInfo(
            &file,
            SeObjectType::SE_FILE_OBJECT,
            SecurityInformation::Owner,
        )
        .map_err(|_| failure(ErrorCode::Unauthorized))?;
        let sid = utilities::current_process_sid().map_err(|_| failure(ErrorCode::Unauthorized))?;
        if sd.owner() != Some(&*sid) {
            return Err(failure(ErrorCode::Unauthorized));
        }
        let handle: OwnedHandle = file.into();
        let stream =
            interprocess::os::windows::named_pipe::local_socket::tokio::Stream::try_from(handle)
                .map_err(|_| failure(ErrorCode::Io))?;
        Ok(Stream::NamedPipe(stream))
    }
    #[cfg(unix)]
    {
        use interprocess::local_socket::traits::tokio::Stream as _;
        let name = path
            .to_fs_name::<GenericFilePath>()
            .map_err(|_| failure(ErrorCode::UnsafePath))?;
        let stream = Stream::connect(name)
            .await
            .map_err(|_| failure(ErrorCode::Io))?;
        validate_peer(&stream)?;
        Ok(stream)
    }
    #[cfg(not(any(windows, unix)))]
    Err(failure(ErrorCode::Blocked))
}

fn validate_peer(_stream: &Stream) -> Result<(), Diagnostic> {
    #[cfg(unix)]
    {
        use interprocess::local_socket::traits::StreamCommon;
        if _stream
            .peer_creds()
            .map_err(|_| failure(ErrorCode::Unauthorized))?
            .euid()
            != Some(rustix::process::geteuid().as_raw())
        {
            return Err(failure(ErrorCode::Unauthorized));
        }
    }
    // Windows enforces the explicit owner-only pipe DACL and rejects remote clients.
    Ok(())
}

pub async fn read_frame(
    stream: &mut (impl AsyncRead + Unpin),
    max: usize,
) -> Result<Vec<u8>, Diagnostic> {
    let len = stream
        .read_u32()
        .await
        .map_err(|_| failure(ErrorCode::Io))? as usize;
    if len == 0 || len > max {
        return Err(failure(ErrorCode::InvalidInput));
    }
    let mut bytes = vec![0; len];
    stream
        .read_exact(&mut bytes)
        .await
        .map_err(|_| failure(ErrorCode::Io))?;
    Ok(bytes)
}
pub async fn write_frame(
    stream: &mut (impl AsyncWrite + Unpin),
    bytes: &[u8],
    max: usize,
) -> Result<(), Diagnostic> {
    let len = u32::try_from(bytes.len()).map_err(|_| failure(ErrorCode::InvalidInput))?;
    if len == 0 || bytes.len() > max {
        return Err(failure(ErrorCode::InvalidInput));
    }
    stream
        .write_u32(len)
        .await
        .map_err(|_| failure(ErrorCode::Io))?;
    stream
        .write_all(bytes)
        .await
        .map_err(|_| failure(ErrorCode::Io))
}
