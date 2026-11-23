# Step 7: what is known, and how well

For someone deciding how far to trust `STEP.md` and the tool behind it.
Tests are named as they stand in `speed/tests/cli.rs` (through the real
binary, on the fixture in `speed/tests/fixture`) or in `speed/src/`. Figures
from runs outside the repository are given with their window and date; the
records of those runs are not in the repository.

## The tool's mechanics: tested on its fixture

- **Pairs, ratios, range.** The sides alternate in ABBA pairs and the
  ratio is taken within each pair (`the_ratio_is_taken_within_each_pair_of_runs_in_abba_order`);
  a fixture port ten times slower on purpose gives a ratio of about 0.1
  (`the_sides_alternate_in_pairs_and_a_port_ten_times_slower_gives_about_a_tenth`).
  The range is the lowest and the highest pair ratio, by construction,
  so it claims no interval.
- **Dropped pairs, `short_run`, the verdicts.** Unit tests in
  `src/figures.rs` and `src/run.rs`: a run whose loop was kept off the CPU
  drops its pair; every pair dropped is `unreliable`; a process beside the
  timing kept off the CPU is `unreliable`; a run under a millisecond is
  flagged; a failed target fails the run before anything else; flags
  leave `pass_with_flags`. The 10% and 15% thresholds, and the loops that
  measure them, are `carcinify decide`'s, reused; they were not chosen
  again for speed. The 1 ms for `short_run` is reasoned (loop and timer
  overhead), not measured.
- **The gate.** Sixteen unit tests in `src/gate.rs` read gate logs, one of
  them a copy of a real log; `a_run_not_under_the_gate_is_unreliable_and_keeps_its_figures`
  and `speed_build_refuses_to_run_under_the_gate` run the binary.
  `an_interrupted_gate_says_so_and_leaves_no_sampler` interrupts
  `ci/gate.sh` itself.
- **Failures.** `a_timed_call_whose_output_differs_from_the_originals_fails`,
  `a_function_carcinify_diff_would_refuse_is_not_timed`,
  `a_cached_original_is_refused_as_carcinify_diff_refuses_it`,
  `a_port_that_raises_only_on_a_new_thread_fails`,
  `a_port_that_answers_otherwise_on_a_new_thread_fails`,
  `a_port_that_changes_its_arguments_where_the_original_does_not_fails`,
  `a_target_that_runs_past_the_time_limit_fails_and_the_next_is_still_timed`,
  `a_target_whose_driver_crashes_fails_and_the_next_is_still_timed`.
- **Targets.** `a_target_must_name_a_case_of_the_suite_that_returns` and
  `a_target_names_a_case_or_a_caller` for direct targets. For callers:
  `a_caller_is_built_with_its_calls_recorded_and_found_among_the_cases`,
  `a_caller_that_reaches_the_function_with_an_input_no_case_has_is_refused`,
  `a_caller_whose_two_recordings_differ_is_refused`,
  `a_caller_refused_for_an_input_no_case_has_passes_with_the_case_added`,
  `a_caller_is_timed_with_each_sides_function_in_place`,
  `a_caller_that_reaches_other_inputs_in_the_run_than_in_the_build_fails`.
  Caller targets have been run on the fixture only.
- **The build record holds the run.** `speed_run_refuses_wheels_that_speed_build_did_not_write`
  and `speed_run_refuses_what_speed_build_did_not_build_for`.
- **`--out` inside the repository.** Refused before anything is built
  (`src/build.rs`); checked by hand on a scratch copy of the fixture on
  2026-11-24 (exit 2 inside, exit 0 outside). No test.
- **Two modules of one name.** `a_module_name_both_sides_provide_is_refused`;
  `a_module_each_side_reaches_by_its_own_path_entry_is_refused` (each side
  adds its own `lib/` to `sys.path` and imports `helper`);
  `a_sides_module_that_python_would_give_the_other_side_is_refused` (the
  original's own `colorsys.py`, the port's own `statistics.py`);
  `a_sides_module_that_shadows_a_shared_one_is_refused` (the original's
  own `helper.py` over a shared one); and
  `an_installed_copy_of_the_ports_module_that_only_the_port_imports_builds`
  (a copy of the port's module, with its `dist-info`, in a scratch venv's
  `site-packages`, with a caller target declared too). The `maturin
  develop` case itself was not run. Through a caller target:
  `a_module_a_side_imports_only_through_a_caller_is_refused` (the
  original's own `colorsys.py`, imported inside the function the caller
  reaches).

## The counts: tested on the fixture, not on a real port

- **The counting allocator** in the template is the fixture's
  (`speed/tests/fixture/rs/src/lib.rs`), which the tests that need maturin
  build and count.
- **Warm, first, workspace.** `the_ports_allocations_are_counted_warm_with_the_first_call_apart`:
  on the fixture's `fill`, the first call makes 102 allocations and keeps
  4,096 live bytes (`workspace_kept_bytes`), a warm call 101 and none
  kept. `the_workspace_is_read_with_the_result_dropped_and_its_growth_over_warm_calls`
  and a unit test in `src/run.rs` cover `result_bytes` and
  `workspace_grows`.
- **The Python figure from emptied free lists.** The same test: a tuple of
  two new ints leaves 3 blocks (112 bytes), a float 1, and a call whose
  temporaries are freed leaves only its result, 1 block. Before 70fd7e6 a
  pair left 2 blocks and a float none: their blocks came from free lists
  and were never traced. For `fill`, the pair and the float, the peak
  equals the bytes left; before the commit that reads it ahead of the
  second collection, every peak was 46 B above, that collection's own.
- **Keyword names written out, or through `partial`.** The same test, with
  the cases `options-odd-key`, `options-nfkc-key` and `options-debug-key`.
- **Refusals.** `a_timed_wheel_built_with_count_allocations_is_refused`;
  `a_python_port_is_not_counted`.
- **No real port carries the allocator yet.** The einsum port's runs
  below are all flagged `counts_not_measured`. Whether the counts read as
  intended on real code is unknown until one does.

## The cold thread: tested on the fixture

`a_cold_thread_is_reported_as_cold_minus_warm_paired_per_thread` (Python
port) and `the_cheap_call_leaves_the_pyo3_ports_workspace_to_the_first_real_call`
(PyO3), with `a_function_with_no_required_parameter_needs_speed_noop_for_its_cold_thread`.
The fixture's first call in a thread sleeps 50 ms to make its workspace.
At 5 ms the PyO3 test once failed under load: sleeps of 20 ms woke 2 to
12 ms late (bd87f2e). A real port's first-call cost has not been measured
with it.

## `--compare`: tested on hand-written reports, run on real ones

- Ten tests on hand-written report JSON: ratios 30% apart flag and 10%
  apart do not; the order of the two reports changes nothing; two
  interpreters, two commits, other measuring code, a missing target, a
  report that did not pass; three significant figures below 1, and an
  `apart` beside the threshold; `builds_differ` and
  `builds_and_platforms_differ` for two reports that differ only in
  RUSTFLAGS, and `builds_differ` for two that differ only in the options
  rustc ran with.
- **The options rustc ran with.** Recorded from Cargo's command line:
  `a_flag_a_cargo_configuration_file_gives_this_platform_is_recorded`
  (a committed `.cargo/config.toml` section for this platform), and
  `CARGO_PROFILE_RELEASE_OPT_LEVEL=1` in
  `speed_build_writes_the_ports_wheel_its_hash_and_the_target_cpu_it_was_built_for`.
  That platform defaults are left out:
  `the_options_that_change_the_code_are_kept_and_hashes_paths_and_links_left_out`,
  on the commands Cargo ran for the einsum port's C2 build on macOS and in
  the Linux arm64 container, which differ in maturin's macOS link
  arguments and the hash only. On the chase's two C2 reports, with each
  build's options added, `--compare` flags `platform_dependent` at 120 and
  270 operands and no `builds_differ` (2026-11-24).
- **1.25 is a first value, and arbitrary.** It was not derived from data.
  On the einsum port's reports (below), one set of flags on both platforms
  (the apple-m1 tuning made to vectorize from 4 words) gave 1.03 apart at
  40 operands, 3.37 at 120 and 4.95 at 270.
- On those reports, Linux's generic build with `-C no-vectorize-loops`
  against macOS's generic build printed `platform_dependent` at 0913eb5,
  with "target CPU generic" on both sides and the extra flag unseen. Since
  293f381 it prints each build's flags and `builds_differ`.

## `ci/linux/speed.sh`: checked in Docker, not in the test suites

No test runs the script. Each change was checked by running the real
script in the CI's container, as its commit message records:
`RUSTFLAGS` reaching `build.json` (3e46ea2); an interrupt in the gate and
during the build (d47484a); the script's own failures and the refusal of
`<out>` and `<repo>` holding one another (ec90cfb); the container's cap
(e4ec399), and a cap ending under `speed run` giving 2, not 137 (c97e750);
an earlier report removed at the start (f349034).

How to stop it rests on the audit of K1 to K3 (2026-11-24, the real
script, a process group of its own per run): TERM to the group during
the gated `speed run` ended it in 1 s with 143 and no report; TERM to the
script's PID alone ended it 62 s later, with 143 beside a complete
report, as bash runs a trap only when its foreground command returns.

## Real use: one port, in a scratch copy, during one investigation

The tool has timed one real port: the einsum port of the README at the top
of the repository, in a scratch clone with one scratch commit adding the
cases, with three direct targets (40, 120 and 270 operands), 15 runs, and
carcinify 0913eb5, on 2026-11-23 and 24. No caller target, and no port's
own `speed.json`, has been run on a real port yet.

The figures `STEP.md` quotes for a caller target (26 to 28 times on the
search at 40 operands on macOS, 6 to 7 times through `np.einsum_path`)
are the README's, taken before this tool with the port's own timing
script.

## The Linux aarch64 lines: one machine, one port

All of it is from one Apple M1 Pro: Linux arm64 in Docker's VM, and macOS
on the metal.

- **2.5 times, with no instruction-set feature added.** 2026-11-23, Linux
  arm64 in Docker, gated, the clean first pass from 14:00:54 to 14:01:50,
  timed with the port's own timing script (15 runs a side), not with this
  tool; 120 operands. The default build (generic) took 8.40 to 8.67 ms;
  the apple-m1 tuning with every feature but NEON removed, 3.41 ms;
  default ÷ that build, 2.47 and 2.52, best and worst apart. The generic
  tuning with all 28 of the M1's features gave 1.00.
- **Which loop.** The disassembly of the search's OR loop over the
  bitsets: the generic tuning enters a NEON loop, 4 words at a time, from
  4 words (`cmp x8, #0x4`); the apple-m1 tuning, 8 at a time, from 8
  (`cmp x8, #0x8`). At 40 operands the bitsets are 2 words, at 120 4, and
  at 270 8.
- **The vectorized loop is the slow one on Linux.** 2026-11-23, Linux
  arm64 in Docker, this tool, the two clean passes from 22:22:38 to
  22:24:29 (the later passes of that window were load-flagged), port
  medians:
  apple-m1 forced to vectorize from 4 words, 8.02 ms at 120 operands and
  166 ms at 270; generic with `-C no-vectorize-loops`, 3.20 and 51.3.
- **"Slow too", and "2.7 times as fast at 8".** 2026-11-24, Linux arm64 in
  Docker, this tool, gated 01:06:23 to 01:10:05, four clean passes (apple-m1, generic with
  `-C no-vectorize-loops`, twice each), port medians:
  - 270 operands: apple-m1 138.1 and 138.2 ms, best to worst 137.8 to
    141.8; the other 50.72 and 50.66 ms, 50.42 to 53.31: 2.72 times, the
    ranges apart;
  - 120 operands: 3.217 and 3.223 against 3.157 and 3.148: 1.02 times,
    alike.

  The predictions were written before the build.
- **On macOS the vectorized loop was not slower.** 2026-11-23, this tool,
  gated 23:15:18 to 23:18:39, clean: apple-m1 forced to vectorize from 4
  words and generic were alike at every size (2.33 ms both at 120
  operands, 33.3 to 33.4 ms at 270). Earlier that day, 13:02:33 to
  13:02:56, clean, with the port's own timing script, the vectorized
  build was the faster one at 120 operands: generic 2.27 to 2.31 ms
  against apple-m1's 3.12 to 3.19.
- **Why the vectorized loop is slow on Linux in the VM is not known.** One
  hypothesis, a 32-byte store forwarded across a 64-byte line, was ruled
  out by the buffer's addresses: on Linux, slow, it crossed no line at 4
  words; on macOS, fast, it crossed one in one process of two. Linux and the VM cannot be told apart: no bare-metal Linux
  arm64 was used.
- **Line 2** is reasoned from that: the figures are of one machine, and
  the effect turned on one loop's tuning.
- **Line 3, the rebuild**, is the same runs read the other way: at 120
  operands the apple-m1 build avoided the slow loop, at 270 it took one.
- **Tuning for other aarch64 CPUs:** no run on Neoverse, Graviton or
  Cortex-A, so the step gives no advice on it.

## Not tried

- Python before 3.12; Python built without the GIL.
- Linux x86-64, Windows, and aarch64 machines other than the one above.
- A caller target on a real port; a real port with the counting
  allocator; the `#[pymodule] fn` form of the export.
