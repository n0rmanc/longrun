use longrun::{
    ipc::{MAX_FRAME_BYTES, read_frame, validate_protocol_version, write_frame},
    protocol::{
        IpcError, IpcEvent, IpcEventKind, IpcMethod, IpcRequest, IpcResponse, PROTOCOL_VERSION,
    },
};
use tokio::io::{AsyncWriteExt, duplex};
use uuid::Uuid;

fn request() -> IpcRequest {
    IpcRequest {
        protocol_version: PROTOCOL_VERSION,
        request_id: Uuid::now_v7(),
        method: IpcMethod::Health,
        params: serde_json::json!({"ready": true}),
    }
}

#[tokio::test]
async fn ipc_frames_round_trip_requests_responses_and_events() {
    let request = request();
    let response = IpcResponse {
        protocol_version: PROTOCOL_VERSION,
        request_id: request.request_id,
        ok: false,
        result: None,
        error: Some(IpcError {
            code: "busy".into(),
            message: "supervisor is busy".into(),
        }),
    };
    let event = IpcEvent {
        protocol_version: PROTOCOL_VERSION,
        job_id: Uuid::now_v7(),
        event: IpcEventKind::Completed,
        payload: serde_json::json!({"exit_code": 0}),
    };

    round_trip(&request).await;
    round_trip(&response).await;
    round_trip(&event).await;
}

#[cfg(unix)]
#[tokio::test]
async fn unix_ipc_transport_round_trips_a_request_and_locks_the_socket_to_the_user() {
    use std::os::unix::fs::PermissionsExt;

    let root = std::path::PathBuf::from("/tmp").join(format!("lr-{}", Uuid::now_v7()));
    std::fs::create_dir_all(&root).expect("root");
    let socket = root.join("longrun.sock");
    let listener = longrun::ipc::unix::bind(&socket).await.expect("bind");
    assert_eq!(
        std::fs::metadata(&socket)
            .expect("socket metadata")
            .permissions()
            .mode()
            & 0o777,
        0o600
    );

    let expected = request();
    let expected_id = expected.request_id;
    let server = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.expect("accept");
        let request: IpcRequest = read_frame(&mut stream).await.expect("request");
        assert_eq!(request.request_id, expected_id);
        write_frame(
            &mut stream,
            &IpcResponse {
                protocol_version: PROTOCOL_VERSION,
                request_id: request.request_id,
                ok: true,
                result: Some(serde_json::json!({"healthy": true})),
                error: None,
            },
        )
        .await
        .expect("response");
    });
    let response = longrun::ipc::unix::request(&socket, &expected)
        .await
        .expect("client response");
    assert!(response.ok);
    server.await.expect("server task");
    std::fs::remove_dir_all(root).expect("cleanup");
}

#[cfg(windows)]
#[tokio::test]
async fn windows_ipc_transport_round_trips_a_request() {
    let endpoint = format!(r"\\.\pipe\longrun-{}", Uuid::now_v7());
    let mut server = longrun::ipc::windows::first_server(&endpoint).expect("server");
    let expected = request();
    let expected_id = expected.request_id;
    let server_task = tokio::spawn(async move {
        server.connect().await.expect("connect");
        let request: IpcRequest = read_frame(&mut server).await.expect("request");
        assert_eq!(request.request_id, expected_id);
        write_frame(
            &mut server,
            &IpcResponse {
                protocol_version: PROTOCOL_VERSION,
                request_id: request.request_id,
                ok: true,
                result: Some(serde_json::json!({"healthy": true})),
                error: None,
            },
        )
        .await
        .expect("response");
    });
    let response = longrun::ipc::windows::request(&endpoint, &expected)
        .await
        .expect("client response");
    assert!(response.ok);
    server_task.await.expect("server task");
}

#[tokio::test]
async fn ipc_frames_reject_unsupported_versions_and_malformed_or_oversized_payloads() {
    assert!(validate_protocol_version(PROTOCOL_VERSION).is_ok());
    assert!(validate_protocol_version(PROTOCOL_VERSION + 1).is_err());

    let (mut writer, mut reader) = duplex(64);
    writer.write_u32(1).await.expect("length");
    writer.write_all(b"{").await.expect("payload");
    drop(writer);
    assert!(read_frame::<IpcRequest, _>(&mut reader).await.is_err());

    let (mut writer, mut reader) = duplex(64);
    writer
        .write_u32((MAX_FRAME_BYTES + 1) as u32)
        .await
        .expect("length");
    drop(writer);
    assert!(read_frame::<IpcRequest, _>(&mut reader).await.is_err());
}

async fn round_trip<T>(message: &T)
where
    T: serde::Serialize + serde::de::DeserializeOwned + std::fmt::Debug + PartialEq,
{
    let (mut writer, mut reader) = duplex(8 * 1024);
    write_frame(&mut writer, message).await.expect("write");
    drop(writer);
    let decoded: T = read_frame(&mut reader).await.expect("read");
    assert_eq!(&decoded, message);
}
