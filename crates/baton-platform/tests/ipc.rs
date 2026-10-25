use baton_platform::ipc::{bind, connect, read_frame, write_frame};
use baton_platform::{create_private_dir, LocalAuthority};
use baton_protocol::Id;

// M4-IPC01; FR-12/13, INV-07: native transport is scoped to owned state.
#[tokio::test]
async fn owned_local_transport_roundtrips_without_network_or_saved_secret() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("state");
    create_private_dir(&root).unwrap();
    let session = Id::new_v4();
    let listener = bind(LocalAuthority::acquire(&root).unwrap(), session).unwrap();
    let server = tokio::spawn(async move {
        let mut stream = listener.accept().await.unwrap();
        assert_eq!(read_frame(&mut stream, 4096).await.unwrap(), b"request");
        write_frame(&mut stream, b"response", 4096).await.unwrap();
    });
    let mut client = connect(&root, session).await.unwrap();
    write_frame(&mut client, b"request", 4096).await.unwrap();
    assert_eq!(read_frame(&mut client, 4096).await.unwrap(), b"response");
    server.await.unwrap();
}

// M4-IPC02; FR-12/13: reject oversized/truncated frames before deserialization.
#[tokio::test]
async fn bounded_frames_reject_declared_oversize_and_truncation() {
    use tokio::io::AsyncWriteExt;
    for payload in [vec![0, 0, 16, 0], vec![0, 0, 0, 4, 1]] {
        let (mut sender, mut receiver) = tokio::io::duplex(64);
        sender.write_all(&payload).await.unwrap();
        drop(sender);
        assert!(read_frame(&mut receiver, 8).await.is_err());
    }
}
