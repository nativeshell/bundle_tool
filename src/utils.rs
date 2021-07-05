use std::{
    fs::{self, File},
    io::{self, Read},
    os::unix::prelude::MetadataExt,
    path::Path,
    process::Command,
};

use crate::error::{FileOperation, IOResultExt, ToolError, ToolResult};

pub(super) fn run_command(mut command: Command, command_name: &str) -> ToolResult<Vec<String>> {
    let output = command
        .output()
        .wrap_error(FileOperation::Command, || command_name.into())?;

    if !output.status.success() {
        Err(ToolError::ToolError {
            command: format!("{:?}", command),
            status: output.status,
            stderr: String::from_utf8_lossy(&output.stderr).into(),
            stdout: String::from_utf8_lossy(&output.stdout).into(),
        })
    } else {
        let stdout = String::from_utf8_lossy(&output.stdout);
        Ok(stdout.split_terminator('\n').map(|s| s.into()).collect())
    }
}

fn diff_files(f1: &mut File, f2: &mut File) -> bool {
    let buf1: &mut [u8] = &mut [0; 1024];
    let buf2: &mut [u8] = &mut [0; 1024];

    loop {
        match f1.read(buf1) {
            Err(_) => return false,
            Ok(f1_read_len) => match f2.read(buf2) {
                Err(_) => return false,
                Ok(f2_read_len) => {
                    if f1_read_len != f2_read_len {
                        return false;
                    }
                    if f1_read_len == 0 {
                        return true;
                    }
                    if &buf1[0..f1_read_len] != &buf2[0..f2_read_len] {
                        return false;
                    }
                }
            },
        }
    }
}

pub fn is_same(f1: &Path, f2: &Path) -> ToolResult<bool> {
    let s1 = f1
        .metadata()
        .wrap_error(FileOperation::MetaData, || f1.into())?;
    let s2 = f2
        .metadata()
        .wrap_error(FileOperation::MetaData, || f2.into())?;
    if s1.size() != s2.size() {
        return Ok(false);
    }
    let mut f1 = File::open(f1).wrap_error(FileOperation::Open, || f1.into())?;
    let mut f2 = File::open(f2).wrap_error(FileOperation::Open, || f2.into())?;
    Ok(diff_files(&mut f1, &mut f2))
}

// Copies source directory to destination, preserving symlinks
pub fn copy_dir(src_dir: &Path, dest_dir: &Path) -> io::Result<()> {
    fs::create_dir(dest_dir)?;
    for entry in src_dir.read_dir()? {
        let entry = entry?;
        let meta = entry.path().symlink_metadata()?;
        let dest = dest_dir.join(entry.file_name());
        if meta.file_type().is_symlink() {
            let link = entry.path().read_link()?;
            std::os::unix::fs::symlink(&link, &dest)?;
        } else {
            copy(&entry.path(), &dest)?;
        }
    }

    Ok(())
}

pub fn copy(src: &Path, dest: &Path) -> io::Result<()> {
    if src.is_dir() {
        copy_dir(src, &dest)?;
    } else {
        fs::copy(src, dest)?;
    }
    Ok(())
}
