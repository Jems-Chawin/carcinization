//! Raw facts about the machine a timing ran on, and the target CPU its
//! build was made for (design §6).
//!
//! Each fact is what the system or `rustc` says, or `unknown` when nothing
//! answers. None is a judgement: there is no "virtualized" or "container"
//! verdict, because in Docker's VM on a Mac nothing visible proves either.

/// A fact's name and its raw value.
pub type Fact = (&'static str, String);

/// The machine's facts: the system, its kernel release, its CPU, page size
/// and CPU count, and on Linux whether `/.dockerenv` exists.
pub fn machine() -> Vec<Fact> {
    let read = |path: &str| std::fs::read_to_string(path).ok();
    match std::env::consts::OS {
        "linux" => linux(&read, &output),
        "macos" => macos(&output),
        os => vec![("os", os.to_string())],
    }
}

/// The build's target CPU and its target features, for the command Cargo
/// ran rustc with (see [`rustc_args`]), each asked of that same rustc: the
/// CPU the last `-C target-cpu` names, else "default of <triple>: <name>",
/// the name being `unknown` where `rustc --print target-cpus` marks no
/// default; the features `rustc --print cfg` gives for the same CPU,
/// features and target; and the rustc's version.
pub fn target(rustc_command: &[String]) -> Vec<Fact> {
    target_with(rustc_command, &|command: &[String]| {
        let command: Vec<&str> = command.iter().map(String::as_str).collect();
        output(&command)
    })
}

/// The command Cargo ran rustc with for the crate `name` (its library
/// name), from a build's verbose output (`cargo build -v`, `maturin build
/// -v`): the rustc, which is the word before `--crate-name` (a
/// RUSTC_WRAPPER comes before it), then the effective flags, whether they
/// came from `RUSTFLAGS` or a Cargo configuration file. None when Cargo
/// compiled no such crate (it prints "Fresh" for a crate it did not
/// rebuild).
///
/// shortcut: split at spaces, though Cargo quotes an argument holding one;
/// the options read here hold none.
pub fn rustc_args(verbose: &str, name: &str) -> Option<Vec<String>> {
    verbose.lines().rev().find_map(|line| {
        let command = line.trim().strip_prefix("Running `")?.strip_suffix('`')?;
        let args: Vec<String> = command.split_whitespace().map(String::from).collect();
        let at = args
            .windows(2)
            .position(|w| w[0] == "--crate-name" && w[1] == name)?;
        Some(args[at.checked_sub(1)?..].to_vec())
    })
}

/// RUST-191: the options of the command Cargo ran rustc with that change
/// the code it makes, in the command's order: each `-C` as `-C name=value`,
/// and each `--cfg feature=…`. Those that name a hash or a path, or only
/// link, are left out: they differ by build directory or by platform (maturin
/// links a macOS module with arguments of its own) and change no code.
pub fn code_options(rustc_command: &[String]) -> Vec<String> {
    const NOT_CODE: [&str; 10] = [
        "metadata",
        "extra-filename",
        "incremental",
        "linker",
        "linker-flavor",
        "link-arg",
        "link-args",
        "link-self-contained",
        "prefer-dynamic",
        "rpath",
    ];
    // Cargo puts an argument the shell would split or expand in single
    // quotes; the options read here hold no quote of their own.
    let args: Vec<&str> = rustc_command
        .iter()
        .map(|a| {
            a.strip_prefix('\'')
                .and_then(|a| a.strip_suffix('\''))
                .unwrap_or(a)
        })
        .collect();
    let mut out = Vec::new();
    for (i, a) in args.iter().enumerate() {
        let next = || args.get(i + 1).copied();
        let option = match *a {
            "-C" | "--codegen" => next(),
            "--cfg" => {
                out.extend(
                    next()
                        .filter(|c| c.starts_with("feature="))
                        .map(|c| format!("--cfg {c}")),
                );
                None
            }
            _ => a
                .strip_prefix("-C")
                .or_else(|| a.strip_prefix("--codegen=")),
        };
        if let Some(o) = option.filter(|o| !NOT_CODE.contains(&o.split('=').next().unwrap_or(o))) {
            out.push(format!("-C {o}"));
        }
    }
    out
}

/// A command's standard output, trimmed, if it succeeded and said anything.
fn output(args: &[&str]) -> Option<String> {
    let out = std::process::Command::new(args[0])
        .args(&args[1..])
        .output()
        .ok()?;
    let text = String::from_utf8_lossy(&out.stdout).trim().to_string();
    (out.status.success() && !text.is_empty()).then_some(text)
}

fn or_unknown(v: Option<String>) -> String {
    v.map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "unknown".into())
}

fn linux(
    read: &dyn Fn(&str) -> Option<String>,
    run: &dyn Fn(&[&str]) -> Option<String>,
) -> Vec<Fact> {
    let cpuinfo = read("/proc/cpuinfo").unwrap_or_default();
    let field = |name: &str| {
        cpuinfo.lines().find_map(|l| {
            let (k, v) = l.split_once(':')?;
            (k.trim() == name).then(|| v.trim().to_string())
        })
    };
    // Arm reports an implementer and a part number; x86 a model name.
    let cpu = match (field("CPU implementer"), field("CPU part")) {
        (Some(implementer), Some(part)) => Some(format!("implementer {implementer}, part {part}")),
        _ => field("model name"),
    };
    let dockerenv = if read("/.dockerenv").is_some() {
        "present"
    } else {
        "absent"
    };
    vec![
        ("os", "linux".into()),
        (
            "kernel_release",
            or_unknown(read("/proc/sys/kernel/osrelease")),
        ),
        ("cpu", or_unknown(cpu)),
        ("page_size", or_unknown(run(&["getconf", "PAGESIZE"]))),
        ("cpus", or_unknown(run(&["getconf", "_NPROCESSORS_ONLN"]))),
        ("dockerenv", dockerenv.into()),
    ]
}

fn macos(run: &dyn Fn(&[&str]) -> Option<String>) -> Vec<Fact> {
    let sysctl = |name: &str| or_unknown(run(&["sysctl", "-n", name]));
    vec![
        ("os", "macos".into()),
        ("kernel_release", sysctl("kern.osrelease")),
        ("cpu", sysctl("machdep.cpu.brand_string")),
        ("page_size", sysctl("hw.pagesize")),
        ("cpus", sysctl("hw.ncpu")),
        ("hv_vmm_present", sysctl("kern.hv_vmm_present")),
    ]
}

/// The options among rustc's arguments that decide the target features,
/// each as one argument: `-Ctarget-cpu=`, `-Ctarget-feature=`, `--target=`.
fn codegen(args: &[String]) -> Vec<String> {
    let mut out = Vec::new();
    for (i, a) in args.iter().enumerate() {
        let next = || args.get(i + 1).map(String::as_str);
        let option = match a.as_str() {
            "-C" | "--codegen" => next(),
            "--target" => {
                out.extend(next().map(|t| format!("--target={t}")));
                None
            }
            _ if a.starts_with("--target=") => {
                out.push(a.clone());
                None
            }
            _ => a
                .strip_prefix("-C")
                .or_else(|| a.strip_prefix("--codegen=")),
        };
        if let Some(o) =
            option.filter(|o| o.starts_with("target-cpu=") || o.starts_with("target-feature="))
        {
            out.push(format!("-C{o}"));
        }
    }
    out
}

fn target_with(command: &[String], rustc: &dyn Fn(&[String]) -> Option<String>) -> Vec<Fact> {
    let (program, args) = command
        .split_first()
        .map_or((None, &[][..]), |(p, a)| (Some(p), a));
    let flags = codegen(args);
    let ask = |what: &[&str], with: &[String]| {
        let mut all = vec![program?.clone()];
        all.extend(what.iter().map(|w| w.to_string()));
        all.extend(with.iter().cloned());
        rustc(&all)
    };
    let verbose = ask(&["-vV"], &[]);
    let target: Vec<String> = flags
        .iter()
        .filter(|f| f.starts_with("--target="))
        .cloned()
        .collect();
    // The last `-C target-cpu=` wins, as in rustc.
    let named = flags
        .iter()
        .filter_map(|f| f.strip_prefix("-Ctarget-cpu="))
        .next_back();
    let triple = match target.last() {
        Some(t) => Some(t["--target=".len()..].to_string()),
        None => verbose.as_ref().and_then(|v| {
            v.lines()
                .find_map(|l| l.strip_prefix("host: ").map(String::from))
        }),
    };
    let cpu = match (named, triple) {
        (Some(cpu), _) => cpu.to_string(),
        (None, Some(triple)) => {
            let list = ask(&["--print", "target-cpus"], &target).unwrap_or_default();
            let default = list
                .lines()
                .find(|l| l.contains("This is the default target CPU"))
                .and_then(|l| l.split_whitespace().next());
            format!("default of {triple}: {}", default.unwrap_or("unknown"))
        }
        (None, None) => "unknown".into(),
    };
    let features: Option<Vec<String>> = ask(&["--print", "cfg"], &flags).map(|cfg| {
        cfg.lines()
            .filter_map(|l| l.strip_prefix("target_feature="))
            .map(|f| f.trim_matches('"').to_string())
            .collect()
    });
    vec![
        ("target_cpu", cpu),
        (
            "target_features",
            or_unknown(features.as_ref().map(|f| f.len().to_string())),
        ),
        (
            "target_feature_list",
            or_unknown(features.map(|f| f.join(","))),
        ),
        (
            "rustc",
            or_unknown(verbose.and_then(|v| v.lines().next().map(String::from))),
        ),
        ("code_options", code_options(args).join(" ")),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn value<'a>(facts: &'a [Fact], name: &str) -> &'a str {
        &facts
            .iter()
            .find(|(n, _)| *n == name)
            .unwrap_or_else(|| panic!("no {name} in {facts:?}"))
            .1
    }

    fn strings(flags: &[&str]) -> Vec<String> {
        flags.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn a_fact_nothing_answers_for_is_unknown() {
        let linux = linux(&|_| None, &|_| None);
        assert_eq!(value(&linux, "os"), "linux");
        for (name, v) in &linux[1..] {
            if *name != "dockerenv" {
                assert_eq!(v, "unknown", "{name}");
            }
        }
        let macos = macos(&|_| None);
        assert_eq!(value(&macos, "os"), "macos");
        assert!(macos[1..].iter().all(|(_, v)| v == "unknown"), "{macos:?}");
        let target = target_with(&[], &|_| None);
        assert_eq!(value(&target, "target_cpu"), "unknown");
        assert_eq!(value(&target, "target_features"), "unknown");
        assert_eq!(value(&target, "target_feature_list"), "unknown");
    }

    #[test]
    fn the_cpu_is_read_from_cpuinfo_on_arm_and_on_x86() {
        let arm = "processor\t: 0\nBogoMIPS\t: 48.00\nCPU implementer\t: 0x61\nCPU architecture: 8\nCPU variant\t: 0x0\nCPU part\t: 0x000\n\nprocessor\t: 1\nCPU implementer\t: 0x61\nCPU part\t: 0x000\n";
        let facts = linux(
            &|p| (p == "/proc/cpuinfo").then(|| arm.to_string()),
            &|_| None,
        );
        assert_eq!(value(&facts, "cpu"), "implementer 0x61, part 0x000");
        let x86 = "processor\t: 0\nvendor_id\t: GenuineIntel\nmodel name\t: Intel(R) Xeon(R) Platinum 8370C CPU @ 2.80GHz\n";
        let facts = linux(
            &|p| (p == "/proc/cpuinfo").then(|| x86.to_string()),
            &|_| None,
        );
        assert_eq!(
            value(&facts, "cpu"),
            "Intel(R) Xeon(R) Platinum 8370C CPU @ 2.80GHz"
        );
    }

    #[test]
    fn the_container_marker_is_reported_as_present_or_absent_not_as_a_verdict() {
        let with = linux(&|p| (p == "/.dockerenv").then(String::new), &|_| None);
        assert_eq!(value(&with, "dockerenv"), "present");
        let without = linux(&|_| None, &|_| None);
        assert_eq!(value(&without, "dockerenv"), "absent");
    }

    /// On the CI image this records the kernel release and the CPU part.
    #[test]
    fn this_machine_s_kernel_release_cpu_and_page_size_are_recorded() {
        let facts = machine();
        for name in ["kernel_release", "cpu", "page_size", "cpus"] {
            assert_ne!(value(&facts, name), "unknown", "{name}: {facts:?}");
        }
    }

    #[test]
    fn the_flags_are_those_on_cargos_command_line_for_the_crate() {
        let verbose = concat!(
            "   Compiling target-lexicon v0.13.3\n",
            "     Running `/opt/rust/bin/rustc --crate-name target_lexicon --edition=2018 src/lib.rs -C opt-level=3 -C target-cpu=generic`\n",
            "       Fresh once_cell v1.21.3\n",
            "   Compiling einsum-greedy v0.1.0 (/work/candidate/py)\n",
            "     Running `/opt/rust/bin/rustc --crate-name einsum_greedy --edition=2024 src/lib.rs --crate-type cdylib --cfg 'feature=\"default\"' -C opt-level=3 -C target-cpu=apple-m1 -Ctarget-feature=-fp16 --target aarch64-unknown-linux-gnu -C link-arg=-s`\n",
        );
        let args = rustc_args(verbose, "einsum_greedy").unwrap();
        assert_eq!(
            args[..3],
            strings(&["/opt/rust/bin/rustc", "--crate-name", "einsum_greedy"])
        );
        assert_eq!(args.last().unwrap(), "link-arg=-s");
        assert!(
            args.contains(&"target-cpu=apple-m1".to_string()),
            "{args:?}"
        );
        assert_eq!(rustc_args(verbose, "once_cell"), None);
        assert_eq!(rustc_args(verbose, "einsum"), None);
        // A RUSTC_WRAPPER comes before the compiler; the compiler is kept.
        let wrapped = "     Running `/usr/bin/sccache /opt/rust/bin/rustc --crate-name einsum_greedy src/lib.rs`\n";
        let args = rustc_args(wrapped, "einsum_greedy").unwrap();
        assert_eq!(args[0], "/opt/rust/bin/rustc");
    }

    /// RUST-191: the commands Cargo ran rustc with for the einsum port's C2
    /// build (`-C target-cpu=apple-m1 -C llvm-args=…` in RUSTFLAGS) on macOS
    /// and in the Linux arm64 container, abridged: the `--extern`, `-L` and
    /// `--out-dir` arguments are left out and the toolchains' paths
    /// shortened. They differ only in the hash and in maturin's macOS link
    /// arguments, which change no code: the options kept are the same.
    #[test]
    fn the_options_that_change_the_code_are_kept_and_hashes_paths_and_links_left_out() {
        let mac = "/rustup/toolchains/stable-aarch64-apple-darwin/bin/rustc --crate-name greedy_rs --edition=2024 py/src/lib.rs --error-format=json --json=diagnostic-rendered-ansi,artifacts,future-incompat,unused-externs-silent --crate-type cdylib --emit=dep-info,link -C opt-level=3 -C embed-bitcode=no --check-cfg 'cfg(docsrs,test)' --check-cfg 'cfg(feature, values())' -C metadata=a21db9b0d2a3736a -C strip=debuginfo -C 'link-args=-Wl,-install_name,@rpath/greedy_rs.cpython-312-darwin.so' -C target-cpu=apple-m1 -C llvm-args=-force-vector-interleave=2 -C link-arg=-undefined -C link-arg=dynamic_lookup --force-warn=unused_crate_dependencies";
        let linux = "/usr/local/rustup/toolchains/1.100.0-aarch64-unknown-linux-gnu/bin/rustc --crate-name greedy_rs --edition=2024 py/src/lib.rs --error-format=json --json=diagnostic-rendered-ansi,artifacts,future-incompat,unused-externs-silent --crate-type cdylib --emit=dep-info,link -C opt-level=3 -C embed-bitcode=no --check-cfg 'cfg(docsrs,test)' --check-cfg 'cfg(feature, values())' -C metadata=22834903f9574a26 -C strip=debuginfo -C target-cpu=apple-m1 -C llvm-args=-force-vector-interleave=2 --force-warn=unused_crate_dependencies";
        let options = |line: &str| {
            let args = rustc_args(&format!("     Running `{line}`\n"), "greedy_rs").unwrap();
            code_options(&args)
        };
        let c2 = strings(&[
            "-C opt-level=3",
            "-C embed-bitcode=no",
            "-C strip=debuginfo",
            "-C target-cpu=apple-m1",
            "-C llvm-args=-force-vector-interleave=2",
        ]);
        assert_eq!(options(mac), c2);
        assert_eq!(options(linux), c2);
        // Every spelling of -C; the features, unquoted; the incremental
        // directory, the file-name hash and the linker left out.
        let other = "rustc --crate-name greedy_rs --cfg 'feature=\"default\"' --cfg docsrs -Copt-level=1 --codegen no-vectorize-loops --codegen=codegen-units=1 -C extra-filename=-5d0c -C incremental=/tmp/i -C linker=cc";
        assert_eq!(
            options(other),
            strings(&[
                "--cfg feature=\"default\"",
                "-C opt-level=1",
                "-C no-vectorize-loops",
                "-C codegen-units=1",
            ])
        );
    }

    /// rustup picks a toolchain by directory, so the `rustc` on PATH need
    /// not be the one that compiled the crate.
    #[test]
    fn the_facts_come_from_the_rustc_that_compiled_the_crate_and_name_its_version() {
        let asked = std::cell::RefCell::new(Vec::new());
        let facts = target_with(
            &strings(&[
                "/toolchain/bin/rustc",
                "--crate-name",
                "probe",
                "-C",
                "target-cpu=generic",
            ]),
            &|command: &[String]| {
                asked.borrow_mut().push(command.to_vec());
                match command[1].as_str() {
                    "-vV" => Some("rustc 1.99.0 (0123abcd 2026-01-01)\nhost: aarch64-apple-darwin\nrelease: 1.99.0".into()),
                    _ => Some("target_feature=\"neon\"".into()),
                }
            },
        );
        let asked = asked.into_inner();
        assert!(!asked.is_empty());
        assert!(
            asked.iter().all(|c| c[0] == "/toolchain/bin/rustc"),
            "{asked:?}"
        );
        assert_eq!(value(&facts, "rustc"), "rustc 1.99.0 (0123abcd 2026-01-01)");
        assert_eq!(value(&facts, "target_feature_list"), "neon");
    }

    /// A real build of an empty crate, read as `speed build` reads the
    /// port's.
    #[test]
    fn a_target_cpu_set_in_a_cargo_configuration_file_is_found_and_rustflags_overrides_it() {
        let dir =
            std::env::temp_dir().join(format!("carcinify-speed-config-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join(".cargo")).unwrap();
        let manifest = "[package]\nname = \"probe\"\nversion = \"0.1.0\"\nedition = \"2024\"\n\n[lib]\npath = \"lib.rs\"\n";
        std::fs::write(dir.join("Cargo.toml"), manifest).unwrap();
        std::fs::write(dir.join("lib.rs"), "").unwrap();
        let config = "[build]\nrustflags = [\"-C\", \"target-cpu=generic\"]\n";
        std::fs::write(dir.join(".cargo/config.toml"), config).unwrap();
        let cpu = |rustflags: Option<&str>| {
            let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
            let mut build = std::process::Command::new(cargo);
            build
                .args(["build", "-v", "--offline"])
                .current_dir(&dir)
                .env("CARGO_TARGET_DIR", dir.join("target"));
            for name in [
                "RUSTFLAGS",
                "CARGO_ENCODED_RUSTFLAGS",
                "CARGO_BUILD_RUSTFLAGS",
            ] {
                build.env_remove(name);
            }
            if let Some(flags) = rustflags {
                build.env("RUSTFLAGS", flags);
            }
            let out = build.output().unwrap();
            let log = String::from_utf8_lossy(&out.stderr).into_owned();
            assert!(out.status.success(), "{log}");
            let facts = target(&rustc_args(&log, "probe").expect(&log));
            assert!(value(&facts, "rustc").starts_with("rustc "), "{facts:?}");
            value(&facts, "target_cpu").to_string()
        };
        assert_eq!(cpu(None), "generic");
        assert_eq!(cpu(Some("-C target-cpu=native")), "native");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// Against the `rustc` in use. On aarch64 the counts are known: the
    /// generic CPU enables neon alone, the M1 28 features.
    #[test]
    fn the_target_cpu_follows_the_flags_and_else_the_targets_default() {
        let generic = target(&strings(&["rustc", "-C", "target-cpu=generic"]));
        assert_eq!(value(&generic, "target_cpu"), "generic");
        let last = target(&strings(&[
            "rustc",
            "-Ctarget-cpu=native",
            "-Ctarget-cpu=generic",
        ]));
        assert_eq!(value(&last, "target_cpu"), "generic");
        if cfg!(target_arch = "aarch64") {
            assert_eq!(value(&generic, "target_features"), "1");
            assert_eq!(value(&generic, "target_feature_list"), "neon");
            let m1 = target(&strings(&["rustc", "-C", "target-cpu=apple-m1"]));
            assert_eq!(
                (value(&m1, "target_cpu"), value(&m1, "target_features")),
                ("apple-m1", "28")
            );
        }
        let default = target(&strings(&["rustc"]));
        if cfg!(all(target_os = "linux", target_arch = "aarch64")) {
            assert_eq!(
                (
                    value(&default, "target_cpu"),
                    value(&default, "target_features")
                ),
                ("default of aarch64-unknown-linux-gnu: generic", "1")
            );
        }
        if cfg!(all(target_os = "linux", target_arch = "x86_64")) {
            assert_eq!(
                value(&default, "target_cpu"),
                "default of x86_64-unknown-linux-gnu: x86-64"
            );
        }
        // rustc marks no default CPU for aarch64-apple-darwin; its features
        // are the M1's.
        if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
            assert_eq!(
                (
                    value(&default, "target_cpu"),
                    value(&default, "target_features")
                ),
                ("default of aarch64-apple-darwin: unknown", "28")
            );
        }
        let linux = target(&strings(&[
            "rustc",
            "--target",
            "aarch64-unknown-linux-gnu",
        ]));
        assert_eq!(
            value(&linux, "target_cpu"),
            "default of aarch64-unknown-linux-gnu: generic"
        );
    }
}
