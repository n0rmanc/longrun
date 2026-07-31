use std::{
    path::Path,
    time::{SystemTime, UNIX_EPOCH},
};

use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};

use crate::{
    error::{Error, Result},
    hook::{input::PreToolUseInput, output::PreToolUseOutput},
    protocol::{NativeString, PendingState, PendingSubmission, sha256_hex},
    store::Store,
};

const PENDING_TTL_MS: i64 = 5 * 60 * 1_000;

pub fn handle_pre_tool_use(
    input: &PreToolUseInput,
    expected_binary: &Path,
    store: &mut Store,
    now_ms: i64,
) -> Result<Option<PreToolUseOutput>> {
    if input.common.hook_event_name != "PreToolUse" || input.tool_name != "Bash" {
        return Ok(None);
    }
    let Some(command) = input.bash_command() else {
        return Ok(None);
    };
    let expected_binary = expected_binary
        .to_str()
        .ok_or_else(|| Error::Unavailable("Longrun executable path is not UTF-8".into()))?;
    let words = match parse_strict_shell_words(command) {
        Ok(words) => words,
        Err(error) if command.contains(expected_binary) => {
            return Ok(Some(PreToolUseOutput::deny(format!(
                "Invalid Longrun submission: {error}"
            ))));
        }
        Err(_) => return Ok(None),
    };
    if words.first().map(String::as_str) != Some(expected_binary)
        || !Path::new(expected_binary).is_absolute()
    {
        return Ok(None);
    }
    let Some(subcommand) = words.get(1).map(String::as_str) else {
        return Ok(None);
    };
    if !matches!(subcommand, "submit" | "submit-shell") {
        return Ok(None);
    }
    if words.iter().any(|word| word == "--hook-token") {
        return Ok(Some(PreToolUseOutput::deny(
            "Invalid Longrun submission: hook tokens are hook-owned.",
        )));
    }
    let (expected_program, expected_args) = match subcommand {
        "submit" => {
            let separator = words.iter().position(|word| word == "--");
            let Some(separator) =
                separator.filter(|separator| *separator > 1 && *separator + 1 < words.len())
            else {
                return Ok(Some(PreToolUseOutput::deny(
                    "Invalid Longrun submission: direct submit requires `-- PROGRAM ARG...`.",
                )));
            };
            (
                NativeString {
                    encoding: crate::protocol::NativeEncoding::Utf8,
                    value: words[separator + 1].clone(),
                },
                words[separator + 2..]
                    .iter()
                    .cloned()
                    .map(|value| NativeString {
                        encoding: crate::protocol::NativeEncoding::Utf8,
                        value,
                    })
                    .collect(),
            )
        }
        "submit-shell" => {
            let script = words
                .windows(2)
                .find_map(|pair| (pair[0] == "--script").then(|| pair[1].clone()));
            let Some(script) = script else {
                return Ok(Some(PreToolUseOutput::deny(
                    "Invalid Longrun submission: submit-shell requires `--script SCRIPT`.",
                )));
            };
            (
                NativeString {
                    encoding: crate::protocol::NativeEncoding::Utf8,
                    value: "longrun-shell".into(),
                },
                vec![NativeString {
                    encoding: crate::protocol::NativeEncoding::Utf8,
                    value: script,
                }],
            )
        }
        _ => unreachable!("candidate subcommands are checked above"),
    };
    let token = random_token()?;
    let command_hash = sha256_hex(
        serde_json::to_string(&words[2..])
            .map_err(Error::Json)?
            .as_bytes(),
    );
    let pending = PendingSubmission {
        session_id: input.common.session_id.clone(),
        turn_id: input.turn_id.clone(),
        tool_use_id: input.tool_use_id.clone(),
        cwd: NativeString::from_os_string(input.common.cwd.clone().into_os_string()),
        binary_path: NativeString {
            encoding: crate::protocol::NativeEncoding::Utf8,
            value: expected_binary.into(),
        },
        expected_program,
        expected_args,
        command_hash,
        hook_token_hash: sha256_hex(token.as_bytes()),
        created_at_ms: now_ms,
        expires_at_ms: now_ms.saturating_add(PENDING_TTL_MS),
        state: PendingState::Pending,
    };
    store.cleanup_expired_pending(now_ms)?;
    store.save_pending(&pending)?;

    let mut rewritten = Vec::with_capacity(words.len() + 2);
    rewritten.extend_from_slice(&words[..2]);
    rewritten.push("--hook-token".into());
    rewritten.push(token);
    rewritten.extend_from_slice(&words[2..]);
    Ok(Some(PreToolUseOutput::allow(render_shell_words(
        &rewritten,
    ))))
}

pub fn now_ms() -> Result<i64> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| Error::Unavailable(format!("system clock before epoch: {error}")))?
        .as_millis()
        .try_into()
        .map_err(|_| Error::Unavailable("system clock is out of range".into()))
}

fn random_token() -> Result<String> {
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes)
        .map_err(|error| Error::Unavailable(format!("cannot obtain hook entropy: {error}")))?;
    Ok(URL_SAFE_NO_PAD.encode(bytes))
}

pub fn parse_strict_shell_words(command: &str) -> Result<Vec<String>> {
    if command.chars().any(|character| {
        matches!(
            character,
            ';' | '|' | '&' | '<' | '>' | '`' | '$' | '\n' | '\r'
        )
    }) {
        return Err(Error::InvalidInput(
            "outer shell composition is not allowed".into(),
        ));
    }

    let mut words = Vec::new();
    let mut word = String::new();
    let mut quote = None;
    let mut escaped = false;
    let mut active = false;
    for character in command.chars() {
        if escaped {
            word.push(character);
            escaped = false;
            active = true;
            continue;
        }
        match quote {
            Some('\'') if character == '\'' => quote = None,
            Some('"') if character == '"' => quote = None,
            Some('"') if character == '\\' => escaped = true,
            Some(_) => {
                word.push(character);
                active = true;
            }
            None if character == '\'' || character == '"' => {
                quote = Some(character);
                active = true;
            }
            None if character == '\\' => escaped = true,
            None if character.is_whitespace() => {
                if active {
                    words.push(std::mem::take(&mut word));
                    active = false;
                }
            }
            None => {
                word.push(character);
                active = true;
            }
        }
    }
    if quote.is_some() || escaped {
        return Err(Error::InvalidInput("unterminated shell quoting".into()));
    }
    if active {
        words.push(word);
    }
    Ok(words)
}

fn render_shell_words(words: &[String]) -> String {
    words
        .iter()
        .map(|word| format!("'{}'", word.replace('\'', "'\"'\"'")))
        .collect::<Vec<_>>()
        .join(" ")
}
