//! The environment the local service starts with. An app opened from Finder
//! inherits launchd's minimal environment, so Desktop reads the variables the
//! user's login shell sets, keeps a few fixed directories on `PATH`, and names
//! the macOS system proxy when no variable names one.

use std::{
    collections::BTreeMap,
    ffi::{CStr, OsStr, OsString},
    io::Read,
    os::unix::{ffi::OsStrExt, process::CommandExt},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::mpsc,
    time::Duration,
};

/// Brackets the variables in the probe's output, so whatever rc files print
/// around them is ignored.
const DELIMITER: &str = "_DIM_SHELL_ENV_DELIMITER_";
/// How long one candidate shell may take to print its variables.
const SHELL_TIMEOUT: Duration = Duration::from_secs(10);
/// Tried in order after the account's login shell.
const FALLBACK_SHELLS: [&str; 3] = ["/bin/zsh", "/bin/bash", "/bin/sh"];
/// Keeps oh-my-zsh update prompts and tmux autostart from blocking a read that
/// has no terminal.
const PROBE_ENVIRONMENT: [(&str, &str); 3] = [
    ("DISABLE_AUTO_UPDATE", "true"),
    ("ZSH_TMUX_AUTOSTARTED", "true"),
    ("ZSH_TMUX_AUTOSTART", "false"),
];
/// Variables that describe the probe shell rather than the user's setup.
const SESSION_KEYS: [&str; 4] = ["PWD", "OLDPWD", "SHLVL", "_"];
/// Desktop sets these for the service itself.
const LAUNCHER_PREFIXES: [&str; 2] = ["DIM_", "DIMCODE_"];
/// An environment that names any of these keeps its own proxy.
const PROXY_NAMES: [&str; 6] = [
    "HTTPS_PROXY",
    "https_proxy",
    "HTTP_PROXY",
    "http_proxy",
    "ALL_PROXY",
    "all_proxy",
];

/// The variables to start the service with: the inherited environment with
/// the login shell's variables over it, the fixed directories appended to
/// `PATH`, and the system proxy when no variable names one.
pub fn service_environment(user_home: &Path) -> BTreeMap<OsString, OsString> {
    let mut variables: BTreeMap<OsString, OsString> = std::env::vars_os().collect();
    match login_shell_variables(user_home) {
        Ok(shell) => merge(&mut variables, shell),
        Err(failures) => eprintln!(
            "Dim: the login shell environment is unavailable ({})",
            failures.join("; ")
        ),
    }
    let path = with_fallback_paths(variables.get(OsStr::new("PATH")), user_home);
    variables.insert("PATH".into(), path);
    if !PROXY_NAMES
        .iter()
        .any(|name| variables.contains_key(OsStr::new(name)))
    {
        variables.extend(
            system_proxy()
                .into_iter()
                .map(|(name, value)| (name.into(), value.into())),
        );
    }
    variables
}

/// The first candidate shell's variables, or why each candidate failed.
fn login_shell_variables(user_home: &Path) -> Result<Vec<(OsString, OsString)>, Vec<String>> {
    let mut failures = Vec::new();
    for shell in shell_candidates() {
        match read_shell(&shell, user_home) {
            Ok(variables) => return Ok(variables),
            Err(reason) => failures.push(format!("{}: {reason}", shell.display())),
        }
    }
    Err(failures)
}

/// The account record's login shell (not `$SHELL`, which a launch need not
/// carry), then the fixed system shells.
fn shell_candidates() -> Vec<PathBuf> {
    let mut shells: Vec<PathBuf> = account_shell().into_iter().collect();
    for shell in FALLBACK_SHELLS.map(PathBuf::from) {
        if !shells.contains(&shell) {
            shells.push(shell);
        }
    }
    shells
}

fn account_shell() -> Option<PathBuf> {
    let mut record = std::mem::MaybeUninit::<libc::passwd>::uninit();
    let mut buffer = vec![0 as libc::c_char; 16 * 1024];
    let mut found = std::ptr::null_mut();
    // SAFETY: getpwuid_r writes the record into `record` and its strings into
    // `buffer`; both outlive every read of `found` below.
    let status = unsafe {
        libc::getpwuid_r(
            libc::getuid(),
            record.as_mut_ptr(),
            buffer.as_mut_ptr(),
            buffer.len(),
            &mut found,
        )
    };
    if status != 0 || found.is_null() {
        return None;
    }
    // SAFETY: a successful call points `found` at the initialized `record`,
    // whose `pw_shell` is null or a NUL-terminated string inside `buffer`.
    let shell = unsafe { (*found).pw_shell };
    if shell.is_null() {
        return None;
    }
    // SAFETY: see above.
    let shell = PathBuf::from(OsStr::from_bytes(
        unsafe { CStr::from_ptr(shell) }.to_bytes(),
    ));
    shell.is_absolute().then_some(shell)
}

/// Runs `shell` as an interactive login shell, so it reads the same startup
/// files as a terminal, and returns the variables it prints.
fn read_shell(shell: &Path, user_home: &Path) -> Result<Vec<(OsString, OsString)>, String> {
    let dump = format!(
        "printf '\\0%s\\0' '{DELIMITER}'; command env -0 || exit; printf '\\0%s\\0' '{DELIMITER}'; exit"
    );
    // stdin is closed, so a prompt in an rc file reads end of file instead of
    // waiting; the shell leads its own process group, so a timeout stops
    // whatever it started too.
    let mut child = Command::new(shell)
        .args(["-ilc", &dump])
        .current_dir(user_home)
        .envs(PROBE_ENVIRONMENT)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .process_group(0)
        .spawn()
        .map_err(|error| error.to_string())?;
    let group = child.id() as libc::pid_t;
    let mut stdout = child.stdout.take().ok_or("no stdout")?;
    let (sender, receiver) = mpsc::channel();
    std::thread::spawn(move || {
        // A background child of an rc file can hold stdout open long after the
        // shell exits, so the read ends at the closing delimiter rather than
        // at end of file.
        let (mut output, mut chunk) = (Vec::new(), [0; 8192]);
        while let Ok(read) = stdout.read(&mut chunk) {
            if read == 0 {
                break;
            }
            output.extend_from_slice(&chunk[..read]);
            if delimiters(&output) >= 2 {
                break;
            }
        }
        let _ = sender.send(output);
    });
    let output = receiver.recv_timeout(SHELL_TIMEOUT);
    // SAFETY: a negative pid signals the probe's process group; a group that
    // has already exited only makes kill report an error.
    unsafe { libc::kill(-group, libc::SIGKILL) };
    let _ = child.wait();
    let output = output.map_err(|_| "timeout".to_owned())?;
    parse_dump(&output).ok_or_else(|| "unparsed output".to_owned())
}

fn delimiters(output: &[u8]) -> usize {
    let marker = [&[0], DELIMITER.as_bytes(), &[0]].concat();
    output
        .windows(marker.len())
        .filter(|window| *window == marker.as_slice())
        .count()
}

/// The variables between the dump's two delimiters, or `None` when the
/// output does not hold both.
fn parse_dump(output: &[u8]) -> Option<Vec<(OsString, OsString)>> {
    let parts: Vec<&[u8]> = output.split(|byte| *byte == 0).collect();
    let first = parts
        .iter()
        .position(|part| *part == DELIMITER.as_bytes())?;
    let last = parts
        .iter()
        .rposition(|part| *part == DELIMITER.as_bytes())?;
    (first < last).then(|| {
        parts[first + 1..last]
            .iter()
            .filter_map(|entry| {
                let separator = entry.iter().position(|byte| *byte == b'=')?;
                (separator > 0).then(|| {
                    (
                        OsStr::from_bytes(&entry[..separator]).to_owned(),
                        OsStr::from_bytes(&entry[separator + 1..]).to_owned(),
                    )
                })
            })
            .collect()
    })
}

/// Puts the shell's variables over `variables`, except the probe session's
/// own and the ones Desktop sets for the service.
fn merge(variables: &mut BTreeMap<OsString, OsString>, shell: Vec<(OsString, OsString)>) {
    for (key, value) in shell {
        let name = key.to_string_lossy();
        if SESSION_KEYS.contains(&name.as_ref())
            || PROBE_ENVIRONMENT.iter().any(|(probe, _)| *probe == name)
            || LAUNCHER_PREFIXES
                .iter()
                .any(|prefix| name.starts_with(prefix))
        {
            continue;
        }
        variables.insert(key, value);
    }
}

/// `path` with the directories a service command commonly needs appended
/// where it lacks them, so a failed shell read still finds Homebrew and
/// user-installed tools.
fn with_fallback_paths(path: Option<&OsString>, user_home: &Path) -> OsString {
    let mut paths: Vec<PathBuf> = path
        .map(|path| std::env::split_paths(path).collect())
        .unwrap_or_default();
    let fallbacks = [
        user_home.join(".local/bin"),
        user_home.join(".cargo/bin"),
        PathBuf::from("/opt/homebrew/bin"),
        PathBuf::from("/usr/local/bin"),
        PathBuf::from("/usr/bin"),
        PathBuf::from("/bin"),
        PathBuf::from("/usr/sbin"),
        PathBuf::from("/sbin"),
    ];
    for fallback in fallbacks {
        if !paths.contains(&fallback) {
            paths.push(fallback);
        }
    }
    std::env::join_paths(paths).unwrap_or_else(|_| path.cloned().unwrap_or_default())
}

/// The proxy macOS's network settings name, as environment variables: the
/// service's HTTP clients read the environment rather than the system
/// settings.
fn system_proxy() -> Vec<(&'static str, String)> {
    match Command::new("/usr/sbin/scutil").arg("--proxy").output() {
        Ok(output) if output.status.success() => {
            proxy_env_of(&String::from_utf8_lossy(&output.stdout))
        }
        _ => Vec::new(),
    }
}

/// `scutil --proxy` is one `Key : Value` per line, with `ExceptionsList` as a
/// nested block of `index : entry` lines.
fn proxy_env_of(settings: &str) -> Vec<(&'static str, String)> {
    let (mut values, mut exceptions, mut in_exceptions) = (Vec::new(), Vec::new(), false);
    for line in settings.lines().map(str::trim) {
        if line.starts_with("ExceptionsList") {
            in_exceptions = true;
        } else if line == "}" {
            in_exceptions = false;
        } else if let Some((key, value)) = line.split_once(" : ") {
            if in_exceptions {
                exceptions.push(value.strip_prefix('*').unwrap_or(value).to_owned());
            } else {
                values.push((key, value));
            }
        }
    }
    let value = |key: String| {
        values
            .iter()
            .find(|(name, _)| *name == key)
            .map(|(_, value)| *value)
    };
    let proxy = |scheme: &str| {
        (value(format!("{scheme}Enable")) == Some("1"))
            .then(|| {
                let host = value(format!("{scheme}Proxy"))?;
                let port = value(format!("{scheme}Port"))?;
                Some(format!("http://{host}:{port}"))
            })
            .flatten()
    };
    let (http, https) = (proxy("HTTP"), proxy("HTTPS"));
    if http.is_none() && https.is_none() {
        return Vec::new();
    }
    let mut env = Vec::new();
    if let Some(http) = http {
        env.extend([("HTTP_PROXY", http.clone()), ("http_proxy", http)]);
    }
    if let Some(https) = https {
        env.extend([("HTTPS_PROXY", https.clone()), ("https_proxy", https)]);
    }
    let mut bypass = vec![
        "localhost".to_owned(),
        "127.0.0.1".to_owned(),
        "::1".to_owned(),
    ];
    bypass.append(&mut exceptions);
    let bypass = bypass.join(",");
    env.extend([("NO_PROXY", bypass.clone()), ("no_proxy", bypass)]);
    env
}
