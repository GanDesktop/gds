use anyhow::{Context, bail};
use serde::Deserialize;
use serde_json::Value;
use std::path::Path;
use tokio::process::Command;

#[derive(Clone, Debug, Deserialize)]
pub struct Target {
    pub name: String,
    pub host: Option<String>,
}

impl Target {
    pub fn label(&self) -> String {
        self.host.as_ref().map_or_else(
            || self.name.clone(),
            |host| format!("{} · {host}", self.name),
        )
    }

    /// # Errors
    /// Reports Nix evaluation errors or an unsupported output shape.
    pub async fn options(
        &self,
        flake: &str,
        cancel: &tokio_util::sync::CancellationToken,
    ) -> anyhow::Result<Vec<OptionDefinition>> {
        let target = match &self.host {
            None => format!("flake.homeConfigurations.{}", nix_string(&self.name)),
            Some(host) => format!(
                "({inspector}) {{ system = flake.nixosConfigurations.{host}; name = {user}; }}",
                inspector = include_str!("home/assets/nixos-user.nix"),
                host = nix_string(host),
                user = nix_string(&self.name)
            ),
        };
        evaluate(
            flake,
            cancel,
            &format!("({}) ({target})", include_str!("home/assets/options.nix")),
        )
        .await
    }
}

#[derive(Clone, Debug, Deserialize)]
pub struct Evaluated {
    pub available: bool,
    pub value: Option<Presentation>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct Presentation {
    pub kind: String,
    pub data: Value,
}

#[derive(Clone, Debug, Deserialize)]
pub struct OptionDefinition {
    pub path: Vec<String>,
    #[serde(rename = "type")]
    pub type_name: String,
    pub description: String,
    #[serde(rename = "readOnly")]
    pub read_only: bool,
    pub current: Evaluated,
    pub default: Evaluated,
}

impl OptionDefinition {
    pub fn group(&self) -> String {
        self.path
            .iter()
            .take(if self.path.len() > 2 { 2 } else { 1 })
            .cloned()
            .collect::<Vec<_>>()
            .join(".")
    }

    pub fn title(&self) -> String {
        self.path
            .last()
            .map(|name| {
                let mut title = String::new();
                for (index, ch) in name.chars().enumerate() {
                    if ch.is_uppercase() && index > 0 {
                        title.push(' ');
                    }
                    title.push(if index == 0 {
                        ch.to_ascii_uppercase()
                    } else {
                        ch
                    });
                }
                title
            })
            .unwrap_or_default()
    }
}

/// # Errors
/// Reports a missing local flake, Nix errors or invalid output.
pub async fn discover(
    flake: &str,
    cancel: &tokio_util::sync::CancellationToken,
) -> anyhow::Result<Vec<Target>> {
    evaluate(
        flake,
        cancel,
        &format!("({}) flake", include_str!("home/assets/discover.nix")),
    )
    .await
}

// JSON string syntax is compatible here after escaping Nix interpolation.
fn nix_string(value: &str) -> String {
    serde_json::to_string(value)
        .expect("strings serialize")
        .replace("${", "\\${")
}

async fn evaluate<T: serde::de::DeserializeOwned>(
    flake: &str,
    cancel: &tokio_util::sync::CancellationToken,
    body: &str,
) -> anyhow::Result<T> {
    let path = tokio::fs::canonicalize(Path::new(flake))
        .await
        .context("Cannot open the flake directory")?;
    if !tokio::fs::try_exists(path.join("flake.nix")).await? {
        bail!("No flake.nix in {}", path.display());
    }
    let expression = format!(
        "let flake = builtins.getFlake {}; in {body}",
        nix_string(&path.to_string_lossy())
    );
    let mut command = Command::new("nix");
    command
        .args([
            "eval",
            "--json",
            "--impure",
            "--no-write-lock-file",
            "--option",
            "accept-flake-config",
            "false",
            "--expr",
            &expression,
        ])
        .kill_on_drop(true);
    let output = gds::service::task_manager::process::output(
        &mut command,
        cancel,
        std::time::Duration::from_secs(180),
    )
    .await
    .context("Nix evaluation failed or cancelled")?;
    if !output.status.success() {
        bail!("{}", String::from_utf8_lossy(&output.stderr).trim());
    }
    serde_json::from_slice(&output.stdout).context("Invalid Home Manager metadata")
}

#[cfg(test)]
mod tests;
