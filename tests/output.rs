use longrun::output::{byte_tail, render_untrusted};

#[test]
fn byte_tails_are_bounded_and_hash_the_full_stream() {
    let tail = byte_tail(b"abcdef", 3);

    assert_eq!(tail.bytes, b"def");
    assert!(tail.truncated);
    assert_eq!(tail.sha256.len(), 71);
    assert!(render_untrusted(&tail).starts_with("UNTRUSTED COMMAND OUTPUT"));
}
