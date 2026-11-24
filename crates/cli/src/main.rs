//! Headless collection runner (GAPS-AND-ROADMAP G-12): runs a collection, folder or single
//! request through the same pipeline as the app's Run button (`at_runner::run`), optionally
//! data-driven, and reports for humans and CI. Exit code 0 = everything passed, 1 = a request
//! or test failed, 2 = usage or configuration error.
//!
//! Script changes to environment/global variables are not written back unless
//! `--persist-vars` is given, so a CI run never edits the data dir.

mod setup;

use at_runner::report::{cli as cli_report, Format, Report};
use at_runner::{merge_script_vars, run, RunMsg, RunRow};
use clap::{Parser, Subcommand, ValueEnum};
use std::io::{IsTerminal, Write};
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::{Instant, SystemTime, UNIX_EPOCH};
use tokio_util::sync::CancellationToken;

/// The executable's name, from `[[bin]]` in Cargo.toml (the one place to rename it).
const BIN: &str = env!("CARGO_BIN_NAME");

#[derive(Parser)]
#[command(name = BIN, version, about = "Run API Tester collections from the command line")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Run a collection, a folder of one, or a single request file.
    Run(RunArgs),
}

#[derive(clap::Args)]
pub(crate) struct RunArgs {
    /// Collection or folder directory, a request .toml file, or the name of a collection in
    /// the data dir.
    pub path: PathBuf,
    /// Environment (by name) from the data dir.
    #[arg(short, long, value_name = "NAME")]
    pub env: Option<String>,
    /// Environment TOML file (`name = "..."` and `[[variables]]` with key/value), layered over
    /// --env.
    #[arg(long, value_name = "FILE")]
    pub env_file: Option<PathBuf>,
    /// Globals TOML file (`[[variables]]`), layered over the data dir's globals.
    #[arg(long, value_name = "FILE")]
    pub global_file: Option<PathBuf>,
    /// Data dir to read environments, globals and settings from [default: the app's, or
    /// $API_TESTER_DATA_DIR].
    #[arg(long, value_name = "DIR")]
    pub data_dir: Option<PathBuf>,
    /// Data file: CSV with a header row, or a JSON array of objects. One iteration per row;
    /// each row is `pm.iterationData` and its columns resolve as {{column}}.
    #[arg(short, long, value_name = "FILE")]
    pub data: Option<PathBuf>,
    /// Iterations [default: the data file's row count, else 1]. Past the last data row, the
    /// last row repeats.
    #[arg(short = 'n', long, value_name = "N", value_parser = clap::value_parser!(u64).range(1..))]
    pub iterations: Option<u64>,
    /// Pause between requests, in milliseconds.
    #[arg(long, value_name = "MS", default_value_t = 0)]
    pub delay_ms: u64,
    /// Stop at the first failed request.
    #[arg(long)]
    pub bail: bool,
    /// Timeout for every request, in milliseconds (0 = none) [default: each request's own].
    #[arg(long, value_name = "MS")]
    pub timeout_ms: Option<u64>,
    /// Don't verify TLS certificates.
    #[arg(short = 'k', long)]
    pub insecure: bool,
    /// Report format; repeat for several. `cli` prints to stdout.
    #[arg(short, long, value_enum, value_name = "FORMAT", default_values_t = [Reporter::Cli])]
    pub reporter: Vec<Reporter>,
    /// File for the (single) file report [default: stdout when it is the only reporter,
    /// else <format's file name> in --out-dir or the current dir].
    #[arg(short, long, value_name = "FILE", conflicts_with = "out_dir")]
    pub out: Option<PathBuf>,
    /// Directory for file reports (report.json, junit.xml, report.html).
    #[arg(long, value_name = "DIR")]
    pub out_dir: Option<PathBuf>,
    /// Save script changes to the data dir's environment (--env) and globals.
    #[arg(long)]
    pub persist_vars: bool,
    /// No colour in the cli report (also: NO_COLOR).
    #[arg(long)]
    pub no_color: bool,
}

#[derive(Clone, Copy, PartialEq, Eq, ValueEnum)]
pub(crate) enum Reporter {
    Cli,
    Json,
    Junit,
    Html,
}

impl Reporter {
    fn format(self) -> Format {
        match self {
            Reporter::Cli => Format::Cli,
            Reporter::Json => Format::Json,
            Reporter::Junit => Format::Junit,
            Reporter::Html => Format::Html,
        }
    }
}

fn main() -> ExitCode {
    let Cmd::Run(args) = Cli::parse().cmd;
    match run_cmd(args) {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::from(1),
        Err(e) => {
            eprintln!("{BIN}: {e}");
            ExitCode::from(2)
        }
    }
}

fn use_color(no_color: bool) -> bool {
    if no_color || std::env::var_os("NO_COLOR").is_some_and(|v| !v.is_empty()) {
        return false;
    }
    if !std::io::stdout().is_terminal() {
        return false;
    }
    // Classic Windows consoles show escape codes literally unless a program turns VT mode on;
    // modern terminals announce themselves.
    !cfg!(windows)
        || ["WT_SESSION", "TERM_PROGRAM", "ANSICON", "TERM"]
            .iter()
            .any(|v| std::env::var_os(v).is_some())
}

/// Where each file report goes; `None` = stdout.
fn destinations(args: &RunArgs) -> Result<Vec<(Format, Option<PathBuf>)>, String> {
    let mut files: Vec<Format> = Vec::new();
    for r in &args.reporter {
        let f = r.format();
        if f != Format::Cli && !files.contains(&f) {
            files.push(f);
        }
    }
    let cli = args.reporter.contains(&Reporter::Cli);
    if let Some(out) = &args.out {
        return match files.as_slice() {
            [f] => Ok(vec![(*f, Some(out.clone()))]),
            [] => Err("--out needs a json, junit or html reporter".into()),
            _ => Err("--out takes one report; use --out-dir for several".into()),
        };
    }
    if files.len() == 1 && !cli && args.out_dir.is_none() {
        return Ok(vec![(files[0], None)]);
    }
    let dir = args.out_dir.clone().unwrap_or_default();
    Ok(files
        .into_iter()
        .map(|f| (f, Some(dir.join(f.file_name()))))
        .collect())
}

/// Ok(all passed).
fn run_cmd(args: RunArgs) -> Result<bool, String> {
    let dests = destinations(&args)?;
    let cli = args.reporter.contains(&Reporter::Cli);
    let color = cli && use_color(args.no_color);
    let mut s = setup::prepare(&args)?;
    let name = s.name.clone();

    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(|e| format!("cannot start the async runtime: {e}"))?;
    let started_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    let started = Instant::now();
    let plan = s.plan.take().expect("plan is built once");
    let (tx, rx) = async_channel::unbounded();
    let rows: Vec<RunRow> = rt.block_on(async {
        let task = tokio::spawn(run(plan, CancellationToken::new(), tx));
        let mut rows = Vec::new();
        let mut stdout = std::io::stdout().lock();
        while let Ok(msg) = rx.recv().await {
            match msg {
                RunMsg::Row { row, env, globals } => {
                    if cli {
                        let _ = stdout.write_all(cli_report::row_lines(&row, color).as_bytes());
                        let _ = stdout.flush();
                    }
                    if args.persist_vars {
                        persist(&mut s, &env, &globals);
                    }
                    rows.push(*row);
                }
                RunMsg::Done => break,
            }
        }
        let _ = task.await;
        rows
    });
    let report = Report {
        name: &name,
        rows: &rows,
        started_ms,
        elapsed: started.elapsed(),
    };
    if cli {
        print!("{}", cli_report::totals(&report, color));
    }
    for (format, dest) in dests {
        let text = format.render(&report);
        match dest {
            None => print!("{text}"),
            Some(path) => {
                if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
                    std::fs::create_dir_all(parent)
                        .map_err(|e| format!("{}: {e}", parent.display()))?;
                }
                std::fs::write(&path, text).map_err(|e| format!("{}: {e}", path.display()))?;
            }
        }
    }
    let _ = std::io::stdout().flush();
    Ok(rows.iter().all(|r| !r.failed()))
}

/// `--persist-vars`: fold one request's script changes into the data dir's environment and
/// globals as a diff (same merge the app uses), and save them.
fn persist(
    s: &mut setup::Setup,
    env: &(at_runner::VarMap, at_runner::VarMap),
    globals: &(at_runner::VarMap, at_runner::VarMap),
) {
    let Some(store) = &s.store else { return };
    if let Some((stored, path)) = &mut s.stored_env {
        if let Some(vars) = merge_script_vars(&stored.variables, &env.0, &env.1) {
            stored.variables = vars;
            match store.save_environment(stored, Some(path)) {
                Ok(p) => *path = p,
                Err(e) => eprintln!("{BIN}: environment not saved: {e}"),
            }
        }
    }
    if let Some(vars) = merge_script_vars(&s.stored_globals, &globals.0, &globals.1) {
        s.stored_globals = vars;
        if let Err(e) = store.save_globals(&s.stored_globals) {
            eprintln!("{BIN}: globals not saved: {e}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(extra: &[&str]) -> RunArgs {
        let mut v = vec![BIN, "run", "c"];
        v.extend_from_slice(extra);
        let Cmd::Run(a) = Cli::try_parse_from(v).unwrap().cmd;
        a
    }

    #[test]
    fn report_destinations() {
        assert!(destinations(&args(&[])).unwrap().is_empty(), "cli only");
        assert_eq!(
            destinations(&args(&["-r", "json"])).unwrap(),
            vec![(Format::Json, None)],
            "a lone file report goes to stdout"
        );
        assert_eq!(
            destinations(&args(&["-r", "cli", "-r", "junit"])).unwrap(),
            vec![(Format::Junit, Some(PathBuf::from("junit.xml")))]
        );
        assert_eq!(
            destinations(&args(&["-r", "junit", "-r", "html", "--out-dir", "o"])).unwrap(),
            vec![
                (Format::Junit, Some(PathBuf::from("o").join("junit.xml"))),
                (Format::Html, Some(PathBuf::from("o").join("report.html")))
            ]
        );
        assert_eq!(
            destinations(&args(&["-r", "html", "--out", "r.html"])).unwrap(),
            vec![(Format::Html, Some(PathBuf::from("r.html")))]
        );
        assert!(destinations(&args(&["--out", "x"])).is_err());
        assert!(destinations(&args(&["-r", "json", "-r", "html", "--out", "x"])).is_err());
        assert!(Cli::try_parse_from([BIN, "run", "c", "--out", "a", "--out-dir", "b"]).is_err());
        assert!(Cli::try_parse_from([BIN, "run", "c", "-n", "0"]).is_err());
    }
}
