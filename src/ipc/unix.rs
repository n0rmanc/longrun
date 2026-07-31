use std::path::Path;

use tokio::net::{UnixListener, UnixStream};

use crate::{
    error::{Error, Result},
    ipc::{read_frame, validate_protocol_version, write_frame},
    protocol::{IpcRequest, IpcResponse},
};

pub async fn bind(path: &Path) -> Result<UnixListener> {
    let listener = UnixListener::bind(path)?;
    std::fs::set_permissions(path, std::os::unix::fs::PermissionsExt::from_mode(0o600))?;
    Ok(listener)
}

pub async fn request(path: &Path, request: &IpcRequest) -> Result<IpcResponse> {
    validate_protocol_version(request.protocol_version)?;
    let mut stream = UnixStream::connect(path).await?;
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
