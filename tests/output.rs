use base64::Engine;
use longrun::{output::RollingOutput, protocol::CapturedOutput};

#[test]
fn rolling_output_keeps_a_bounded_tail_and_counts_all_bytes() {
    let mut output = RollingOutput::new(3).expect("rolling output");
    output.push(b"abc");
    output.push(b"defgh");
    let output = output.finish();
    assert_eq!(output.total_bytes, 8);
    assert!(output.truncated);
    assert_eq!(
        base64::engine::general_purpose::URL_SAFE_NO_PAD
            .decode(output.tail_base64url)
            .expect("tail"),
        b"fgh"
    );
}

#[test]
fn rolling_output_preserves_invalid_bytes_as_base64() {
    let mut output = RollingOutput::new(8).expect("rolling output");
    output.push(&[0xff, 0x00, b'x']);
    let output = output.finish();
    let decoded = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(output.tail_base64url)
        .expect("base64");
    assert_eq!(decoded, [0xff, 0x00, b'x']);
}

#[test]
fn result_envelopes_are_bounded_untrusted_data() {
    let result = longrun::protocol::ResultEnvelope {
        protocol_version: 2,
        terminal_reason: longrun::protocol::TerminalReason::Exited,
        exit_code: Some(0),
        signal: None,
        duration_ms: 1,
        stdout: CapturedOutput {
            total_bytes: 3,
            tail_base64url: "YWJj".into(),
            truncated: false,
            sha256: "sha256:test".into(),
        },
        stderr: CapturedOutput {
            total_bytes: 0,
            tail_base64url: String::new(),
            truncated: false,
            sha256: "sha256:empty".into(),
        },
    };
    let context = longrun::hook::output::bounded_result_context(&result, 512);
    assert!(context.contains("untrusted command output"));
    assert!(context.contains("YWJj"));
}
