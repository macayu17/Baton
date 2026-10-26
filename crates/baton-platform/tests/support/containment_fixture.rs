//! Benign native containment fixture; only caller-owned temporary paths and loopback.
use std::{
    fs,
    io::ErrorKind,
    net::{SocketAddr, TcpStream},
    path::PathBuf,
    time::Duration,
};

fn main() {
    if std::env::args().nth(1).as_deref() == Some("--diagnose-executor") {
        let args: Vec<_> = std::env::args_os().collect();
        assert_eq!(args.len(), 5);
        let mut command = std::process::Command::new(&args[2]);
        command
            .arg(&args[3])
            .env_clear()
            .env("SystemRoot", std::env::var_os("SystemRoot").unwrap());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x0800_0000);
        }
        let output = command.output().unwrap();
        fs::write(&args[4], &output.stderr).unwrap();
        use std::io::Write;
        std::io::stdout().write_all(&output.stdout).unwrap();
        std::io::stdout().flush().unwrap();
        std::process::exit(output.status.code().unwrap_or(3));
    }
    if std::env::args().nth(1).as_deref() == Some("--exit-seven") {
        std::process::exit(7);
    }
    assert_eq!(std::env::args().nth(1).as_deref(), Some("--workload"));
    let root = PathBuf::from(std::env::var_os("BATON_PROBE_ROOT").unwrap());
    let outside = PathBuf::from(std::env::var_os("BATON_PROBE_OUTSIDE").unwrap());
    let address: SocketAddr = std::env::var("BATON_PROBE_ADDRESS")
        .unwrap()
        .parse()
        .unwrap();
    assert!(address.ip().is_loopback());
    let read_ok = fs::read(root.join("input.txt")).is_ok_and(|bytes| bytes == b"baton-owned-input");
    let write_denied = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(root.join("blocked.txt"))
        .is_err_and(|error| error.kind() == ErrorKind::PermissionDenied);
    let outside_denied =
        fs::read(outside).is_err_and(|error| error.kind() == ErrorKind::PermissionDenied);
    let network_denied = TcpStream::connect_timeout(&address, Duration::from_secs(1))
        .is_err_and(|error| error.kind() == ErrorKind::PermissionDenied);
    println!("{{\"schema_version\":\"0.1.0\",\"read_ok\":{read_ok},\"write_denied\":{write_denied},\"outside_denied\":{outside_denied},\"network_denied\":{network_denied}}}");
    if !(read_ok && write_denied && outside_denied && network_denied) {
        std::process::exit(2);
    }
}
