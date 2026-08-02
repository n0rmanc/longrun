use std::collections::VecDeque;

use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use sha2::{Digest, Sha256};

use crate::{error::Result, protocol::CapturedOutput};

#[derive(Debug, Clone)]
pub struct RollingOutput {
    limit: usize,
    total_bytes: u64,
    bytes: VecDeque<u8>,
    digest: Sha256,
}

impl RollingOutput {
    pub fn new(limit: usize) -> Result<Self> {
        if limit == 0 {
            return Err(crate::error::Error::InvalidInput(
                "output tail size must be positive".into(),
            ));
        }
        Ok(Self {
            limit,
            total_bytes: 0,
            bytes: VecDeque::with_capacity(limit),
            digest: Sha256::new(),
        })
    }

    pub fn push(&mut self, bytes: &[u8]) {
        self.total_bytes = self.total_bytes.saturating_add(bytes.len() as u64);
        self.digest.update(bytes);
        for byte in bytes {
            if self.bytes.len() == self.limit {
                self.bytes.pop_front();
            }
            self.bytes.push_back(*byte);
        }
    }

    pub fn finish(self) -> CapturedOutput {
        let tail = self.bytes.into_iter().collect::<Vec<_>>();
        CapturedOutput {
            total_bytes: self.total_bytes,
            tail_base64url: URL_SAFE_NO_PAD.encode(&tail),
            truncated: self.total_bytes > tail.len() as u64,
            sha256: format!("sha256:{:x}", self.digest.finalize()),
        }
    }
}
