use std::ffi::OsString;

use longrun::protocol::{
    EnvironmentPolicy, HandoffState, NativeEncoding, NativeString, PROTOCOL_VERSION, TargetSpec,
};

#[test]
fn utf8_arguments_round_trip_without_reencoding() {
    let value = NativeString::from_os_string(OsString::from("測試 --literal"));

    assert_eq!(value.encoding, NativeEncoding::Utf8);
    assert_eq!(value.to_os_string().expect("decode"), "測試 --literal");
    assert_eq!(PROTOCOL_VERSION, 2);
}

#[cfg(unix)]
#[test]
fn unix_non_utf8_arguments_round_trip_without_loss() {
    use std::os::unix::ffi::OsStringExt;

    let original = OsString::from_vec(vec![0xff, b'-', b'x']);
    let value = NativeString::from_os_string(original.clone());

    assert_eq!(value.encoding, NativeEncoding::UnixBytes);
    assert_eq!(value.to_os_string().expect("decode"), original);
}

#[test]
fn windows_utf16_arguments_round_trip_without_loss() {
    let units = [0xd800, 0x0061, 0x0000, 0xffff];
    let value = NativeString::from_windows_units(&units);

    assert_eq!(value.encoding, NativeEncoding::WindowsUtf16Le);
    assert_eq!(value.to_windows_units().expect("decode"), units);
}

#[test]
fn target_spec_serializes_native_arguments_and_policy() {
    let target = TargetSpec {
        protocol_version: PROTOCOL_VERSION,
        program: NativeString::from_os_string("gh".into()),
        args: vec![NativeString::from_os_string("run".into())],
        cwd: NativeString::from_os_string("/tmp".into()),
        timeout_ms: 1_000,
        permission_profile: ":workspace".into(),
        environment_policy: EnvironmentPolicy::default(),
        created_at_ms: 1,
        command_hash: "sha256:test".into(),
    };
    let json = serde_json::to_value(&target).expect("serialize target");
    assert_eq!(json["protocol_version"], 2);
    assert_eq!(json["program"]["value"], "gh");
    assert_eq!(HandoffState::Prepared, HandoffState::Prepared);
}
