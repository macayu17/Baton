//! Synthetic process-tree fixture. All writes are confined to the supplied temp root.
use std::{
    fs,
    io::Write,
    path::Path,
    process::{Command, Stdio},
    thread,
    time::{Duration, Instant},
};
#[allow(clippy::zombie_processes)] // The fixture deliberately exits before its child.
fn main() {
    let args: Vec<_> = std::env::args_os().collect();
    let root = Path::new(&args[2]);
    match args[1].to_str().unwrap() {
        "parent" => {
            let _child = Command::new(std::env::current_exe().unwrap())
                .arg("writer")
                .arg(root)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .unwrap();
            let limit = Instant::now() + Duration::from_secs(15);
            while !root.join("writer-ready").exists() {
                assert!(Instant::now() < limit);
                thread::sleep(Duration::from_millis(10));
            }
            fs::write(root.join("parent-exiting"), b"ready").unwrap();
        }
        "writer" => {
            let mut lock = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(root.join("writer-lock"))
                .unwrap();
            lock.write_all(b"held until process exit").unwrap();
            fs::write(root.join("writer-ready"), b"ready").unwrap();
            let limit = Instant::now() + Duration::from_secs(30);
            let mut n = 0;
            while Instant::now() < limit && !root.join("release").exists() {
                n += 1;
                fs::write(root.join("heartbeat"), n.to_string()).unwrap();
                thread::sleep(Duration::from_millis(15));
            }
            drop(lock);
        }
        "wait" | "deploy" => {
            if args[1] == "deploy" {
                let limit = Instant::now() + Duration::from_secs(15);
                while !root.join("effect-authorized").exists() {
                    assert!(Instant::now() < limit);
                    thread::sleep(Duration::from_millis(10));
                }
                let mut service = fs::OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(root.join("external-service-committed"))
                    .unwrap();
                service.write_all(b"one mutation").unwrap();
                service.sync_all().unwrap();
            }
            fs::write(root.join("agent-ready"), b"ready").unwrap();
            let limit = Instant::now() + Duration::from_secs(30);
            while Instant::now() < limit && !root.join("release").exists() {
                thread::sleep(Duration::from_millis(10));
            }
        }
        _ => panic!("unknown fixture mode"),
    }
}
