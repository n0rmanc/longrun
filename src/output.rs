use std::{io::SeekFrom, path::Path};

use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use sha2::{Digest, Sha256};
use tokio::io::{AsyncReadExt, AsyncSeekExt};

use crate::error::Result;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ByteTail {
    pub bytes: Vec<u8>,
    pub truncated: bool,
    pub sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogChunk {
    pub bytes: Vec<u8>,
    pub next_offset: u64,
    pub at_end: bool,
}

pub fn byte_tail(input: &[u8], limit: usize) -> ByteTail {
    let start = input.len().saturating_sub(limit);
    ByteTail {
        bytes: input[start..].to_vec(),
        truncated: start > 0,
        sha256: format!("sha256:{:x}", Sha256::digest(input)),
    }
}

pub fn render_untrusted(tail: &ByteTail) -> String {
    let encoded = URL_SAFE_NO_PAD.encode(&tail.bytes);
    format!(
        "UNTRUSTED COMMAND OUTPUT (base64url; truncated={}):\n{}",
        tail.truncated, encoded
    )
}

pub async fn read_log_chunk(path: &Path, offset: u64, max_bytes: usize) -> Result<LogChunk> {
    if max_bytes == 0 {
        return Err(crate::error::Error::InvalidInput(
            "log chunk size must be positive".into(),
        ));
    }
    let mut file = match tokio::fs::File::open(path).await {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(LogChunk {
                bytes: Vec::new(),
                next_offset: 0,
                at_end: true,
            });
        }
        Err(error) => return Err(error.into()),
    };
    let length = file.metadata().await?.len();
    let offset = offset.min(length);
    file.seek(SeekFrom::Start(offset)).await?;
    let mut bytes = vec![0; max_bytes];
    let read = file.read(&mut bytes).await?;
    bytes.truncate(read);
    let next_offset = offset.saturating_add(read as u64);
    Ok(LogChunk {
        bytes,
        next_offset,
        at_end: next_offset >= length,
    })
}
