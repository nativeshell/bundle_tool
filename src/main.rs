#[allow(dead_code)]
mod error;
mod macos_bundle;
mod utils;

use std::path::PathBuf;

use clap::{AppSettings, Clap};
use macos_bundle::MacOSBundleSelfContained;
use simple_logger::SimpleLogger;

#[derive(Clap)]
#[clap(version = "1.0", author = "Matej Knopp")]
#[clap(setting = AppSettings::ColoredHelp)]
struct Opts {
    /// A level of verbosity, and can be used multiple times
    #[clap(short = 'v', long = "verbose", parse(from_occurrences))]
    verbose: i32,
    #[clap(subcommand)]
    subcmd: SubCommand,
}

#[derive(Clap)]
pub struct MacOSBundleOptions {
    /// Delete bundle in target directory (out-dir/BundleName.app) if already exists
    #[clap(long)]
    delete_existing_bundle: bool,
    /// Path to bundle produced by NativeShell
    source_path: PathBuf,
    /// Output directory
    out_dir: PathBuf,
}

#[derive(Clap)]
enum SubCommand {
    /// Creates a self-contained macOS bundle
    #[clap(name = "macos_bundle")]
    MacOSBundleSelfContained(MacOSBundleOptions),
}

fn main() {
    let opts: Opts = Opts::parse();

    let log_level = match opts.verbose {
        0 => log::LevelFilter::Info,
        1 => log::LevelFilter::Debug,
        _ => log::LevelFilter::Trace,
    };

    SimpleLogger::new().with_level(log_level).init().unwrap();

    let res = match opts.subcmd {
        SubCommand::MacOSBundleSelfContained(options) => {
            MacOSBundleSelfContained::new(options).perform()
        }
    };

    if let Err(error) = res {
        eprintln!("\n** Tool failed with error **\n\n{}", error);
        std::process::exit(1);
    }
}
