use longrun::output::{byte_tail, read_log_chunk, render_untrusted};
use uuid::Uuid;

#[test]
fn byte_tails_are_bounded_and_hash_the_full_stream() {
    let tail = byte_tail(b"abcdef", 3);

    assert_eq!(tail.bytes, b"def");
    assert!(tail.truncated);
    assert_eq!(tail.sha256.len(), 71);
    assert!(render_untrusted(&tail).starts_with("UNTRUSTED COMMAND OUTPUT"));
}

#[tokio::test]
async fn log_chunks_are_bounded_and_resume_at_the_next_offset() {
    let root = std::env::temp_dir().join(format!("longrun-output-{}", Uuid::now_v7()));
    std::fs::create_dir_all(&root).expect("root");
    let log = root.join("stdout.log");
    std::fs::write(&log, b"abcdef").expect("log");

    let first = read_log_chunk(&log, 0, 3).await.expect("first chunk");
    assert_eq!(first.bytes, b"abc");
    assert_eq!(first.next_offset, 3);
    assert!(!first.at_end);

    let second = read_log_chunk(&log, first.next_offset, 3)
        .await
        .expect("second chunk");
    assert_eq!(second.bytes, b"def");
    assert_eq!(second.next_offset, 6);
    assert!(second.at_end);
    std::fs::remove_dir_all(root).expect("cleanup");
}
