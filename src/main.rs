#[allow(dead_code)]
mod error;
mod macos;
mod utils;

use clap::Parser;
use simple_logger::SimpleLogger;

#[derive(Parser)]
#[clap(version = env!("CARGO_PKG_VERSION"), author = "Matej Knopp")]
struct Opts {
    /// A level of verbosity, and can be used multiple times
    #[clap(short = 'v', long = "verbose", parse(from_occurrences))]
    verbose: i32,
    #[clap(subcommand)]
    subcmd: SubCommand,
}

#[derive(Parser)]
#[allow(clippy::enum_variant_names)]
enum SubCommand {
    /// Creates a self-contained macOS bundle
    #[clap(name = "macos-bundle")]
    MacOSBundle(macos::bundle::Options),

    /// Code-signs a self-contained macOS bundle
    #[clap(name = "macos-codesign")]
    MacOSCodesign(macos::codesign::Options),

    #[clap(name = "macos-notarize")]
    MacOSNotarize(macos::notarize::Options),

    #[clap(name = "macos-universal")]
    MacOSUniversal(macos::universal::Options),
}

fn main() {
    let args = std::env::args_os();
    // when running from cargo filter out the bundle-tool argument
    let args = args.enumerate().filter_map(|(x, y)| {
        if x == 1 && y.to_string_lossy() == "bundle-tool" {
            None
        } else {
            Some(y)
        }
    });
    let opts = Opts::parse_from(args);

    let log_level = match opts.verbose {
        0 => log::LevelFilter::Info,
        1 => log::LevelFilter::Debug,
        _ => log::LevelFilter::Trace,
    };

    SimpleLogger::new().with_level(log_level).init().unwrap();

    let res = match opts.subcmd {
        SubCommand::MacOSBundle(options) => macos::bundle::SelfContained::new(options).perform(),
        SubCommand::MacOSCodesign(options) => macos::codesign::CodeSign::new(options).perform(),
        SubCommand::MacOSNotarize(options) => macos::notarize::Notarize::new(options).perform(),
        SubCommand::MacOSUniversal(options) => macos::universal::Universal::new(options).perform(),
    };

    if let Err(error) = res {
        eprintln!("\n** Tool failed with error **\n\n{}", error);
        std::process::exit(1);
    }
}
