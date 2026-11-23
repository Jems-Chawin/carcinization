//! The command line of this tool, as a function the `carcinify` binary
//! calls.

use carcinify_diff::report::HashSeed;
use std::path::PathBuf;
use std::process::ExitCode;

pub const USAGE: &str = "\
usage: carcinify speed build <speed.json> --out <dir> [options]
       carcinify speed run <speed.json> --wheels <dir> --report <report.json>
                           [--load-log <file>] [options]

Times the port against its original from the Python caller, on the cases
the declaration names, in two steps so that nothing is built while timing:

  build   not under the gate: builds the port as carcinify diff builds it,
          and writes the wheel, its SHA-256 and the target CPU it was built
          for to <dir>. Refuses to run while GATE_RUN_ID is set.
  run     under the gate (LOG=<file> ci/gate.sh carcinify speed run ...):
          checks the wheels against the hashes build wrote, then only times,
          and judges the run's load from the gate's log, --load-log.

options:
  --python <path>       interpreter       [$CARCINIFY_PYTHON, else python3]
  --maturin <path>      build: maturin    [$CARCINIFY_MATURIN, else maturin]
  --hash-seed <n>       run: PYTHONHASHSEED of the timing process, 0 to
                        4294967295 [drawn at random; the report records it]
  --debug-log <file>    run: one line for each step of the timing
  --timeout-secs <n>    run: limit for each target's timing; the driver
                        is killed and the target fails   [120]

exit status: 0 pass, with or without flags; 1 a target refused or failed,
or the run unreliable; 2 refused, or the run could not be carried out";

/// The `speed` subcommand: its arguments, without the program's name.
pub fn main(mut args: impl Iterator<Item = String>) -> ExitCode {
    let step = args.next();
    let result = match step.as_deref() {
        Some("build") => options(args).map(|o| crate::build::main(&o)),
        Some("run") => options(args).map(|o| crate::run::main(&o)),
        _ => Err(match step {
            Some(other) => format!("no step {other:?}: build or run"),
            None => String::new(),
        }),
    };
    match result {
        Ok(code) => code,
        Err(e) => {
            if !e.is_empty() {
                eprintln!("carcinify speed: {e}\n");
            }
            eprintln!("{USAGE}");
            ExitCode::from(2)
        }
    }
}

pub struct Options {
    pub declaration: PathBuf,
    /// build: where the wheels go.
    pub out: Option<PathBuf>,
    pub python: PathBuf,
    pub maturin: PathBuf,
    /// run: where build put them.
    pub wheels: Option<PathBuf>,
    pub report: Option<PathBuf>,
    pub load_log: Option<PathBuf>,
    pub hash_seed: HashSeed,
    pub debug_log: Option<PathBuf>,
    pub timeout: std::time::Duration,
}

fn options(mut args: impl Iterator<Item = String>) -> Result<Options, String> {
    let env =
        |name: &str, default: &str| PathBuf::from(std::env::var_os(name).unwrap_or(default.into()));
    let mut declaration = None;
    let mut opts = Options {
        declaration: PathBuf::new(),
        out: None,
        python: env("CARCINIFY_PYTHON", "python3"),
        maturin: env("CARCINIFY_MATURIN", "maturin"),
        wheels: None,
        report: None,
        load_log: None,
        hash_seed: HashSeed {
            value: carcinify_diff::cli::drawn_seed(),
            chosen: "drawn",
        },
        debug_log: None,
        timeout: std::time::Duration::from_secs(120),
    };
    while let Some(arg) = args.next() {
        let mut value = || args.next().ok_or_else(|| format!("{arg} needs a value"));
        match arg.as_str() {
            "--out" => opts.out = Some(PathBuf::from(value()?)),
            "--python" => opts.python = PathBuf::from(value()?),
            "--maturin" => opts.maturin = PathBuf::from(value()?),
            "--wheels" => opts.wheels = Some(PathBuf::from(value()?)),
            "--report" => opts.report = Some(PathBuf::from(value()?)),
            "--load-log" => opts.load_log = Some(PathBuf::from(value()?)),
            "--debug-log" => opts.debug_log = Some(PathBuf::from(value()?)),
            "--timeout-secs" => {
                let secs: u64 = value()?
                    .parse()
                    .map_err(|_| "--timeout-secs needs a whole number".to_string())?;
                opts.timeout = std::time::Duration::from_secs(secs.max(1));
            }
            "--hash-seed" => {
                let value = value()?.parse().map_err(|_| {
                    "--hash-seed needs a whole number from 0 to 4294967295".to_string()
                })?;
                opts.hash_seed = HashSeed {
                    value,
                    chosen: "given",
                };
            }
            flag if flag.starts_with('-') => return Err(format!("unknown option {flag}")),
            _ if declaration.is_none() => declaration = Some(PathBuf::from(arg)),
            _ => return Err(format!("unexpected argument {arg}")),
        }
    }
    opts.declaration = declaration.ok_or("no declaration file given")?;
    // A relative tool path with a directory in it must survive the build
    // changing directory, as in carcinify diff.
    for tool in [&mut opts.python, &mut opts.maturin] {
        if tool.components().count() > 1 && tool.is_relative() {
            *tool = std::env::current_dir()
                .map_err(|e| e.to_string())?
                .join(&*tool);
        }
    }
    Ok(opts)
}
