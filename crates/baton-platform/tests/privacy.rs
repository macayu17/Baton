use baton_platform::{create_private_dir, private_file, validate_private, LocalAuthority};
use std::fs;

// M4-OS01; FR-12/16, INV-05: creation never adopts or changes existing work.
#[test]
fn private_creation_is_owner_scoped_and_never_adopts_existing_paths() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("owned");
    create_private_dir(&root).unwrap();
    validate_private(&root).unwrap();
    let file = root.join("capability");
    let mut writer = private_file(&file).unwrap();
    use std::io::Write;
    writer.write_all(b"synthetic fixture").unwrap();
    writer.sync_all().unwrap();
    drop(writer);
    validate_private(&file).unwrap();
    assert!(create_private_dir(&root).is_err());
    assert!(private_file(&file).is_err());
    assert_eq!(fs::read(&file).unwrap(), b"synthetic fixture");
}

// M4-OS02; FR-12/16: broad access is refused rather than silently repaired.
#[test]
fn widened_permissions_are_refused_without_changing_user_acl() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("owned");
    create_private_dir(&root).unwrap();
    #[cfg(windows)]
    {
        use windows_permissions::{
            constants::{SeObjectType, SecurityInformation},
            wrappers, LocalBox, SecurityDescriptor,
        };
        let sd: LocalBox<SecurityDescriptor> = "D:P(A;OICI;FA;;;WD)".parse().unwrap();
        wrappers::SetNamedSecurityInfo(
            root.as_os_str(),
            SeObjectType::SE_FILE_OBJECT,
            SecurityInformation::Dacl | SecurityInformation::ProtectedDacl,
            None,
            None,
            sd.dacl(),
            None,
        )
        .unwrap();
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&root, fs::Permissions::from_mode(0o755)).unwrap();
    }
    assert!(validate_private(&root).is_err());
    assert!(create_private_dir(&root.join("child")).is_err());
}

// M4-OS03; FR-12/16, INV-05: another name for a capability cannot be trusted.
#[test]
fn hardlinked_capability_files_are_refused() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("owned");
    create_private_dir(&root).unwrap();
    let file = root.join("key");
    drop(private_file(&file).unwrap());
    fs::hard_link(&file, root.join("alias")).unwrap();
    assert!(validate_private(&file).is_err());
}

// M4-OS04; FR-12, INV-07: only one coordinator may recover local identity.
#[test]
fn local_authority_lock_is_exclusive_and_released_on_close() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("state");
    create_private_dir(&root).unwrap();
    let authority = LocalAuthority::acquire(&root).unwrap();
    assert_eq!(
        authority.subject(),
        baton_platform::current_principal().unwrap()
    );
    assert!(LocalAuthority::acquire(&root).is_err());
    drop(authority);
    assert!(LocalAuthority::acquire(&root).is_ok());
}

// M4-OS05; FR-12/16: UNC state is refused before any remote path access.
#[cfg(windows)]
#[test]
fn unc_state_is_refused_before_remote_access() {
    let path = std::path::Path::new(r"\\unreachable.invalid\share\state");
    assert_eq!(
        validate_private(path).unwrap_err().code,
        baton_protocol::ErrorCode::UnsafePath
    );
}

// M4-OS06; FR-12/16, INV-05: an existing linked root is never adopted.
#[cfg(unix)]
#[test]
fn symlinked_state_leaf_is_refused() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("state");
    create_private_dir(&root).unwrap();
    let alias = temp.path().join("alias");
    std::os::unix::fs::symlink(&root, &alias).unwrap();
    assert!(validate_private(&alias).is_err());
}
