use std::ffi::OsString;

use longrun::protocol::{
    DeliveryState, ExecutionState, IpcMethod, IpcRequest, NativeEncoding, NativeString,
    PROTOCOL_VERSION,
};
use uuid::Uuid;

#[test]
fn utf8_arguments_round_trip_without_reencoding() {
    let value = NativeString::from_os_string(OsString::from("測試 --literal"));

    assert_eq!(value.encoding, NativeEncoding::Utf8);
    assert_eq!(value.to_os_string().expect("decode"), "測試 --literal");
    assert_eq!(PROTOCOL_VERSION, 1);
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
fn execution_and_delivery_terminal_states_cannot_reopen() {
    assert!(ExecutionState::Accepted.can_transition_to(ExecutionState::Starting));
    assert!(ExecutionState::Starting.can_transition_to(ExecutionState::Running));
    assert!(ExecutionState::Running.can_transition_to(ExecutionState::Succeeded));
    assert!(!ExecutionState::Succeeded.can_transition_to(ExecutionState::Running));
    assert!(ExecutionState::Succeeded.is_terminal());

    assert!(DeliveryState::Undelivered.can_transition_to(DeliveryState::HookLeased));
    assert!(DeliveryState::HookLeased.can_transition_to(DeliveryState::Undelivered));
    assert!(DeliveryState::HookLeased.can_transition_to(DeliveryState::DeliveredInTurn));
    assert!(!DeliveryState::DeliveredInTurn.can_transition_to(DeliveryState::Undelivered));
}

#[test]
fn ipc_messages_are_versioned_json_domain_values() {
    let request = IpcRequest {
        protocol_version: PROTOCOL_VERSION,
        request_id: Uuid::now_v7(),
        method: IpcMethod::Status,
        params: serde_json::json!({"job_id": "example"}),
    };

    let json = serde_json::to_value(&request).expect("serialize");
    assert_eq!(json["protocol_version"], 1);
    assert_eq!(json["method"], "status");
}
