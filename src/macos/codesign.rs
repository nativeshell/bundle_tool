use std::{
    collections::HashSet,
    path::{Path, PathBuf},
    process::Command,
};

use log::debug;

use crate::{
    error::{FileOperation, IOResultExt, PlistResultExt, ToolError, ToolResult},
    utils::run_command,
};

use super::utils::is_executable_binary;

#[derive(clap::Clap)]
pub struct Options {
    /// Path to self-contained bundle produced by the macos_bundle command
    bundle_path: PathBuf,

    /// Path to the entitlements file
    #[clap(long)]
    entitlements: PathBuf,

    /// Identity used during the codesigning process
    #[clap(long)]
    identity: String,
}

pub struct CodeSign {
    options: Options,
    done: HashSet<PathBuf>,
}

impl CodeSign {
    pub fn new(options: Options) -> Self {
        Self {
            options,
            done: HashSet::new(),
        }
    }

    pub fn perform(mut self) -> ToolResult<()> {
        let bundle_path = self.options.bundle_path.clone();
        self.process_app_bundle(&bundle_path)
    }

    fn process_app_bundle(&mut self, path: &Path) -> ToolResult<()> {
        if !is_app_bundle(path) {
            return Err(ToolError::OtherError(format!(
                "Path \"{:?}\" is not an app bundle",
                path,
            )));
        }
        self.process_folder(path)?;
        self.codesign(path, true)?;
        Ok(())
    }

    fn process_framework_bundle(&mut self, path: &Path) -> ToolResult<()> {
        if !is_framework_bundle(path) {
            return Err(ToolError::OtherError(format!(
                "Path \"{:?}\" is not a framework bundle",
                path,
            )));
        }
        self.process_folder(path)?;
        self.codesign(path, false)?;
        Ok(())
    }

    fn process_folder(&mut self, path: &Path) -> ToolResult<()> {
        for entry in path
            .read_dir()
            .wrap_error(FileOperation::ReadDir, || path.into())?
        {
            let entry = entry.wrap_error(FileOperation::Read, || path.into())?;
            let path = &entry.path();

            if path.is_dir() {
                if is_app_bundle(path) {
                    self.process_app_bundle(path)?;
                } else if is_framework_bundle(path) {
                    self.process_framework_bundle(path)?;
                } else {
                    self.process_folder(path)?;
                }
            } else if is_executable_binary(path)? {
                // ignore bundle executables and framework dylibs
                if is_bundle_executable(path)? {
                    continue;
                }
                if is_framework_dylib(path) {
                    continue;
                }
                self.codesign(path, false)?;
            }
        }

        Ok(())
    }

    fn codesign(&mut self, path: &Path, is_app_bundle: bool) -> ToolResult<()> {
        let resolved = path
            .canonicalize()
            .wrap_error(FileOperation::Canonicalize, || path.into())?;

        if self.done.contains(&resolved) {
            debug!("Skipping {:?}", path);
            return Ok(());
        }

        debug!("Codesigning {:?}", resolved);
        let mut command = Command::new("codesign");
        command //
            .arg("-o")
            .arg("runtime")
            .arg("--timestamp");
        if is_app_bundle {
            command
                .arg("--entitlements")
                .arg(&self.options.entitlements);
        }
        command
            .arg("-f")
            .arg("-s")
            .arg(&self.options.identity)
            .arg(&resolved);
        run_command(command, "codesign")?;

        self.done.insert(resolved);

        Ok(())
    }
}

fn is_in_framework(path: &Path) -> bool {
    path.parent().map(|p| p.ends_with("Contents/Frameworks")) == Some(true)
}

fn is_framework_dylib(path: &Path) -> bool {
    if let Some(parent) = path.parent() {
        if parent
            .file_name()
            .unwrap()
            .to_string_lossy()
            .ends_with(".framework")
        {
            return is_in_framework(parent);
        }
        if let Some(parent) = parent.parent() {
            if parent.file_name().unwrap() == "Versions" {
                if let Some(parent) = parent.parent() {
                    if parent
                        .file_name()
                        .unwrap()
                        .to_string_lossy()
                        .ends_with(".framework")
                    {
                        return is_in_framework(parent);
                    }
                }
            }
        }
    }
    is_in_framework(path)
}

fn is_bundle_executable(path: &Path) -> ToolResult<bool> {
    if let Some(parent) = path.parent() {
        if parent.file_name().unwrap() == "MacOS" {
            if let Some(parent) = parent.parent() {
                if parent.file_name().unwrap() == "Contents" {
                    let info_plist = parent.join("Info.plist");
                    let bundle_executable = get_bundle_executable(&info_plist)?;
                    if bundle_executable != path.file_name().unwrap().to_string_lossy() {
                        return Ok(false);
                    }
                    if let Some(parent) = parent.parent() {
                        return Ok(is_app_bundle(parent));
                    }
                }
            }
        }
    }
    Ok(false)
}

fn get_bundle_executable(info_plist: &Path) -> ToolResult<String> {
    let plist = plist::Value::from_file(&info_plist).wrap_error(|| Some(info_plist.into()))?;
    if let plist::Value::Dictionary(plist) = plist {
        let identifier = plist.get("CFBundleExecutable");
        if let Some(plist::Value::String(identifier)) = identifier {
            return Ok(identifier.into());
        }
    }
    Err(ToolError::OtherError("Malformed info.plist".into()))
}

fn is_app_bundle(path: &Path) -> bool {
    path.extension().map(|s| s.to_string_lossy()) == Some("app".into())
        && path.join("Contents/Info.plist").is_file()
}

fn is_framework_bundle(path: &Path) -> bool {
    path.extension().map(|s| s.to_string_lossy()) == Some("framework".into())
}
