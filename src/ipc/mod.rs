use serde::{Serialize, de::DeserializeOwned};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

use crate::{
    error::{Error, Result},
    protocol::{IpcEvent, IpcRequest, IpcResponse, PROTOCOL_VERSION},
};

#[cfg(unix)]
pub mod unix;
#[cfg(windows)]
pub mod windows;

pub const MAX_FRAME_BYTES: usize = 1024 * 1024;

pub async fn read_frame<T, R>(reader: &mut R) -> Result<T>
where
    T: DeserializeOwned,
    R: AsyncRead + Unpin,
{
    let length = reader.read_u32().await? as usize;
    if length > MAX_FRAME_BYTES {
        return Err(Error::InvalidInput(format!(
            "IPC frame exceeds {MAX_FRAME_BYTES} bytes"
        )));
    }
    let mut payload = vec![0; length];
    reader.read_exact(&mut payload).await?;
    Ok(serde_json::from_slice(&payload)?)
}

pub async fn write_frame<T, W>(writer: &mut W, message: &T) -> Result<()>
where
    T: Serialize,
    W: AsyncWrite + Unpin,
{
    let payload = serde_json::to_vec(message)?;
    if payload.len() > MAX_FRAME_BYTES {
        return Err(Error::InvalidInput(format!(
            "IPC frame exceeds {MAX_FRAME_BYTES} bytes"
        )));
    }
    writer.write_u32(payload.len() as u32).await?;
    writer.write_all(&payload).await?;
    writer.flush().await?;
    Ok(())
}

pub async fn read_response<R>(reader: &mut R, request: &IpcRequest) -> Result<IpcResponse>
where
    R: AsyncRead + Unpin,
{
    loop {
        let message: serde_json::Value = read_frame(reader).await?;
        if let Ok(event) = serde_json::from_value::<IpcEvent>(message.clone()) {
            validate_protocol_version(event.protocol_version)?;
            continue;
        }
        let response: IpcResponse = serde_json::from_value(message)?;
        validate_protocol_version(response.protocol_version)?;
        if response.request_id != request.request_id {
            return Err(Error::InvalidInput(
                "IPC response request_id does not match request".into(),
            ));
        }
        return Ok(response);
    }
}

pub fn validate_protocol_version(version: u32) -> Result<()> {
    if version == PROTOCOL_VERSION {
        Ok(())
    } else {
        Err(Error::InvalidInput(format!(
            "unsupported IPC protocol version: {version}"
        )))
    }
}
