use std::{os::unix::fs::FileTypeExt, path::Path};

use tokio::net::{UnixListener, UnixStream};

use crate::{
    error::{Error, Result},
    ipc::{read_response, validate_protocol_version, write_frame},
    protocol::{IpcRequest, IpcResponse},
};

pub async fn bind(path: &Path) -> Result<UnixListener> {
    let listener = match UnixListener::bind(path) {
        Ok(listener) => listener,
        Err(error) if error.kind() == std::io::ErrorKind::AddrInUse => {
            match UnixStream::connect(path).await {
                Ok(_) => {
                    return Err(Error::Unavailable(
                        "Longrun supervisor is already running".into(),
                    ));
                }
                Err(connect_error)
                    if matches!(
                        connect_error.kind(),
                        std::io::ErrorKind::ConnectionRefused | std::io::ErrorKind::NotFound
                    ) =>
                {
                    let metadata = std::fs::symlink_metadata(path)?;
                    if !metadata.file_type().is_socket() {
                        return Err(error.into());
                    }
                    std::fs::remove_file(path)?;
                    UnixListener::bind(path)?
                }
                Err(_) => return Err(error.into()),
            }
        }
        Err(error) => return Err(error.into()),
    };
    std::fs::set_permissions(path, std::os::unix::fs::PermissionsExt::from_mode(0o600))?;
    Ok(listener)
}

pub async fn request(path: &Path, request: &IpcRequest) -> Result<IpcResponse> {
    validate_protocol_version(request.protocol_version)?;
    let mut stream = UnixStream::connect(path).await?;
    write_frame(&mut stream, request).await?;
    read_response(&mut stream, request).await
}
