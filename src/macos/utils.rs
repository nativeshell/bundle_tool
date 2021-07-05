use std::{fs::File, io::Read, path::Path};

use is_executable::IsExecutable;

use crate::error::{FileOperation, IOResultExt, ToolResult};

pub(super) fn is_executable_binary(path: &Path) -> ToolResult<bool> {
    if path.is_executable() {
        // Check for MACH-O magic
        let mut f = File::open(path).wrap_error(FileOperation::Open, || path.into())?;
        let mut start = [0; 4];
        let num_read = f
            .read(&mut start)
            .wrap_error(FileOperation::Read, || path.into())?;

        static MAGIC_FAT: &'static [u8] = &[0xca, 0xfe, 0xba, 0xbe];
        static CIGAM_FAT: &'static [u8] = &[0xbe, 0xba, 0xfe, 0xca];
        static MAGIC_64: &'static [u8] = &[0xfe, 0xed, 0xfa, 0xcf];
        static CIGAM_64: &'static [u8] = &[0xcf, 0xfa, 0xed, 0xfe];

        Ok(num_read == 4
            && (start == MAGIC_FAT || start == CIGAM_FAT || start == MAGIC_64 || start == CIGAM_64))
    } else {
        Ok(false)
    }
}
