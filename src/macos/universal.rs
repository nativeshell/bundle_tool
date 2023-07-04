use std::{
    fs,
    os::unix::prelude::MetadataExt,
    path::{Path, PathBuf},
};

use crate::error::{FileOperation, IOResultExt, ToolError, ToolResult};

use super::utils::is_executable_binary;

#[derive(clap::Parser)]
pub struct Options {
    /// Input paths
    #[clap(required = true)]
    paths_in: Vec<PathBuf>,

    /// Output path for lipo-ed bundle
    #[clap(long)]
    out: PathBuf,

    /// Delete bundle in target directory (out-dir/BundleName.app) if already exists
    #[clap(long)]
    delete_existing_bundle: bool,
}

pub struct Universal {
    options: Options,
}

impl Universal {
    pub fn new(options: Options) -> Self {
        Self { options }
    }

    pub fn perform(self) -> ToolResult<()> {
        for path in &self.options.paths_in {
            if !path.exists() {
                return Err(ToolError::OtherError(format!(
                    "Path \"{:?}\" does not exist",
                    path,
                )));
            }
        }

        if self.options.out.exists() {
            if self.options.delete_existing_bundle {
                fs::remove_dir_all(&self.options.out)
                    .wrap_error(FileOperation::RemoveDir, || self.options.out.clone())?;
            } else {
                return Err(ToolError::OtherError(format!(
                    "Target folder {:?} already exists. Please delete it first.",
                    self.options.out
                )));
            }
        }

        fs::create_dir_all(&self.options.out)
            .wrap_error(FileOperation::MkDir, || self.options.out.clone())?;

        Self::process_dir(&self.options.paths_in, &self.options.out)
    }

    // This is for checking whether binaries are same across all bundles, for which
    // we assume already lipo-ed binary. This only checks file size, chance of binary
    // having identical sized for different architecture is very low.
    fn are_files_same(paths: &[PathBuf]) -> ToolResult<bool> {
        let meta_data = paths
            .iter()
            .map(|p| {
                p.metadata()
                    .wrap_error(FileOperation::MetaData, || p.into())
            })
            .collect::<ToolResult<Vec<_>>>()?;
        let mut sizes = meta_data.iter().map(|f| f.size());
        let first_size = sizes.clone().next().unwrap();
        Ok(sizes.all(|s| s == first_size))
    }

    fn process_dir(paths_in: &[PathBuf], path_out: &Path) -> ToolResult<()> {
        let path = &paths_in[0];
        let paths_rest = &paths_in[1..];
        for entry in path
            .read_dir()
            .wrap_error(FileOperation::ReadDir, || path.into())?
        {
            let entry = entry.wrap_error(FileOperation::Read, || path.into())?;
            let dest = path_out.join(entry.file_name());
            let path = entry.path();
            let meta = entry
                .path()
                .symlink_metadata()
                .wrap_error(FileOperation::MetaData, || entry.path())?;

            let paths = {
                let mut paths_rest = paths_rest
                    .iter()
                    .map(|a| a.join(entry.file_name()))
                    .collect::<Vec<_>>();

                if paths_rest.iter().any(|f| !f.exists()) {
                    return Err(ToolError::BundlesNotIdentical);
                }
                let mut paths: Vec<PathBuf> = vec![path.clone()];
                paths.append(&mut paths_rest);
                paths
            };

            if meta.is_symlink() {
                let link = path
                    .read_link()
                    .wrap_error(FileOperation::ReadLink, || entry.path())?;
                // Assume that symlinks are relative to the bundle root
                // TODO(knopp): Check and enforce this
                std::os::unix::fs::symlink(&link, &dest)
                    .wrap_error(FileOperation::SymLink, || dest.clone())?;
            } else if meta.is_dir() {
                fs::create_dir(&dest).wrap_error(FileOperation::CreateDir, || dest.clone())?;
                Self::process_dir(&paths, &dest)?;
            } else if is_executable_binary(&path)? && !Self::are_files_same(&paths)? {
                let mut cmd = std::process::Command::new("lipo");
                cmd.arg("-create");
                cmd.args(&paths);
                cmd.arg("-output");
                cmd.arg(&dest);
                let status = cmd
                    .status()
                    .wrap_error(FileOperation::Command, || dest.clone())?;
                if !status.success() {
                    return Err(ToolError::Command {
                        command: "lipo".into(),
                        status,
                        stderr: String::new(),
                        stdout: String::new(),
                    });
                }
            } else {
                fs::copy(&path, &dest).wrap_error_with_src(
                    FileOperation::Copy,
                    || dest.clone(),
                    || path.clone(),
                )?;
            }
        }

        Ok(())
    }
}
