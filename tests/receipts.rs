use std::ffi::OsString;

use longrun::{
    protocol::{EnvironmentPolicy, ExecutionMode, JobSpecification, NativeString, ShellMode},
    receipt::{ReceiptExpectation, ReceiptPayload, ReceiptSigner},
    store::Store,
};
use time::{OffsetDateTime, format_description::well_known::Rfc3339};
use uuid::Uuid;

fn payload(expires_at: &str) -> ReceiptPayload {
    let specification = JobSpecification {
        protocol_version: 1,
        job_id: Uuid::now_v7(),
        program: NativeString::from_os_string(OsString::from("echo")),
        args: vec![NativeString::from_os_string(OsString::from("done"))],
        cwd: NativeString::from_os_string(std::env::current_dir().expect("cwd").into_os_string()),
        execution_mode: ExecutionMode::Embedded,
        shell_mode: ShellMode::Direct,
        timeout_ms: 1_000,
        permission_profile: ":workspace".into(),
        environment_policy: EnvironmentPolicy::default(),
        created_at_ms: 1,
        command_hash: "sha256:0000000000000000000000000000000000000000000000000000000000000000"
            .into(),
    };
    ReceiptPayload::from_job(
        specification,
        "session",
        "turn",
        "tool",
        "2026-07-31T00:00:00Z",
        expires_at,
        "nonce",
    )
}

#[test]
fn receipts_sign_exact_payload_bytes_and_verify_context() {
    let signer = ReceiptSigner::new([7; 32]);
    let payload = payload("2099-01-01T00:00:00Z");
    let line = signer.issue(&payload).expect("issue receipt").to_line();
    let receipt = signer.parse(&line).expect("parse receipt");
    let expected = ReceiptExpectation::from_payload(&payload);

    assert_eq!(
        receipt
            .verify(&signer, &expected, OffsetDateTime::now_utc())
            .expect("verify"),
        payload
    );
}

#[test]
fn expired_or_mismatched_receipts_are_rejected() {
    let signer = ReceiptSigner::new([8; 32]);
    let expired = payload("2020-01-01T00:00:00Z");
    let receipt = signer
        .parse(&signer.issue(&expired).expect("issue").to_line())
        .expect("parse");
    assert!(
        receipt
            .verify(
                &signer,
                &ReceiptExpectation::from_payload(&expired),
                OffsetDateTime::now_utc()
            )
            .is_err()
    );

    let payload = payload("2099-01-01T00:00:00Z");
    let receipt = signer
        .parse(&signer.issue(&payload).expect("issue").to_line())
        .expect("parse");
    let mut expected = ReceiptExpectation::from_payload(&payload);
    expected.session_id = "other".into();
    assert!(
        receipt
            .verify(
                &signer,
                &expected,
                OffsetDateTime::parse("2026-07-31T00:00:00Z", &Rfc3339).expect("time")
            )
            .is_err()
    );
}

#[test]
fn receipt_nonces_are_consumed_once() {
    let mut store = Store::open_in_memory().expect("store");

    store.consume_receipt_once("nonce").expect("first consume");
    assert!(store.consume_receipt_once("nonce").is_err());
}
