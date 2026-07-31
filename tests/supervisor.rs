use longrun::{
    config::Config,
    ipc::{MAX_FRAME_BYTES, read_frame, validate_protocol_version, write_frame},
    paths::AppPaths,
    protocol::{
        EnvironmentPolicy, ExecutionMode, ExecutionState, IpcError, IpcEvent, IpcEventKind,
        IpcMethod, IpcRequest, IpcResponse, JobSpecification, NativeString, PROTOCOL_VERSION,
        ShellMode,
    },
    store::Store,
    supervisor::Supervisor,
};
use tokio::{
    io::{AsyncWriteExt, duplex},
    sync::watch,
    time::{Duration, sleep},
};
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

#[cfg(unix)]
#[tokio::test]
async fn supervisor_recovers_accepted_jobs_and_starts_each_durable_job_once() {
    use std::{ffi::OsString, fs, os::unix::fs::PermissionsExt, path::PathBuf};

    let root = std::env::temp_dir().join(format!("longrun-supervisor-{}", Uuid::now_v7()));
    let paths = AppPaths {
        config_dir: root.join("config"),
        data_dir: root.join("data"),
        state_dir: root.join("state"),
        log_dir: root.join("logs"),
        jobs_dir: root.join("jobs"),
        integration_dir: root.join("integration"),
        socket_path: std::env::temp_dir().join(format!("lr-{}.sock", Uuid::now_v7())),
    };
    paths.ensure_private_state().expect("state");
    let starts = root.join("starts.log");
    let bin = root.join("bin");
    fs::create_dir_all(&bin).expect("bin");
    let codex = bin.join("codex");
    fs::write(
        &codex,
        format!(
            "#!/bin/sh\nprintf x >> '{}'\nwhile [ \"$1\" != \"--\" ]; do shift; done\nshift\nexec \"$@\"\n",
            starts.display().to_string().replace('\'', "'\"'\"'")
        ),
    )
    .expect("sandbox");
    fs::set_permissions(&codex, fs::Permissions::from_mode(0o755)).expect("mode");

    let mut config = Config::default();
    config.execution.concurrency = 1;
    let config_path = paths.config_dir.join("config.toml");
    fs::write(&config_path, toml::to_string(&config).expect("config")).expect("write config");
    let database = paths.state_dir.join("longrun.sqlite");
    let resumed = durable_job("printf resumed; sleep 0.1");
    Store::open(&database)
        .expect("store")
        .create_job(&resumed)
        .expect("accepted job");
    let worker_path: OsString =
        format!("{}:{}", bin.display(), std::env::var("PATH").expect("PATH")).into();
    let supervisor = Supervisor::new(
        paths.clone(),
        &config,
        PathBuf::from(env!("CARGO_BIN_EXE_longrun")),
        config_path,
        worker_path,
    )
    .expect("supervisor");
    let (shutdown, receiver) = watch::channel(false);
    let server = {
        let supervisor = supervisor.clone();
        tokio::spawn(async move { supervisor.serve_until(receiver).await })
    };
    wait_for_socket(&paths.socket_path).await;

    let health = longrun::ipc::unix::request(
        &paths.socket_path,
        &IpcRequest {
            protocol_version: PROTOCOL_VERSION,
            request_id: Uuid::now_v7(),
            method: IpcMethod::Health,
            params: serde_json::Value::Null,
        },
    )
    .await
    .expect("health");
    assert_eq!(health.result.expect("health result")["healthy"], true);

    let submitted = durable_job("printf submitted");
    let response = longrun::ipc::unix::request(
        &paths.socket_path,
        &IpcRequest {
            protocol_version: PROTOCOL_VERSION,
            request_id: Uuid::now_v7(),
            method: IpcMethod::Submit,
            params: serde_json::to_value(&submitted).expect("job"),
        },
    )
    .await
    .expect("submit");
    assert!(response.ok, "submit response: {response:?}");
    wait_for_terminal(&database, resumed.job_id).await;
    wait_for_terminal(&database, submitted.job_id).await;
    assert_eq!(
        fs::read_to_string(&starts).expect("start count"),
        "xx",
        "one recovered job and one submitted job must each start once"
    );

    shutdown.send(true).expect("stop");
    server.await.expect("server task").expect("server result");
    assert!(
        !paths.socket_path.exists(),
        "socket must be removed on shutdown"
    );
    fs::remove_dir_all(root).expect("cleanup");
}

#[cfg(unix)]
fn durable_job(script: &str) -> JobSpecification {
    JobSpecification {
        protocol_version: PROTOCOL_VERSION,
        job_id: Uuid::now_v7(),
        program: NativeString::from_os_string("/bin/sh".into()),
        args: vec![
            NativeString::from_os_string("-c".into()),
            NativeString::from_os_string(script.into()),
        ],
        cwd: NativeString::from_os_string(std::env::current_dir().expect("cwd").into_os_string()),
        execution_mode: ExecutionMode::Durable,
        shell_mode: ShellMode::Direct,
        timeout_ms: 1_000,
        permission_profile: ":workspace".into(),
        environment_policy: EnvironmentPolicy::default(),
        created_at_ms: 1,
        command_hash: format!("sha256:{}", Uuid::now_v7()),
    }
}

#[cfg(unix)]
async fn wait_for_socket(socket: &std::path::Path) {
    for _ in 0..100 {
        if socket.exists() {
            return;
        }
        sleep(Duration::from_millis(10)).await;
    }
    panic!("supervisor socket was not created");
}

#[cfg(unix)]
async fn wait_for_terminal(database: &std::path::Path, job_id: Uuid) {
    for _ in 0..200 {
        if Store::open(database)
            .expect("store")
            .execution_state(job_id)
            .expect("state")
            .is_terminal()
        {
            assert_eq!(
                Store::open(database)
                    .expect("store")
                    .execution_state(job_id)
                    .expect("state"),
                ExecutionState::Succeeded
            );
            return;
        }
        sleep(Duration::from_millis(10)).await;
    }
    panic!("job {job_id} did not finish");
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
