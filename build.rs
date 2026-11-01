#[cfg(feature = "ui")]
mod metadata {
    use std::{
        collections::BTreeMap, env, error::Error, fmt::Write as _, fs, path::PathBuf,
        process::Command,
    };

    use serde::Deserialize;

    #[derive(Deserialize)]
    struct Manifest {
        package: Package,
        bin: Vec<Binary>,
    }

    #[derive(Deserialize)]
    struct Binary {
        name: String,
    }

    #[derive(Deserialize)]
    struct Package {
        authors: Vec<String>,
        metadata: Metadata,
    }

    #[derive(Deserialize)]
    struct Metadata {
        gds: Suite,
    }

    #[derive(Deserialize)]
    struct Suite {
        about: About,
    }

    #[derive(Deserialize)]
    #[serde(rename_all = "kebab-case")]
    struct About {
        feedback: String,
        license_file: PathBuf,
        credits: Vec<Credit>,
        applications: BTreeMap<String, Application>,
    }

    #[derive(Deserialize)]
    struct Application {
        name: String,
        icon: String,
    }

    #[derive(Deserialize)]
    struct Credit {
        name: String,
        website: String,
    }

    pub(super) struct Build;

    impl Build {
        pub(super) fn write() -> Result<(), Box<dyn Error>> {
            let manifest: Manifest = toml::from_str(&fs::read_to_string("Cargo.toml")?)?;
            let about = &manifest.package.metadata.gds.about;
            let tag = Self::tag()?;
            println!("cargo:rustc-env=GDS_BUILD_TAG={tag}");
            let mut output = format!(
                "const LICENSE_TEXT: &str = {:?};\npub static APPLICATIONS: &[Application] = &[\n",
                Self::asset(&about.license_file)?
            );
            for (binary, app) in &about.applications {
                if !manifest.bin.iter().any(|target| &target.name == binary) {
                    return Err(format!("About metadata references unknown binary {binary}").into());
                }
                writeln!(
                    output,
                    "Application {{ binary: {:?}, name: {:?}, icon: {:?},",
                    binary, app.name, app.icon
                )?;
                writeln!(
                    output,
                    "version: env!(\"CARGO_PKG_VERSION\"), build_tag: env!(\"GDS_BUILD_TAG\"),"
                )?;
                writeln!(
                    output,
                    "website: env!(\"CARGO_PKG_HOMEPAGE\"), feedback: {:?},",
                    about.feedback
                )?;
                writeln!(
                    output,
                    "license: env!(\"CARGO_PKG_LICENSE\"), license_text: LICENSE_TEXT,"
                )?;
                writeln!(
                    output,
                    "authors: &{:?}, credits: &[",
                    manifest.package.authors
                )?;
                for credit in &about.credits {
                    writeln!(
                        output,
                        "Credit {{ name: {:?}, website: {:?} }},",
                        credit.name, credit.website
                    )?;
                }
                output.push_str("] },\n");
            }
            output.push_str("];\n");
            fs::write(
                PathBuf::from(env::var_os("OUT_DIR").ok_or("Cargo OUT_DIR is missing")?)
                    .join("about.rs"),
                output,
            )?;
            Ok(())
        }

        fn asset(path: &PathBuf) -> Result<String, std::io::Error> {
            println!("cargo:rerun-if-changed={}", path.display());
            fs::read_to_string(path)
        }

        fn git(arguments: &[&str]) -> Option<String> {
            let output = Command::new("git").args(arguments).output().ok()?;
            output
                .status
                .success()
                .then(|| String::from_utf8_lossy(&output.stdout).trim().to_owned())
        }

        fn tag() -> Result<String, Box<dyn Error>> {
            let tag = match env::var("GDS_BUILD_TAG") {
                Ok(tag) => tag,
                Err(env::VarError::NotPresent) => {
                    for reference in ["HEAD", "refs", "packed-refs"] {
                        if let Some(path) = Self::git(&["rev-parse", "--git-path", reference])
                            && PathBuf::from(&path).exists()
                        {
                            println!("cargo:rerun-if-changed={path}");
                        }
                    }
                    Self::git(&["rev-parse", "HEAD"]).unwrap_or_else(|| {
                        println!("cargo:warning=Git revision unavailable; set GDS_BUILD_TAG for source archives");
                        "unknown".into()
                    })
                }
                Err(error) => return Err(error.into()),
            };
            if tag == "unknown" {
                return Ok(tag);
            }
            if tag.len() < 7 || !tag.bytes().all(|byte| byte.is_ascii_hexdigit()) {
                return Err("GDS_BUILD_TAG must be a Git commit hash".into());
            }
            Ok(tag.chars().take(9).collect())
        }
    }
}

fn main() {
    println!("cargo:rerun-if-changed=Cargo.toml");
    println!("cargo:rerun-if-env-changed=GDS_BUILD_TAG");
    #[cfg(feature = "ui")]
    metadata::Build::write().expect("generate application metadata");
}
