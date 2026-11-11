use super::*;
use std::{ffi::OsStr, path::Path, time::Duration};

const COMMAND_TIMEOUT: Duration = Duration::from_secs(15);
const STAT_VALUE_COLUMN: usize = 22;

#[derive(Clone, Debug)]
pub struct Baloo {
    control: PathBuf,
    kio: PathBuf,
}

impl Baloo {
    pub async fn connect(commands: &Commands) -> Result<Self, String> {
        let backend = Self {
            control: resolve(commands.control.as_deref(), &["balooctl6", "balooctl"])?,
            kio: resolve(
                commands.kio_client.as_deref(),
                &["kioclient6", "kioclient", "kioclient5"],
            )?,
        };
        run(&backend.control, &["--version"]).await?;
        run(&backend.kio, &["--version"]).await?;
        backend.check().await?;
        // This also verifies that Baloo's tags KIO worker is installed and loadable.
        run(&backend.kio, &["ls", "tags:/"]).await?;
        Ok(backend)
    }

    async fn check(&self) -> Result<(), String> {
        let status = run(&self.control, &["status"]).await?;
        if status.to_ascii_lowercase().contains("disabled") {
            return Err(status.trim().to_owned());
        }
        Ok(())
    }

    async fn list(&self, tag: &str) -> Result<Listing, String> {
        // The upstream tags worker interpolates names into its query string.
        // Baloo 6.31 silently returns no files for a tag containing a quote.
        if tag.contains('"') {
            return Err(
                "The Baloo tags worker cannot reliably query tag names containing a double quote"
                    .into(),
            );
        }
        self.check().await?;
        let uri = crate::entities::location::x::tags::TagAddress::new(tag)?.uri();
        let names = run(&self.kio, &["ls", &uri]).await?;
        let mut listing = Listing::default();
        for name in names.lines().filter(|s| !matches!(*s, "" | "." | "..")) {
            let child = if tag.is_empty() {
                name.to_owned()
            } else {
                format!("{tag}/{name}")
            };
            let child_uri = crate::entities::location::x::tags::TagAddress::new(&child)?.uri();
            let stat = run(&self.kio, &["stat", &child_uri]).await?;
            if let Some(path) = field(&stat, "LOCAL_PATH") {
                let path = PathBuf::from(path);
                if !path.is_absolute() {
                    return Err("Baloo returned a non-local result".into());
                }
                listing.files.push(path);
            } else if matches!(field(&stat, "DISPLAY_TYPE"), Some("Tag" | "Tag Fragment")) {
                listing.tags.push(child);
            } else {
                return Err(format!("Cannot resolve Baloo entry: {child_uri}"));
            }
        }
        listing.tags.sort();
        listing.tags.dedup();
        listing.files.sort();
        listing.files.dedup();
        Ok(listing)
    }
}

impl TagIndex for Baloo {
    fn browse<'a>(&'a self, tag: &'a str) -> Query<'a, Listing> {
        Box::pin(self.list(tag))
    }

    fn delete_tag<'a>(&'a self, tag: &'a str) -> Query<'a, ()> {
        Box::pin(async move {
            if tag.is_empty() || tag.contains('"') {
                return Err("Cannot delete the tag root or a tag containing a double quote".into());
            }
            let address = crate::entities::location::x::tags::TagAddress::new(tag)?;
            self.check().await?;
            // KIO's tags worker removes associations recursively, as in Dolphin.
            run(&self.kio, &["remove", &address.uri()]).await?;
            Ok(())
        })
    }
}

fn field<'a>(stat: &'a str, name: &str) -> Option<&'a str> {
    stat.lines().find_map(|line| {
        let rest = line.strip_prefix(name)?;
        // kioclient uses a 20-column field name followed by two spaces.
        let padding = STAT_VALUE_COLUMN.checked_sub(name.len())?;
        if rest.len() < padding || !rest.as_bytes()[..padding].iter().all(|b| *b == b' ') {
            return None;
        }
        Some(&rest[padding..])
    })
}

fn resolve(configured: Option<&Path>, defaults: &[&str]) -> Result<PathBuf, String> {
    resolve_in_path(
        configured,
        defaults,
        &std::env::var_os("PATH").unwrap_or_default(),
    )
}

fn resolve_in_path(
    configured: Option<&Path>,
    defaults: &[&str],
    search_path: &OsStr,
) -> Result<PathBuf, String> {
    let names: Vec<&OsStr> = configured.map_or_else(
        || defaults.iter().map(OsStr::new).collect(),
        |p| vec![p.as_os_str()],
    );
    for name in names {
        let path = Path::new(name);
        let candidates = if path.components().count() > 1 || path.is_absolute() {
            vec![path.to_owned()]
        } else {
            std::env::split_paths(search_path)
                .map(|dir| dir.join(path))
                .collect()
        };
        for candidate in candidates {
            if executable(&candidate) {
                return candidate.canonicalize().map_err(|e| e.to_string());
            }
        }
    }
    Err(format!(
        "Executable not found: {}",
        configured
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| defaults.join(" / "))
    ))
}

fn executable(path: &Path) -> bool {
    let Ok(metadata) = path.metadata() else {
        return false;
    };
    if !metadata.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        metadata.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    {
        false
    }
}

async fn run(program: &Path, args: &[&str]) -> Result<String, String> {
    let output = tokio::time::timeout(
        COMMAND_TIMEOUT,
        tokio::process::Command::new(program)
            .args(args)
            .env("LC_ALL", "C.UTF-8")
            .env("LANGUAGE", "C")
            .env("QT_QPA_PLATFORM", "offscreen")
            .stdin(std::process::Stdio::null())
            .kill_on_drop(true)
            .output(),
    )
    .await
    .map_err(|_| format!("{} timed out", program.display()))?
    .map_err(|e| format!("{}: {e}", program.display()))?;
    if !output.status.success() {
        return Err(format!(
            "{}: {}{}",
            program.display(),
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    String::from_utf8(output.stdout).map_err(|e| format!("Invalid UTF-8 command output: {e}"))
}

#[cfg(test)]
mod tests;
