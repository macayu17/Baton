#![cfg(feature = "process")]
use baton_platform::workspace::Workspace;
use baton_protocol::ErrorCode;

// M5-W01; FR-06/09/14, INV-04/05: creation is rooted, bounded and never overwrites.
#[test]
fn rooted_creation_preserves_existing_files_and_refuses_unsafe_names() {
    let root = tempfile::tempdir().unwrap();
    let workspace = Workspace::open(root.path()).unwrap();
    workspace.create_file("new.rs", b"fn main() {}\n").unwrap();
    assert_eq!(
        std::fs::read(root.path().join("new.rs")).unwrap(),
        b"fn main() {}\n"
    );
    assert!(workspace.create_file("new.rs", b"overwrite").is_err());
    assert_eq!(
        std::fs::read(root.path().join("new.rs")).unwrap(),
        b"fn main() {}\n"
    );
    for name in [
        "../escape",
        "a/b",
        "a\\b",
        ".git",
        "CON.txt",
        "name.",
        "a:b",
        "",
    ] {
        assert_eq!(
            workspace.create_file(name, b"bad").unwrap_err().code,
            ErrorCode::UnsafePath
        );
    }
    assert!(workspace
        .create_file("large.txt", &vec![0; 16_385])
        .is_err());
    assert!(!root.path().join("large.txt").exists());
}

// M5-W03; INV-05: an existing hardlink cannot redirect a creation into another file.
#[test]
fn existing_hardlink_preserves_external_content() {
    let root = tempfile::tempdir().unwrap();
    let outside = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(outside.path(), b"preserve").unwrap();
    std::fs::hard_link(outside.path(), root.path().join("linked.txt")).unwrap();
    let workspace = Workspace::open(root.path()).unwrap();
    assert!(workspace.create_file("linked.txt", b"overwrite").is_err());
    assert_eq!(std::fs::read(outside.path()).unwrap(), b"preserve");
}

// M5-W02; Windows directory handles deny deletion/rename while the capability lives.
#[cfg(windows)]
#[test]
fn held_windows_root_cannot_be_renamed() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("root");
    std::fs::create_dir(&root).unwrap();
    let workspace = Workspace::open(&root).unwrap();
    assert!(std::fs::rename(&root, temp.path().join("moved")).is_err());
    workspace.create_file("new.txt", b"inside").unwrap();
    drop(workspace);
    std::fs::rename(&root, temp.path().join("moved")).unwrap();
}

// M5-W03; Unix symbolic leaf links are preserved and never followed by creation.
#[cfg(unix)]
#[test]
fn existing_symlink_preserves_external_content() {
    let root = tempfile::tempdir().unwrap();
    let outside = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(outside.path(), b"preserve").unwrap();
    std::os::unix::fs::symlink(outside.path(), root.path().join("linked.txt")).unwrap();
    let workspace = Workspace::open(root.path()).unwrap();
    assert!(workspace.create_file("linked.txt", b"overwrite").is_err());
    assert_eq!(std::fs::read(outside.path()).unwrap(), b"preserve");
}

// M5-W02; INV-05/09: held root cannot silently switch to a replacement directory.
#[cfg(unix)]
#[test]
fn replaced_root_is_refused_without_touching_the_replacement() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("root");
    std::fs::create_dir(&root).unwrap();
    let workspace = Workspace::open(&root).unwrap();
    std::fs::rename(&root, temp.path().join("old")).unwrap();
    std::fs::create_dir(&root).unwrap();
    assert!(workspace.create_file("new.txt", b"no").is_err());
    assert!(!root.join("new.txt").exists());
    assert!(!temp.path().join("old/new.txt").exists());
}
