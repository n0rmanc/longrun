use std::time::Duration;

use tokio::{
    net::windows::named_pipe::{ClientOptions, NamedPipeClient, NamedPipeServer, ServerOptions},
    time::sleep,
};
use windows_sys::Win32::Foundation::ERROR_PIPE_BUSY;

use crate::{
    error::{Error, Result},
    ipc::{read_frame, validate_protocol_version, write_frame},
    protocol::{IpcRequest, IpcResponse},
};

pub fn first_server(endpoint: &str) -> Result<NamedPipeServer> {
    let mut options = server_options();
    options.first_pipe_instance(true);
    Ok(options.create(endpoint)?)
}

pub fn next_server(endpoint: &str) -> Result<NamedPipeServer> {
    Ok(server_options().create(endpoint)?)
}

pub async fn request(endpoint: &str, request: &IpcRequest) -> Result<IpcResponse> {
    validate_protocol_version(request.protocol_version)?;
    let mut stream = connect(endpoint).await?;
    write_frame(&mut stream, request).await?;
    let response: IpcResponse = read_frame(&mut stream).await?;
    validate_protocol_version(response.protocol_version)?;
    if response.request_id != request.request_id {
        return Err(Error::InvalidInput(
            "IPC response request_id does not match request".into(),
        ));
    }
    Ok(response)
}

async fn connect(endpoint: &str) -> Result<NamedPipeClient> {
    loop {
        match ClientOptions::new().open(endpoint) {
            Ok(client) => return Ok(client),
            Err(error) if error.raw_os_error() == Some(ERROR_PIPE_BUSY as i32) => {
                sleep(Duration::from_millis(50)).await
            }
            Err(error) => return Err(error.into()),
        }
    }
}

fn server_options() -> ServerOptions {
    let mut options = ServerOptions::new();
    options.reject_remote_clients(true).write_dac(false);
    options
}
