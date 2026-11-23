//! What a target's runs give (design §2): each side's best, median and
//! worst over the runs kept, and the ratio original ÷ port within each pair
//! of runs, with its median and range.

/// Which side a run timed.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Side {
    Original,
    Port,
}

/// One timed run, as the driver gives it.
#[derive(Debug, Clone)]
pub struct Run {
    pub side: Side,
    /// The mean of the run's calls.
    pub seconds_per_call: f64,
    pub calls: usize,
    /// The larger share of its time that the loop of pure computation just
    /// before and just after the run was kept off the CPU.
    pub kept_off_the_cpu: f64,
}

/// Best, median and worst seconds per call, and the spread: worst minus
/// best, over the median.
#[derive(Debug, Clone, PartialEq)]
pub struct Spread {
    pub best: f64,
    pub median: f64,
    pub worst: f64,
    pub spread: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Figures {
    /// Over the runs of the pairs kept; `None` when every pair was dropped.
    pub original: Option<Spread>,
    pub port: Option<Spread>,
    /// Original ÷ port within each pair kept: median, and the range from
    /// the lowest to the highest. The range is how noisy, not an interval.
    pub ratio: Option<(f64, f64, f64)>,
    pub pairs: usize,
    pub pairs_dropped: usize,
    /// A run shorter than [`SHORT_RUN_SECONDS`]: loop and timer overhead
    /// start to count.
    pub short_run: bool,
}

pub const SHORT_RUN_SECONDS: f64 = 0.001;

/// The figures of runs given in the order they ran: pairs of two, one run
/// of each side.
pub fn figures(runs: &[Run]) -> Figures {
    let pairs: Vec<&[Run]> = runs.as_chunks::<2>().0.iter().map(|p| &p[..]).collect();
    // decide's threshold for a loop of pure computation kept off the CPU.
    let disturbed = |r: &Run| r.kept_off_the_cpu > carcinify_decide::report::BUSY_CALIBRATION;
    let kept: Vec<&[Run]> = pairs
        .iter()
        .copied()
        .filter(|p| !p.iter().any(disturbed))
        .collect();
    let side = |s: Side| {
        let mut times: Vec<f64> = kept
            .iter()
            .flat_map(|p| p.iter())
            .filter(|r| r.side == s)
            .map(|r| r.seconds_per_call)
            .collect();
        times.sort_by(f64::total_cmp);
        let median = median(&times)?;
        let (best, worst) = (times[0], times[times.len() - 1]);
        Some(Spread {
            best,
            median,
            worst,
            spread: (worst - best) / median,
        })
    };
    let of = |p: &[Run], s: Side| {
        p.iter()
            .find(|r| r.side == s)
            .map_or(f64::NAN, |r| r.seconds_per_call)
    };
    let mut ratios: Vec<f64> = kept
        .iter()
        .map(|p| of(p, Side::Original) / of(p, Side::Port))
        .collect();
    ratios.sort_by(f64::total_cmp);
    Figures {
        original: side(Side::Original),
        port: side(Side::Port),
        ratio: median(&ratios).map(|m| (m, ratios[0], ratios[ratios.len() - 1])),
        pairs: pairs.len(),
        pairs_dropped: pairs.len() - kept.len(),
        short_run: runs
            .iter()
            .any(|r| r.seconds_per_call * (r.calls as f64) < SHORT_RUN_SECONDS),
    }
}

/// The median of sorted values.
fn median(sorted: &[f64]) -> Option<f64> {
    let n = sorted.len();
    (n > 0).then(|| (sorted[(n - 1) / 2] + sorted[n / 2]) / 2.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use Side::{Original as O, Port as P};

    fn run(side: Side, ms: f64, kept_off_the_cpu: f64) -> Run {
        Run {
            side,
            seconds_per_call: ms / 1000.0,
            calls: 10,
            kept_off_the_cpu,
        }
    }

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-9
    }

    #[test]
    fn the_ratio_is_taken_within_each_pair_of_runs_in_abba_order() {
        let f = figures(&[
            run(O, 2.0, 0.0),
            run(P, 1.0, 0.0),
            run(P, 1.0, 0.0),
            run(O, 3.0, 0.0),
        ]);
        let (median, low, high) = f.ratio.unwrap();
        assert!(
            close(median, 2.5) && close(low, 2.0) && close(high, 3.0),
            "{f:?}"
        );
        let original = f.original.unwrap();
        assert!(
            close(original.best, 0.002)
                && close(original.median, 0.0025)
                && close(original.worst, 0.003)
        );
        assert!(close(original.spread, 0.4), "{original:?}");
        assert_eq!((f.pairs, f.pairs_dropped, f.short_run), (2, 0, false));
    }

    #[test]
    fn a_run_whose_loop_was_kept_off_the_cpu_drops_its_pair() {
        // The second pair's port run was disturbed, past decide's 10%.
        let f = figures(&[
            run(O, 2.0, 0.0),
            run(P, 1.0, 0.01),
            run(P, 9.0, 0.11),
            run(O, 3.0, 0.0),
            run(O, 4.0, 0.0),
            run(P, 1.0, 0.0),
        ]);
        let (median, low, high) = f.ratio.unwrap();
        assert!(
            close(median, 3.0) && close(low, 2.0) && close(high, 4.0),
            "{f:?}"
        );
        assert!(
            close(f.port.unwrap().worst, 0.001),
            "the dropped pair's runs are not counted"
        );
        assert!(close(f.original.unwrap().median, 0.003));
        assert_eq!((f.pairs, f.pairs_dropped), (3, 1));
    }

    #[test]
    fn every_pair_dropped_leaves_no_figure() {
        let f = figures(&[run(O, 2.0, 0.2), run(P, 1.0, 0.0)]);
        assert_eq!((f.original, f.port, f.ratio), (None, None, None));
        assert_eq!((f.pairs, f.pairs_dropped), (1, 1));
    }

    #[test]
    fn a_run_under_a_millisecond_is_flagged() {
        // 10 calls of 0.09 ms: 0.9 ms.
        assert!(figures(&[run(O, 0.09, 0.0), run(P, 1.0, 0.0)]).short_run);
        assert!(!figures(&[run(O, 0.1, 0.0), run(P, 1.0, 0.0)]).short_run);
    }
}
