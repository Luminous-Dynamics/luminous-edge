use clap::Parser;
use rea_rs::{inventory, Coverage, Limits};
use std::{io::{self, Write}, path::PathBuf, process::ExitCode};

#[derive(Debug, Parser)]
#[command(name = "rea-rs", version, about = "Offline, read-only artifact inventory prototype")]
struct Args {
    /// Directory to inventory. The report contains root-relative paths only.
    #[arg(long, default_value = ".")]
    root: PathBuf,

    /// Maximum number of regular files to attempt.
    #[arg(long, default_value_t = 100_000)]
    max_files: usize,

    /// Maximum total child paths admitted (files, links and directories).
    #[arg(long, default_value_t = 250_000)]
    max_paths: usize,

    /// Maximum encoded length of one root-relative path.
    #[arg(long, default_value_t = 1024)]
    max_path_bytes: usize,

    /// Maximum total encoded path bytes admitted during the scan.
    #[arg(long, default_value_t = 16 * 1024 * 1024)]
    max_total_path_bytes: usize,

    /// Maximum size of an individual file to hash.
    #[arg(long, default_value_t = 512 * 1024 * 1024)]
    max_file_bytes: u64,

    /// Maximum total bytes read across this run.
    #[arg(long, default_value_t = 4 * 1024 * 1024 * 1024)]
    max_total_bytes: u64,

    /// Maximum directory depth below the selected root.
    #[arg(long, default_value_t = 64)]
    max_depth: usize,

    /// Maximum directory entries accepted before skipping a directory.
    #[arg(long, default_value_t = 100_000)]
    max_entries_per_directory: usize,
}

fn main() -> ExitCode {
    let args = Args::parse();
    let limits = Limits {
        max_files: args.max_files,
        max_paths: args.max_paths,
        max_path_bytes: args.max_path_bytes,
        max_total_path_bytes: args.max_total_path_bytes,
        max_file_bytes: args.max_file_bytes,
        max_total_bytes: args.max_total_bytes,
        max_depth: args.max_depth,
        max_entries_per_directory: args.max_entries_per_directory,
    };

    let report = match inventory(&args.root, limits) {
        Ok(report) => report,
        Err(error) => {
            eprintln!("rea-rs: cannot inventory selected root: {error}");
            return ExitCode::FAILURE;
        }
    };

    let stdout = io::stdout();
    let mut output = stdout.lock();
    if let Err(error) = serde_json::to_writer_pretty(&mut output, &report) {
        eprintln!("rea-rs: cannot write JSON report: {error}");
        return ExitCode::FAILURE;
    }
    if let Err(error) = output.write_all(b"\n") {
        eprintln!("rea-rs: cannot finish JSON report: {error}");
        return ExitCode::FAILURE;
    }

    match report.coverage {
        Coverage::Complete => ExitCode::SUCCESS,
        Coverage::Partial => ExitCode::from(2),
    }
}
