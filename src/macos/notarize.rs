use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
    time::Instant,
};

use log::{debug, info, trace};
use rand::{distributions::Alphanumeric, thread_rng, Rng};
use scopeguard::defer;

use crate::{
    error::{IOResultExt, PlistResultExt, ToolError, ToolResult},
    utils::run_command,
};

#[derive(clap::Parser)]
pub struct Options {
    /// Path to self-contained code-signed bundle produced by
    /// the macos_bundle and macos_codesign commands
    bundle_path: PathBuf,

    /// User name used during notarization
    #[clap(long)]
    username: String,

    /// Password used during notarization
    #[clap(long)]
    password: String,

    /// Team identifier used during notarization
    #[clap(long, alias = "team")]
    team_id: String,
}

pub struct Notarize {
    options: Options,
}

#[derive(Debug)]
struct NotarizationResult {
    id: String,
    status: String,
}

impl Notarize {
    pub fn new(options: Options) -> Self {
        Self { options }
    }

    pub fn perform(self) -> ToolResult<()> {
        let temp_dir = self.temp_dir()?;
        let compressed_path = self.compress_bundle(&temp_dir)?;
        let now = Instant::now();
        let result = self.notarize(&compressed_path)?;
        debug!("Notarization result ${:?}", result);
        let log = self.fetch_log(&result.id)?;
        info!("Notarization took {:#?}, log:\n{}", now.elapsed(), log);
        if result.status.to_lowercase() != "accepted" {
            return Err(ToolError::OtherError(format!(
                "Notarization failed with status: {}",
                result.status
            )));
        }
        self.staple()?;
        defer! {
            fs::remove_dir_all(&temp_dir).ok();
        }
        Ok(())
    }

    // Returns notarization id
    fn notarize(&self, compressed_path: &Path) -> ToolResult<NotarizationResult> {
        debug!("Submitting bundle for notarization and waiting for response.");

        let mut command = Command::new("xcrun");
        command
            .arg("notarytool")
            .arg("submit")
            .arg("--apple-id")
            .arg(&self.options.username)
            .arg("--password")
            .arg(&self.options.password)
            .arg("--team-id")
            .arg(&self.options.team_id)
            .arg("--output-format")
            .arg("plist")
            .arg("--wait")
            .arg(compressed_path);

        let res = run_command(command, "xcrun")?.join("\n");

        let plist = plist::Value::from_reader_xml(res.as_bytes()).wrap_error(|| None)?;
        if let plist::Value::Dictionary(value) = plist {
            let id = value.get("id");
            let status = value.get("status");
            if let (Some(plist::Value::String(id)), Some(plist::Value::String(status))) =
                (id, status)
            {
                trace!("Bundle successfully submitted. RequestID: {}", id);
                return Ok(NotarizationResult {
                    id: id.clone(),
                    status: status.clone(),
                });
            }
        }

        Err(ToolError::OtherError(format!(
            "Malformed notarization response: {:}",
            res,
        )))
    }

    fn fetch_log(&self, id: &str) -> ToolResult<String> {
        debug!("Fetching notarization log");
        let mut command = Command::new("xcrun");
        command
            .arg("notarytool")
            .arg("log")
            .arg(id)
            .arg("--apple-id")
            .arg(&self.options.username)
            .arg("--password")
            .arg(&self.options.password)
            .arg("--team-id")
            .arg(&self.options.team_id);

        Ok(run_command(command, "xcrun")?.join("\n"))
    }

    fn staple(&self) -> ToolResult<()> {
        debug!("Stapling");
        let mut command = Command::new("xcrun");
        command
            .arg("stapler")
            .arg("staple")
            .arg(&self.options.bundle_path);
        run_command(command, "xcrun")?;
        Ok(())
    }

    fn temp_dir(&self) -> ToolResult<PathBuf> {
        let temp_dir = std::env::temp_dir();
        let rand_string: String = thread_rng()
            .sample_iter(&Alphanumeric)
            .take(10)
            .map(char::from)
            .collect();
        let path = temp_dir.join(rand_string);
        fs::create_dir_all(&path)
            .wrap_error(crate::error::FileOperation::CreateDir, || path.clone())?;
        Ok(path)
    }

    fn compress_bundle(&self, temp_dir: &Path) -> ToolResult<PathBuf> {
        debug!("Compressing bundle");
        let name = format!(
            "{}.zip",
            self.options
                .bundle_path
                .file_name()
                .unwrap()
                .to_string_lossy()
        );

        let compressed_path = temp_dir.join(name);

        let mut command = Command::new("ditto");
        command
            .arg("-c")
            .arg("-k")
            .arg("--sequesterRsrc")
            .arg("--keepParent")
            .arg(&self.options.bundle_path)
            .arg(&compressed_path);

        run_command(command, "ditto")?;

        Ok(compressed_path)
    }
}
