use std::{
    collections::HashMap,
    fmt::Display,
    fs::{self, File},
    io::Read,
    path::{Path, PathBuf},
    process::Command,
};

use is_executable::IsExecutable;
use log::{debug, trace};

use crate::{
    error::{FileOperation, IOResultExt, ToolError, ToolResult},
    utils::{copy, is_same, run_command},
    MacOSBundleOptions,
};

pub struct MacOSBundleSelfContained {
    options: MacOSBundleOptions,
    out_path: PathBuf,
    executables: Vec<PathBuf>,
    processed_libraries: HashMap<ModulePath, PathBuf>,
}

impl MacOSBundleSelfContained {
    pub fn new(options: MacOSBundleOptions) -> Self {
        Self {
            options,
            out_path: PathBuf::new(),
            executables: Vec::new(),
            processed_libraries: HashMap::new(),
        }
    }

    pub fn perform(mut self) -> ToolResult<()> {
        if !self.options.source_path.is_dir() {
            return Err(ToolError::OtherError(
                "Source-path is not a valid folder.".into(),
            ));
        }

        if !self.options.out_dir.is_dir() {
            return Err(ToolError::OtherError(
                "Out-dir is not a valid folder".into(),
            ));
        }

        self.out_path = self
            .options
            .out_dir
            .join(self.options.source_path.file_name().unwrap());
        if self.out_path.exists() {
            if self.options.delete_existing_bundle {
                fs::remove_dir_all(&self.out_path)
                    .wrap_error(FileOperation::RemoveDir, || self.out_path.clone())?;
            } else {
                return Err(ToolError::OtherError(format!(
                    "Target folder {:?} already exists. Please delete it first.",
                    self.out_path
                )));
            }
        }

        fs::create_dir(&self.out_path)
            .wrap_error(FileOperation::MkDir, || self.out_path.clone())?;

        self.process_dir(&self.options.source_path.clone(), &self.out_path.clone())?;

        let executable = self.executables.clone();
        for b in executable {
            self.process_executable(&b)?;
        }

        Ok(())
    }

    fn process_dir(&mut self, src_dir: &Path, dst_dir: &Path) -> ToolResult<()> {
        for entry in src_dir
            .read_dir()
            .wrap_error(FileOperation::ReadDir, || src_dir.into())?
        {
            let entry = entry.wrap_error(FileOperation::Read, || src_dir.into())?;
            let dest = dst_dir.join(entry.file_name());
            let meta = entry
                .path()
                .symlink_metadata()
                .wrap_error(FileOperation::MetaData, || entry.path().clone())?;

            if src_dir.file_name().unwrap() == "Contents" && entry.file_name() == "Frameworks" {
                // Frameworks are handled separately (while processing binaries)
                debug!("{:?}: ignoring frameworks path", src_dir);
                continue;
            }

            if meta.file_type().is_symlink() {
                // copy the symlink and see if it resolves within the bundle
                let link = entry
                    .path()
                    .read_link()
                    .wrap_error(FileOperation::ReadLink, || entry.path())?;
                std::os::unix::fs::symlink(&link, &dest)
                    .wrap_error(FileOperation::SymLink, || dest.clone())?;
                let dest_resolved = dest
                    .canonicalize()
                    .wrap_error(FileOperation::Canonicalize, || dest.clone())?;
                let bundle_path_resolved = self
                    .out_path
                    .canonicalize()
                    .wrap_error(FileOperation::Canonicalize, || self.out_path.clone())?;
                if dest_resolved.starts_with(bundle_path_resolved) {
                    debug!("{:?}: preserving symlink", entry.path());
                    continue;
                }
                fs::remove_file(&dest).wrap_error(FileOperation::Remove, || dest.clone())?;
            }

            let src_resolved = entry
                .path()
                .canonicalize()
                .wrap_error(FileOperation::Canonicalize, || entry.path().clone())?;

            if src_resolved.is_dir() {
                fs::create_dir(&dest).wrap_error(FileOperation::CreateDir, || dest.clone())?;
                debug!("{:?}: create directory", entry.path());
                self.process_dir(&entry.path(), &dest)?;
                continue;
            } else {
                fs::copy(&src_resolved, &dest).wrap_error_with_src(
                    FileOperation::Copy,
                    || dest.clone(),
                    || entry.path().clone(),
                )?;
                if !is_executable_binary(&src_resolved)? {
                    debug!("{:?}: copy", entry.path());
                } else {
                    debug!("{:?}: copy binary", entry.path());
                    self.executables.push(entry.path().clone())
                }
                continue;
            }
        }
        Ok(())
    }

    fn process_executable(&mut self, executable: &Path) -> ToolResult<()> {
        debug!("Processing executable: {:?}", executable);
        let relative = pathdiff::diff_paths(executable, &self.options.source_path).unwrap();
        let executable = executable
            .canonicalize()
            .wrap_error(FileOperation::Canonicalize, || executable.into())?;
        let rpath = executable.parent().unwrap();
        let path_resolver = PathResolver::new(vec![rpath]);
        let module = load_executable(executable.clone())?;

        let target_executable_path = self.out_path.join(&relative);
        self.process_module(&target_executable_path, &module, &path_resolver)?;
        let has_local_dependencies = module.dependencies.iter().any(|d| !d.is_system());
        if has_local_dependencies {
            // Add rpath
            let frameworks_path = self.out_path.join("Contents").join("Frameworks");
            let rpath =
                pathdiff::diff_paths(frameworks_path, target_executable_path.parent().unwrap())
                    .unwrap();
            let mut cmd = Command::new("install_name_tool");
            cmd.arg("-add_rpath")
                .arg(rpath)
                .arg(&target_executable_path);
            run_command(cmd, "install_name_tool")?;
        }
        Ok(())
    }

    fn process_module(
        &mut self,
        target_module_path: &Path,
        module: &Module,
        path_resolver: &PathResolver,
    ) -> ToolResult<()> {
        let mut paths_to_change = Vec::<(ModulePath, ModulePath)>::new();
        for dependency in &module.dependencies {
            if dependency.is_system() {
                continue;
            }
            let new_path = self.process_dependency(dependency, path_resolver)?;
            if &new_path != dependency {
                paths_to_change.push((dependency.clone(), new_path));
            }
        }
        if !paths_to_change.is_empty() {
            debug!(
                "Changing paths for {:?}: {:?}",
                module.path, paths_to_change
            );
            let mut cmd = Command::new("install_name_tool");
            for (from, to) in &paths_to_change {
                cmd.arg("-change").arg(&from.0).arg(&to.0);
            }
            cmd.arg(target_module_path);
            run_command(cmd, "install_name_tool")?;
        }
        Ok(())
    }

    fn process_dependency(
        &mut self,
        dependency: &ModulePath,
        path_resolver: &PathResolver,
    ) -> ToolResult<ModulePath> {
        let resolved = path_resolver.resolve(dependency)?;
        let root = find_dependency_root(&resolved);
        let relative_path = pathdiff::diff_paths(&resolved, root.parent().unwrap()).unwrap();
        let new_module_path =
            ModulePath::new(format!("@rpath/{}", relative_path.to_string_lossy()));
        if let Some(existing) = self.processed_libraries.get(&new_module_path) {
            if !is_same(&resolved, &existing)? {
                return Err(ToolError::OtherError(format!(
                    "Trying to bundle two different version of single framework: {:?}, {:?}",
                    resolved, existing
                )));
            }
            trace!("Dependency {:?} - skipping", relative_path);
        } else {
            debug!("Dependency {:?} - processing", relative_path);
            self.processed_libraries
                .insert(new_module_path.clone(), resolved.clone());
            let library = load_library(resolved.clone())?;
            let frameworks_path = self.out_path.join("Contents").join("Frameworks");
            fs::create_dir_all(&frameworks_path)
                .wrap_error(FileOperation::MkDir, || frameworks_path.clone())?;
            let copy_target = frameworks_path.join(root.file_name().unwrap());
            let real_root = root
                .canonicalize()
                .wrap_error(FileOperation::Canonicalize, || root.clone())?;

            copy(&real_root, &copy_target).wrap_error_with_src(
                FileOperation::Copy,
                || root.clone(),
                || copy_target.clone(),
            )?;

            let target_module_path = frameworks_path.join(&relative_path);
            self.process_module(&target_module_path, &library.module, path_resolver)?;
            if library.install_name != new_module_path {
                let mut cmd = Command::new("install_name_tool");
                cmd.arg("-id")
                    .arg(&new_module_path.0)
                    .arg(&frameworks_path.join(&relative_path));
                run_command(cmd, "install_name_tool")?;
            }
        }
        Ok(new_module_path)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ModulePath(String);

impl ModulePath {
    pub fn new(path: String) -> Self {
        ModulePath(path)
    }

    pub fn is_system(&self) -> bool {
        return self.0.starts_with("/usr/")
            || self.0.starts_with("/lib/")
            || self.0.starts_with("/System/");
    }
}

impl Display for ModulePath {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

pub struct PathResolver<'a> {
    rpaths: Vec<&'a Path>,
}

impl<'a> PathResolver<'a> {
    pub fn new(rpaths: Vec<&'a Path>) -> Self {
        Self { rpaths }
    }

    pub fn resolve(&self, path: &ModulePath) -> ToolResult<PathBuf> {
        let p = PathBuf::from(&path.0);
        if p.exists() {
            Ok(p)
        } else {
            for rpath in &self.rpaths {
                let replaced =
                    PathBuf::from(&str::replace(&path.0, "@rpath", &rpath.to_string_lossy()));
                if replaced.exists() {
                    return Ok(replaced);
                }
            }
            Err(ToolError::PathResolveError {
                path: path.clone(),
                rpaths: self.rpaths.iter().map(|p| p.into()).collect(),
            })
        }
    }
}

#[derive(Debug)]
struct Library {
    module: Module,
    install_name: ModulePath,
}

#[derive(Debug)]
struct Module {
    path: PathBuf,
    dependencies: Vec<ModulePath>,
}

fn load_library(path: PathBuf) -> ToolResult<Library> {
    let mut paths = find_module_paths(&path)?;

    if paths.is_empty() {
        Err(ToolError::OtherError(format!(
            "Invalid otool -L output for {:?}",
            path
        )))
    } else {
        let install_name = paths.remove(0);
        Ok(Library {
            install_name,
            module: Module {
                path,
                dependencies: paths,
            },
        })
    }
}

fn load_executable(path: PathBuf) -> ToolResult<Module> {
    let paths = find_module_paths(&path)?;

    if paths.is_empty() {
        Err(ToolError::OtherError(format!(
            "Invalid otool -L output for {:?}",
            path
        )))
    } else {
        Ok(Module {
            path,
            dependencies: paths,
        })
    }
}

fn find_module_paths(path: &Path) -> ToolResult<Vec<ModulePath>> {
    let mut cmd = Command::new("otool");
    cmd.arg("-L").arg(&path.to_string_lossy().to_string());
    let lines = run_command(cmd, "otool")?;
    let mut iter = lines.into_iter();
    iter.next();
    iter.map(|f| extract_module_path(f)).collect()
}

fn extract_module_path(line: String) -> ToolResult<ModulePath> {
    let line = line.trim();
    let index = line.find(' ');
    match index {
        Some(index) => Ok(ModulePath(line[0..index].into())),
        None => Err(ToolError::OtherError(format!(
            "Malformed otool -L output: {}",
            line
        ))),
    }
}

fn is_executable_binary(path: &Path) -> ToolResult<bool> {
    if path.is_executable() {
        // Check for shebang
        let mut f = File::open(path).wrap_error(FileOperation::Open, || path.into())?;
        let mut start = [0; 2];
        let num_read = f
            .read(&mut start)
            .wrap_error(FileOperation::Read, || path.into())?;
        Ok(num_read == 2 && start[0] != b'#' && start[1] != b'!')
    } else {
        Ok(false)
    }
}

fn find_dependency_root(path: &Path) -> PathBuf {
    if let Some(parent) = path.parent() {
        if parent
            .file_name()
            .unwrap()
            .to_string_lossy()
            .ends_with(".framework")
        {
            return parent.into();
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
                        return parent.into();
                    }
                }
            }
        }
    }
    path.into()
}
