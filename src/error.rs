use std::{fmt::Display, io, path::PathBuf, process::ExitStatus};

pub type ToolResult<T> = Result<T, ToolError>;

#[derive(Debug)]
pub enum FileOperation {
    CreateDir,
    Copy,
    Move,
    Remove,
    RemoveDir,
    Read,
    ReadLink,
    Write,
    Open,
    Create,
    SymLink,
    MetaData,
    CopyDir,
    MkDir,
    ReadDir,
    Canonicalize,
    Command,
    Unarchive,
}
#[derive(Debug)]
pub enum ToolError {
    ToolError {
        command: String,
        status: ExitStatus,
        stderr: String,
        stdout: String,
    },
    FileOperationError {
        operation: FileOperation,
        path: PathBuf,
        source_path: Option<PathBuf>,
        source: io::Error,
    },
    PathResolveError {
        path: String,
        rpaths: Vec<PathBuf>,
    },
    OtherError(String),
}

pub type BuildResult<T> = Result<T, ToolError>;

impl Display for ToolError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ToolError::ToolError {
                command,
                status,
                stderr,
                stdout,
            } => {
                write!(
                    f,
                    "External Tool Failed!\nStatus: {:?}\nCommand: {}\nStderr:\n{}\nStdout:\n{}",
                    status, command, stderr, stdout
                )
            }
            ToolError::FileOperationError {
                operation,
                path,
                source_path,
                source,
            } => match source_path {
                Some(source_path) => {
                    write!(
                        f,
                        "File operation failed: {:?}, target path: {:?}, source path: {:?}, error: {}",
                        operation, path, source_path, source
                    )
                }
                None => {
                    write!(
                        f,
                        "File operation failed: {:?}, path: {:?}, error: {}",
                        operation, path, source
                    )
                }
            },
            ToolError::OtherError(err) => {
                write!(f, "{}", err)
            }
            ToolError::PathResolveError { path, rpaths } => {
                write!(f, "Failed to resolve path: {} (rpaths: {:?}", path, rpaths)
            }
        }
    }
}

impl std::error::Error for ToolError {}

pub(super) trait IOResultExt<T> {
    fn wrap_error<F>(self, operation: FileOperation, path: F) -> BuildResult<T>
    where
        F: FnOnce() -> PathBuf;
    fn wrap_error_with_src<F, G>(
        self,
        operation: FileOperation,
        path: F,
        source_path: G,
    ) -> BuildResult<T>
    where
        F: FnOnce() -> PathBuf,
        G: FnOnce() -> PathBuf;
}

impl<T> IOResultExt<T> for io::Result<T> {
    fn wrap_error<F>(self, operation: FileOperation, path: F) -> BuildResult<T>
    where
        F: FnOnce() -> PathBuf,
    {
        self.map_err(|e| ToolError::FileOperationError {
            operation,
            path: path(),
            source_path: None,
            source: e,
        })
    }

    fn wrap_error_with_src<F, G>(
        self,
        operation: FileOperation,
        path: F,
        source_path: G,
    ) -> BuildResult<T>
    where
        F: FnOnce() -> PathBuf,
        G: FnOnce() -> PathBuf,
    {
        self.map_err(|e| ToolError::FileOperationError {
            operation,
            path: path(),
            source_path: Some(source_path()),
            source: e,
        })
    }
}
