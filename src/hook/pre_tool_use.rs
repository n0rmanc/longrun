use std::{
    ffi::OsString,
    fs,
    path::Path,
    time::{SystemTime, UNIX_EPOCH},
};

use crate::{
    cli::{GlobalsForTarget, is_management_command, target_from_words_at_cwd},
    config::Config,
    error::{Error, Result},
    handoff::HandoffStore,
    hook::{input::PreToolUseInput, output::PreToolUseOutput},
    paths::AppPaths,
    protocol::NativeString,
};

const RTK_WRAPPER: &str = "rtk";
const RTK_LONGRUN_COMMAND: &str = "longrun";
const UNSUPPORTED_WRAPPERS: &[&str] = &["command", "env", "nohup", "sudo", "timeout"];

pub fn handle_pre_tool_use(
    input: &PreToolUseInput,
    expected_binary: &Path,
    paths: &AppPaths,
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
            if looks_like_longrun(command, expected_binary)
                || looks_like_wrapped_longrun(command, expected_binary) =>
        {
            return Ok(Some(PreToolUseOutput::deny(format!(
                "Invalid Longrun command: {error}"
            ))));
        }
        Err(_) => return Ok(None),
    };
    let Some(mut target_words) = normalize_longrun_words(&words, expected_binary) else {
        if has_unsupported_wrapper(&words, expected_binary) {
            return Ok(Some(PreToolUseOutput::deny(
                "Invalid Longrun command: unsupported wrapper; invoke Longrun directly",
            )));
        }
        return Ok(None);
    };
    let (globals, explicit_separator) = parse_target_options(&mut target_words)?;
    if target_words.is_empty() {
        return Ok(Some(PreToolUseOutput::deny(
            "Invalid Longrun command: missing target program.",
        )));
    }
    if !explicit_separator && is_management_command(&target_words[0]) {
        return Ok(None);
    }
    let cwd = NativeString::from_os_string(fs::canonicalize(&input.common.cwd)?.into_os_string());
    let target = match target_from_words_at_cwd(target_words, &globals, config, cwd.clone(), now_ms)
    {
        Ok(target) => target,
        Err(error) => {
            return Ok(Some(PreToolUseOutput::deny(format!(
                "Invalid Longrun command: {error}"
            ))));
        }
    };
    let handoff = HandoffStore::new(paths).prepare(
        input.common.session_id.clone(),
        input.turn_id.clone(),
        input.tool_use_id.clone(),
        NativeString::from_os_str(Path::new(expected_binary).as_os_str()),
        target,
        now_ms,
        config.handoff.ttl_ms,
    )?;
    let rewritten = [
        expected_binary.to_owned(),
        "internal".into(),
        "receipt".into(),
        "--handoff-id".into(),
        handoff.id,
    ];
    Ok(Some(PreToolUseOutput::allow(render_shell_words(
        &rewritten,
    ))))
}

fn normalize_longrun_words(words: &[String], expected_binary: &str) -> Option<Vec<OsString>> {
    if words.first().map(String::as_str) == Some(expected_binary) {
        return Some(words.iter().skip(1).cloned().map(OsString::from).collect());
    }
    if words.first().map(String::as_str) == Some(RTK_WRAPPER)
        && words.get(1).map(String::as_str) == Some(RTK_LONGRUN_COMMAND)
    {
        return Some(words.iter().skip(2).cloned().map(OsString::from).collect());
    }
    None
}

fn looks_like_longrun(command: &str, expected_binary: &str) -> bool {
    let words = shell_prefix_words(command, 3);
    words.first().map(String::as_str) == Some(expected_binary)
        || (words.first().map(String::as_str) == Some(RTK_WRAPPER)
            && words.get(1).map(String::as_str) == Some(RTK_LONGRUN_COMMAND))
}

fn looks_like_wrapped_longrun(command: &str, expected_binary: &str) -> bool {
    has_unsupported_wrapper(&shell_prefix_words(command, 4), expected_binary)
}

fn has_unsupported_wrapper(words: &[String], expected_binary: &str) -> bool {
    UNSUPPORTED_WRAPPERS.contains(&words.first().map(String::as_str).unwrap_or_default())
        && (words.get(1).map(String::as_str) == Some(expected_binary)
            || (words.get(1).map(String::as_str) == Some(RTK_WRAPPER)
                && words.get(2).map(String::as_str) == Some(RTK_LONGRUN_COMMAND)))
}

fn parse_target_options(words: &mut Vec<OsString>) -> Result<(GlobalsForTarget, bool)> {
    let mut globals = GlobalsForTarget::default();
    let mut explicit_separator = false;
    let index = 0;
    while index < words.len() {
        let value = words[index].to_string_lossy();
        if value == "--" {
            words.drain(..=index);
            explicit_separator = true;
            break;
        }
        if value == "--json" {
            words.remove(index);
            continue;
        }
        if value == "--timeout" || value == "--permission-profile" || value == "--env-pass" {
            let option = value.into_owned();
            let next = words
                .get(index + 1)
                .cloned()
                .ok_or_else(|| Error::InvalidInput(format!("{option} requires a value")))?;
            words.drain(index..=index + 1);
            let next = next.to_string_lossy().into_owned();
            match option.as_str() {
                "--timeout" => globals.timeout = Some(next),
                "--permission-profile" => globals.permission_profile = Some(next),
                "--env-pass" => globals.env_pass.push(next),
                _ => unreachable!(),
            }
            continue;
        }
        if let Some(value) = value.strip_prefix("--timeout=") {
            globals.timeout = Some(value.to_owned());
            words.remove(index);
            continue;
        }
        if let Some(value) = value.strip_prefix("--permission-profile=") {
            globals.permission_profile = Some(value.to_owned());
            words.remove(index);
            continue;
        }
        if let Some(value) = value.strip_prefix("--env-pass=") {
            globals.env_pass.push(value.to_owned());
            words.remove(index);
            continue;
        }
        break;
    }
    Ok((globals, explicit_separator))
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
            None if matches!(character, ';' | '|' | '&' | '<' | '>' | '`' | '$') => {
                return Err(Error::InvalidInput(
                    "shell composition is not allowed".into(),
                ));
            }
            None => {
                word.push(character);
                active = true;
            }
        }
    }
    if quote.is_some() || escaped {
        return Err(Error::InvalidInput("unterminated shell quote".into()));
    }
    if active {
        words.push(word);
    }
    Ok(words)
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
                return words;
            }
            None => {
                word.push(character);
                active = true;
            }
        }
    }
    if active && words.len() < limit {
        words.push(word);
    }
    words
}

pub fn render_shell_words(words: &[String]) -> String {
    words
        .iter()
        .map(|word| {
            if word
                .chars()
                .all(|character| character.is_ascii_alphanumeric() || "-_./:".contains(character))
            {
                word.clone()
            } else {
                format!("'{}'", word.replace('\'', "'\"'\"'"))
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

pub fn now_ms() -> Result<i64> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| Error::Unavailable(format!("system clock before epoch: {error}")))?
        .as_millis()
        .try_into()
        .map_err(|_| Error::Unavailable("system clock is out of range".into()))
}
