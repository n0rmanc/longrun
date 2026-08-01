use std::{
    ffi::OsString,
    fs,
    path::Path,
    time::{SystemTime, UNIX_EPOCH},
};

use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use clap::Parser;
use time::{OffsetDateTime, format_description::well_known::Rfc3339};

use crate::{
    cli::{Cli, Command, job_from_execution_args, job_from_shell_args},
    config::Config,
    error::{Error, Result},
    hook::{input::PreToolUseInput, output::PreToolUseOutput},
    protocol::{NativeString, PendingState, PendingSubmission, sha256_hex},
    receipt::{ReceiptPayload, ReceiptSigner},
    store::Store,
};

const PENDING_TTL_MS: i64 = 5 * 60 * 1_000;
const RECEIPT_HANDLE_PREFIX: &str = "LONGRUN_RECEIPT_HANDLE_V1 ";
const RTK_WRAPPER: &str = "rtk";
const RTK_LONGRUN_COMMAND: &str = "longrun";

pub fn handle_pre_tool_use(
    input: &PreToolUseInput,
    expected_binary: &Path,
    store: &mut Store,
    signer: &ReceiptSigner,
    config: &Config,
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
        Err(error)
            if command.contains(expected_binary) || looks_like_rtk_longrun_submission(command) =>
        {
            return Ok(Some(PreToolUseOutput::deny(format!(
                "Invalid Longrun submission: {error}"
            ))));
        }
        Err(_) => return Ok(None),
    };
    let Some(words) = normalize_submission_words(words, expected_binary) else {
        return Ok(None);
    };
    let Some(subcommand) = words.get(1).map(String::as_str) else {
        return Ok(None);
    };
    if !matches!(subcommand, "submit" | "submit-shell") {
        return Ok(None);
    }
    let wrapper_options = &words[..words
        .iter()
        .position(|word| word == "--")
        .unwrap_or(words.len())];
    if wrapper_options.iter().any(|word| {
        matches!(word.as_str(), "--hook-token" | "--hook-receipt")
            || word.starts_with("--hook-token=")
            || word.starts_with("--hook-receipt=")
    }) {
        return Ok(Some(PreToolUseOutput::deny(
            "Invalid Longrun submission: hook fields are hook-owned.",
        )));
    }
    if wrapper_options
        .iter()
        .any(|word| word == "--config" || word.starts_with("--config="))
    {
        return Ok(Some(PreToolUseOutput::deny(
            "Invalid Longrun submission: --config is not supported by Codex hooks; configure Longrun's trusted hook config instead.",
        )));
    }
    let cwd = NativeString::from_os_string(fs::canonicalize(&input.common.cwd)?.into_os_string());
    let parsed = match Cli::try_parse_from(words.iter().map(OsString::from)) {
        Ok(cli) => cli,
        Err(error) => {
            return Ok(Some(PreToolUseOutput::deny(format!(
                "Invalid Longrun submission: {error}"
            ))));
        }
    };
    let Cli { command, .. } = parsed;
    let job = match command {
        Command::Submit(arguments) => {
            job_from_execution_args(arguments.execution, cwd.clone(), config, now_ms)
        }
        Command::SubmitShell(arguments) => {
            job_from_shell_args(arguments.shell, cwd.clone(), config, now_ms)
        }
        _ => Err(Error::InvalidInput("invalid Longrun submission".into())),
    };
    let mut job = match job {
        Ok(job) => job,
        Err(error) => {
            return Ok(Some(PreToolUseOutput::deny(format!(
                "Invalid Longrun submission: {error}"
            ))));
        }
    };
    let token = random_token()?;
    let command_hash = sha256_hex(
        serde_json::to_string(&words[2..])
            .map_err(Error::Json)?
            .as_bytes(),
    );
    job.command_hash = command_hash.clone();
    let mut pending = PendingSubmission {
        session_id: input.common.session_id.clone(),
        turn_id: input.turn_id.clone(),
        tool_use_id: input.tool_use_id.clone(),
        cwd,
        binary_path: NativeString {
            encoding: crate::protocol::NativeEncoding::Utf8,
            value: expected_binary.into(),
        },
        expected_program: job.program.clone(),
        expected_args: job.args.clone(),
        command_hash,
        hook_token_hash: sha256_hex(token.as_bytes()),
        signed_receipt: None,
        created_at_ms: now_ms,
        expires_at_ms: now_ms.saturating_add(PENDING_TTL_MS),
        state: PendingState::Claimed,
    };
    let receipt = issue_receipt(job, &pending, signer, now_ms)?;
    pending.signed_receipt = Some(receipt);
    store.cleanup_expired_pending(now_ms)?;
    store.save_pending(&pending)?;

    let mut rewritten = Vec::with_capacity(words.len() + 4);
    rewritten.push(words[0].clone());
    rewritten.push("submit".into());
    rewritten.push("--hook-token".into());
    rewritten.push(token.clone());
    rewritten.push("--hook-receipt".into());
    rewritten.push(format!("{RECEIPT_HANDLE_PREFIX}{token}"));
    rewritten.extend(["--".into(), "longrun-hook-receipt".into()]);
    Ok(Some(PreToolUseOutput::allow(render_shell_words(
        &rewritten,
    ))))
}

fn normalize_submission_words(words: Vec<String>, expected_binary: &str) -> Option<Vec<String>> {
    if !Path::new(expected_binary).is_absolute() {
        return None;
    }
    if words.first().map(String::as_str) == Some(expected_binary) {
        return Some(words);
    }
    let supported_rtk_target =
        matches!(words.get(1).map(String::as_str), Some(RTK_LONGRUN_COMMAND));
    if words.first().map(String::as_str) != Some(RTK_WRAPPER) || !supported_rtk_target {
        return None;
    }

    let mut normalized = Vec::with_capacity(words.len() - 1);
    normalized.push(expected_binary.into());
    normalized.extend(words.into_iter().skip(2));
    Some(normalized)
}

fn looks_like_rtk_longrun_submission(command: &str) -> bool {
    matches!(
        shell_prefix_words(command, 3).as_slice(),
        [wrapper, target, subcommand]
            if wrapper == RTK_WRAPPER
                && target == RTK_LONGRUN_COMMAND
                && matches!(subcommand.as_str(), "submit" | "submit-shell")
    )
}

fn shell_prefix_words(command: &str, limit: usize) -> Vec<String> {
    let mut words = Vec::with_capacity(limit);
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
                    if words.len() == limit {
                        return words;
                    }
                    active = false;
                }
            }
            None if matches!(
                character,
                ';' | '|' | '&' | '<' | '>' | '`' | '$' | '\n' | '\r'
            ) =>
            {
                if active {
                    words.push(std::mem::take(&mut word));
                }
                return words;
            }
            None => {
                word.push(character);
                active = true;
            }
        }
    }
    if active {
        words.push(word);
    }
    words
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

fn issue_receipt(
    job: crate::protocol::JobSpecification,
    pending: &PendingSubmission,
    signer: &ReceiptSigner,
    now_ms: i64,
) -> Result<String> {
    let issued = OffsetDateTime::from_unix_timestamp_nanos(i128::from(now_ms) * 1_000_000)
        .map_err(|error| Error::Unavailable(format!("invalid hook timestamp: {error}")))?;
    let expires = OffsetDateTime::from_unix_timestamp_nanos(
        i128::from(pending.expires_at_ms).saturating_mul(1_000_000),
    )
    .map_err(|error| Error::Unavailable(format!("invalid pending expiry: {error}")))?;
    if expires <= issued {
        return Err(Error::Denied("hook token has expired".into()));
    }
    let payload = ReceiptPayload::from_job(
        job,
        &pending.session_id,
        &pending.turn_id,
        &pending.tool_use_id,
        issued
            .format(&Rfc3339)
            .map_err(|error| Error::Unavailable(format!("cannot format receipt time: {error}")))?,
        expires.format(&Rfc3339).map_err(|error| {
            Error::Unavailable(format!("cannot format receipt expiry: {error}"))
        })?,
        ReceiptSigner::random_nonce()?,
    );
    Ok(signer.issue(&payload)?.to_line())
}

pub fn parse_strict_shell_words(command: &str) -> Result<Vec<String>> {
    let mut words = Vec::new();
    let mut word = String::new();
    let mut quote = None;
    let mut escaped = false;
    let mut active = false;
    let mut characters = command.chars().peekable();
    while let Some(character) = characters.next() {
        if matches!(character, '\n' | '\r') {
            return Err(Error::InvalidInput(
                "outer shell composition is not allowed".into(),
            ));
        }
        if escaped {
            word.push(character);
            escaped = false;
            active = true;
            continue;
        }
        match quote {
            Some('\'') if character == '\'' => quote = None,
            Some('"') if character == '"' => quote = None,
            Some('"') if character == '\\' => {
                if matches!(
                    characters.peek(),
                    Some('"') | Some('\\') | Some('$') | Some('`')
                ) {
                    word.push(characters.next().expect("peeked character exists"));
                    active = true;
                } else {
                    word.push(character);
                    active = true;
                }
            }
            Some('"') if matches!(character, '$' | '`') => {
                return Err(Error::InvalidInput(
                    "outer shell composition is not allowed".into(),
                ));
            }
            Some(_) => {
                word.push(character);
                active = true;
            }
            None if character == '\'' || character == '"' => {
                quote = Some(character);
                active = true;
            }
            None if character == '\\' => escaped = true,
            None if matches!(character, ';' | '|' | '&' | '<' | '>' | '`' | '$') => {
                return Err(Error::InvalidInput(
                    "outer shell composition is not allowed".into(),
                ));
            }
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
