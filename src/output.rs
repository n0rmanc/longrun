use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use sha2::{Digest, Sha256};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ByteTail {
    pub bytes: Vec<u8>,
    pub truncated: bool,
    pub sha256: String,
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
