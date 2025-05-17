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
    Metadata,
}
#[derive(Debug)]
pub enum ToolError {
    Command {
        command: String,
        status: ExitStatus,
        stderr: String,
        stdout: String,
    },
    FileOperation {
        operation: FileOperation,
        path: PathBuf,
        source_path: Option<PathBuf>,
        source: io::Error,
    },
    PathResolve {
        path: String,
        rpaths: Vec<PathBuf>,
    },
    Plist {
        path: Option<PathBuf>,
        error: plist::Error,
    },
    NotarizationFailure {
        log_file_url: Option<String>,
    },
    BundlesNotIdentical,
    OtherError(String),
}

impl Display for ToolError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ToolError::Command {
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
            ToolError::FileOperation {
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
            ToolError::PathResolve { path, rpaths } => {
                write!(f, "Failed to resolve path: {} (rpaths: {:?}", path, rpaths)
            }
            ToolError::Plist { path, error } => {
                write!(f, "PlistError: {} (Path:{:?})", error, path)
            }
            ToolError::NotarizationFailure { log_file_url } => {
                write!(
                    f,
                    "Notarizaiton failed: {}",
                    log_file_url.as_ref().unwrap_or(&"No long available".into())
                )
            }
            ToolError::BundlesNotIdentical => {
                write!(f, "Bundles are not identical")
            }
        }
    }
}

impl std::error::Error for ToolError {}

pub(super) trait IOResultExt<T> {
    fn wrap_error<F>(self, operation: FileOperation, path: F) -> ToolResult<T>
    where
        F: FnOnce() -> PathBuf;
    fn wrap_error_with_src<F, G>(
        self,
        operation: FileOperation,
        path: F,
        source_path: G,
    ) -> ToolResult<T>
    where
        F: FnOnce() -> PathBuf,
        G: FnOnce() -> PathBuf;
}

pub(super) trait PlistResultExt<T> {
    fn wrap_error<F>(self, path: F) -> ToolResult<T>
    where
        F: FnOnce() -> Option<PathBuf>;
}

impl<T> IOResultExt<T> for io::Result<T> {
    fn wrap_error<F>(self, operation: FileOperation, path: F) -> ToolResult<T>
    where
        F: FnOnce() -> PathBuf,
    {
        self.map_err(|e| ToolError::FileOperation {
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
    ) -> ToolResult<T>
    where
        F: FnOnce() -> PathBuf,
        G: FnOnce() -> PathBuf,
    {
        self.map_err(|e| ToolError::FileOperation {
            operation,
            path: path(),
            source_path: Some(source_path()),
            source: e,
        })
    }
}

impl<T> PlistResultExt<T> for Result<T, plist::Error> {
    fn wrap_error<F>(self, path: F) -> ToolResult<T>
    where
        F: FnOnce() -> Option<PathBuf>,
    {
        self.map_err(|e| ToolError::Plist {
            path: path(),
            error: e,
        })
    }
}
