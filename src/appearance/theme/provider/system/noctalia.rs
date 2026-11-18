//! Provision and decode palettes rendered by Noctalia v5's template engine.
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

use anti_webui::prelude::Color;
use directories::BaseDirs;
use serde::{Deserialize, Serialize};

use super::super::{Loaded, color::hex_color, read};
use super::material::{self, Parsed};
use crate::appearance::theme::{Error, ModePreference, TerminalColors};

pub(super) const TEMPLATE: &str = include_str!("noctalia/assets/gds.json");
const SCHEMA_VERSION: u64 = 1;
const MANAGED_HEADER: &str =
    "# Automatically managed by GDS. Remove this comment to keep custom edits.\n";

#[cfg(unix)]
const IPC_TIMEOUT: Duration = Duration::from_millis(250);
#[cfg(unix)]
const RENDER_WAIT_BUDGET: Duration = Duration::from_millis(750);
#[cfg(unix)]
const RENDER_POLL_INTERVAL: Duration = Duration::from_millis(10);

#[cfg(any(unix, test))]
pub(super) struct Environment<'a> {
    pub cache_dir: &'a Path,
    pub config_dir: PathBuf,
    pub socket: &'a Path,
}

#[derive(Serialize)]
struct ManagedConfig<'a> {
    theme: ManagedTheme<'a>,
}

#[derive(Serialize)]
struct ManagedTheme<'a> {
    templates: ManagedTemplates<'a>,
}

#[derive(Serialize)]
struct ManagedTemplates<'a> {
    user: ManagedUsers<'a>,
}

#[derive(Serialize)]
struct ManagedUsers<'a> {
    gds: UserTemplate<'a>,
}

#[derive(Serialize)]
struct UserTemplate<'a> {
    input_path: &'a str,
    output_path: &'a str,
}

#[derive(Deserialize)]
struct Export {
    #[serde(default)]
    mode: Option<serde_json::Value>,
    colors: HashMap<String, HashMap<String, String>>,
    #[serde(flatten)]
    extensions: HashMap<String, serde_json::Value>,
}

pub(super) fn output_path(cache_dir: &Path) -> PathBuf {
    cache_dir.join("gds/theme/noctalia.json")
}

#[cfg(unix)]
fn config_dir(dirs: &BaseDirs) -> PathBuf {
    if let Some(root) = std::env::var_os("NOCTALIA_CONFIG_HOME").filter(|value| !value.is_empty()) {
        return PathBuf::from(root).join("noctalia");
    }
    dirs.config_dir().join("noctalia")
}

/// Ensure GDS's bundled Noctalia template is registered and load the rendered palette.
///
/// # Errors
/// Propagates missing rendered exports or invalid Noctalia palette payloads.
#[cfg(unix)]
pub(super) fn load(
    dirs: &BaseDirs,
    socket: &Path,
    preference: ModePreference,
) -> Result<Loaded, Error> {
    load_with(
        &Environment {
            cache_dir: dirs.cache_dir(),
            config_dir: config_dir(dirs),
            socket,
        },
        preference,
    )
}

#[cfg(any(unix, test))]
pub(super) fn load_with(
    env: &Environment<'_>,
    preference: ModePreference,
) -> Result<Loaded, Error> {
    let output = output_path(env.cache_dir);
    let template = env.cache_dir.join("gds/theme/noctalia-template.json");
    let config = env.config_dir.join("gds.toml");

    let template_changed = sync_template(&template);
    let config_changed = sync_config(&config, &template, &output);
    let output_missing = !output.is_file();

    #[cfg(unix)]
    let render_queued = if config_changed || template_changed || output_missing {
        if config_changed {
            let _ = send_ipc(env.socket, "config-reload");
        }
        send_ipc(env.socket, "templates-apply")
    } else {
        false
    };

    #[cfg(not(unix))]
    let _ = (config_changed, template_changed, output_missing, env.socket);

    #[cfg(unix)]
    if output_missing && render_queued {
        wait_for_file(&output, None);
    }

    let first_snapshot = std::fs::read(&output).ok();
    match read(&output).and_then(|contents| parse(&contents, &output, preference)) {
        Ok(parsed) => Ok(Loaded {
            palette: parsed.palette,
            mode: parsed.mode,
            source_path: Some(output),
        }),
        #[cfg(unix)]
        Err(_) if render_queued => {
            wait_for_file(&output, first_snapshot.as_deref());
            let contents = read(&output)?;
            let parsed = parse(&contents, &output, preference)?;
            Ok(Loaded {
                palette: parsed.palette,
                mode: parsed.mode,
                source_path: Some(output),
            })
        }
        Err(error) => {
            let _ = first_snapshot;
            Err(error)
        }
    }
}

#[cfg(any(unix, test))]
fn sync_template(path: &Path) -> bool {
    let file = crate::persistence::File::new(path.to_path_buf());
    let existing = match file.read() {
        Ok(Some(bytes)) if bytes == TEMPLATE.as_bytes() => return false,
        Ok(existing) => existing,
        Err(error) => {
            tracing::debug!(?path, %error, "cannot inspect Noctalia theme template");
            return false;
        }
    };
    if let Err(error) = file.write(existing.as_deref(), TEMPLATE.as_bytes()) {
        tracing::debug!(?path, %error, "cannot stage Noctalia theme template");
        return false;
    }
    true
}

#[cfg(any(unix, test))]
fn render_managed_config(template_path: &Path, output_path: &Path) -> Option<String> {
    let body = toml::to_string(&ManagedConfig {
        theme: ManagedTheme {
            templates: ManagedTemplates {
                user: ManagedUsers {
                    gds: UserTemplate {
                        input_path: template_path.to_str()?,
                        output_path: output_path.to_str()?,
                    },
                },
            },
        },
    })
    .ok()?;
    Some(format!("{MANAGED_HEADER}{body}"))
}

#[cfg(any(unix, test))]
fn sync_config(config_path: &Path, template_path: &Path, output_path: &Path) -> bool {
    let Some(expected) = render_managed_config(template_path, output_path) else {
        return false;
    };
    if std::fs::symlink_metadata(config_path).is_ok_and(|meta| meta.file_type().is_symlink()) {
        return false;
    }
    let file = crate::persistence::File::new(config_path.to_path_buf());
    let existing = match file.read() {
        Ok(None) => None,
        Ok(Some(bytes)) if bytes == expected.as_bytes() => return false,
        Ok(Some(bytes))
            if std::str::from_utf8(&bytes).is_ok_and(|text| text.starts_with(MANAGED_HEADER)) =>
        {
            Some(bytes)
        }
        Ok(Some(_)) => return false,
        Err(error) => {
            tracing::debug!(path = ?config_path, %error, "cannot inspect Noctalia template config");
            return false;
        }
    };
    if let Err(error) = file.write(existing.as_deref(), expected.as_bytes()) {
        tracing::debug!(path = ?config_path, %error, "cannot write Noctalia template config");
        return false;
    }
    true
}

#[cfg(unix)]
fn send_ipc(socket: &Path, command: &str) -> bool {
    use std::{
        io::{Read, Write},
        net::Shutdown,
        os::unix::net::UnixStream,
    };
    let Ok(mut stream) = UnixStream::connect(socket) else {
        return false;
    };
    let _ = stream.set_write_timeout(Some(IPC_TIMEOUT));
    let _ = stream.set_read_timeout(Some(IPC_TIMEOUT));
    if stream.write_all(command.as_bytes()).is_err() || stream.shutdown(Shutdown::Write).is_err() {
        return false;
    }
    let mut response = String::new();
    stream.read_to_string(&mut response).is_ok() && response.starts_with("ok")
}

#[cfg(unix)]
fn wait_for_file(path: &Path, previous: Option<&[u8]>) {
    let deadline = Instant::now() + RENDER_WAIT_BUDGET;
    while Instant::now() < deadline {
        if let Ok(bytes) = std::fs::read(path)
            && !bytes.is_empty()
            && previous != Some(bytes.as_slice())
        {
            return;
        }
        std::thread::sleep(RENDER_POLL_INTERVAL);
    }
}

/// Resolve the requested UI and terminal scheme from a rendered Noctalia v5 export.
///
/// # Errors
/// Rejects malformed JSON, unsupported `schema_version`, invalid active mode when
/// following `System`, missing requested schemes, incomplete role pairs, or partial
/// terminal exports.
pub(super) fn parse(
    contents: &str,
    path: &Path,
    preference: ModePreference,
) -> Result<Parsed, Error> {
    let export: Export = serde_json::from_str(contents).map_err(|source| Error::Json {
        path: path.to_owned(),
        source,
    })?;
    if let Some(version) = export.extensions.get("schema_version")
        && version.as_u64() != Some(SCHEMA_VERSION)
    {
        return Err(Error::invalid(
            "schema_version",
            "unsupported Noctalia theme schema version",
        ));
    }
    let active = export.mode.as_ref().and_then(serde_json::Value::as_str);
    let mode = material::resolve_mode(active, preference, "Noctalia")?;
    let mut palette = material::palette(&export.colors, mode, "Noctalia")?;
    let name = material::scheme_name(mode);
    if let Some(terminal) = export.extensions.get("terminal") {
        // A present terminal section is atomic: never mix partial roles with fallbacks.
        let scheme = terminal
            .get(name)
            .and_then(serde_json::Value::as_object)
            .ok_or_else(|| {
                Error::invalid(
                    format!("terminal.{name}"),
                    "requested Noctalia terminal scheme is missing or not an object",
                )
            })?;
        palette.terminal = TerminalColors {
            foreground: terminal_role(scheme, name, "foreground")?,
            background: terminal_role(scheme, name, "background")?,
            cursor: terminal_role(scheme, name, "cursor")?,
            cursor_text: terminal_role(scheme, name, "cursor_text")?,
            selection: terminal_role(scheme, name, "selection")?,
            selection_text: terminal_role(scheme, name, "selection_text")?,
            normal: terminal_ansi(scheme, name, "normal")?,
            bright: terminal_ansi(scheme, name, "bright")?,
        };
    }
    Ok(Parsed { palette, mode })
}

/// # Errors
/// Rejects missing, non-string, or malformed hexadecimal terminal colors.
fn terminal_role(
    scheme: &serde_json::Map<String, serde_json::Value>,
    scheme_name: &str,
    role: &str,
) -> Result<Color, Error> {
    let field = format!("terminal.{scheme_name}.{role}");
    let value = scheme
        .get(role)
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| {
            Error::invalid(
                &field,
                "required Noctalia terminal color is missing or not a string",
            )
        })?;
    hex_color(value, &field, false)
}

/// # Errors
/// Rejects missing arrays, arrays without all 8 ANSI entries, or invalid hex colors.
fn terminal_ansi(
    scheme: &serde_json::Map<String, serde_json::Value>,
    scheme_name: &str,
    group: &str,
) -> Result<[Color; 8], Error> {
    let field = format!("terminal.{scheme_name}.{group}");
    let entries = scheme
        .get(group)
        .and_then(serde_json::Value::as_array)
        .filter(|entries| entries.len() == 8)
        .ok_or_else(|| {
            Error::invalid(
                &field,
                "expected an array of 8 Noctalia ANSI terminal colors",
            )
        })?;
    let mut colors = [Color::default(); 8];
    for (index, (slot, entry)) in colors.iter_mut().zip(entries).enumerate() {
        let item_field = format!("{field}[{index}]");
        let value = entry.as_str().ok_or_else(|| {
            Error::invalid(
                &item_field,
                "required Noctalia terminal color is missing or not a string",
            )
        })?;
        *slot = hex_color(value, &item_field, false)?;
    }
    Ok(colors)
}

#[cfg(test)]
mod tests;
