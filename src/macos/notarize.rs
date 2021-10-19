use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
    thread::sleep,
    time::{Duration, Instant},
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

    /// Team name used during notarization (optional)
    #[clap(long)]
    team: Option<String>,
}

pub struct Notarize {
    options: Options,
}

impl Notarize {
    pub fn new(options: Options) -> Self {
        Self { options }
    }

    pub fn perform(self) -> ToolResult<()> {
        let bundle_identifier = self.get_bundle_identifier()?;
        let temp_dir = self.temp_dir()?;
        let compressed_path = self.compress_bundle(&temp_dir)?;
        let now = Instant::now();
        let id = self.notarize(&bundle_identifier, &compressed_path)?;
        self.notarize_wait(&id)?;
        self.staple()?;
        info!("Notarization took {:#?}", now.elapsed());
        defer! {
            fs::remove_dir_all(&temp_dir).ok();
        }
        Ok(())
    }

    fn get_bundle_identifier(&self) -> ToolResult<String> {
        let info_plist = self.options.bundle_path.join("Contents").join("Info.plist");
        let plist = plist::Value::from_file(&info_plist).wrap_error(|| Some(info_plist))?;
        if let plist::Value::Dictionary(plist) = plist {
            let identifier = plist.get("CFBundleIdentifier");
            if let Some(plist::Value::String(identifier)) = identifier {
                return Ok(identifier.into());
            }
        }
        Err(ToolError::OtherError("Malformed info.plist".into()))
    }

    // Returns notarization id
    fn notarize(&self, bundle_identifier: &str, compressed_path: &Path) -> ToolResult<String> {
        debug!("Submitting bundle for notarization ({})", bundle_identifier);

        let mut command = Command::new("xcrun");
        command
            .arg("altool")
            .arg("--notarize-app")
            .arg("--primary-bundle-id")
            .arg(bundle_identifier)
            .arg("-u")
            .arg(&self.options.username)
            .arg("-p")
            .arg(&self.options.password)
            .arg("--file")
            .arg(compressed_path)
            .arg("--output-format")
            .arg("xml");
        if let Some(team) = &self.options.team {
            command.arg("--asc-provider").arg(team);
        }

        let res = run_command(command, "xcrun")?.join("\n");
        let plist = plist::Value::from_reader_xml(res.as_bytes()).wrap_error(|| None)?;
        if let plist::Value::Dictionary(value) = plist {
            let upload = value.get("notarization-upload");
            if let Some(plist::Value::Dictionary(upload)) = upload {
                let request = upload.get("RequestUUID");
                if let Some(plist::Value::String(identifier)) = request {
                    trace!("Bundle successfully submitted. RequestID: {}", identifier);
                    return Ok(identifier.into());
                }
            }
        }

        Err(ToolError::OtherError(format!(
            "Malformed notarization response: {:}",
            res,
        )))
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

    fn notarize_wait(&self, request_id: &str) -> ToolResult<()> {
        debug!("Waiting for notarization...");
        let mut attempt = 0;
        loop {
            sleep(Duration::from_secs(20));
            let mut command = Command::new("xcrun");
            command
                .arg("altool")
                .arg("--notarization-info")
                .arg(request_id)
                .arg("-u")
                .arg(&self.options.username)
                .arg("-p")
                .arg(&self.options.password)
                .arg("--output-format")
                .arg("xml");
            attempt += 1;
            trace!("Polling for status (attempt {})", attempt);
            let res = run_command(command, "xcrun")?.join("\n");
            let plist = plist::Value::from_reader_xml(res.as_bytes()).wrap_error(|| None)?;
            let (status, log) = Self::get_status_from_plist(&plist)?;
            trace!("Status is '{}'", status);
            if status == "invalid" {
                return Err(ToolError::NotarizationFailure { log_file_url: log });
            }
            if status == "success" {
                break;
            }
        }
        Ok(())
    }

    fn get_status_from_plist(status: &plist::Value) -> ToolResult<(String, Option<String>)> {
        if let plist::Value::Dictionary(value) = status {
            let upload = value.get("notarization-info");
            if let Some(plist::Value::Dictionary(upload)) = upload {
                let request = upload.get("Status");
                if let Some(plist::Value::String(status)) = request {
                    return Ok((
                        status.into(),
                        upload
                            .get("LogFileURL")
                            .and_then(|v| v.as_string().map(|s| s.into())),
                    ));
                }
            }
        }
        Err(ToolError::OtherError(format!(
            "Malformed notarization status response: {:#?}",
            status,
        )))
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
        return Ok(path);
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
