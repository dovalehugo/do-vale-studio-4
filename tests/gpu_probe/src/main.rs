//! Experiment 0 — isolated GPU capability probe binary.

use gpu_probe::{format_report, run_probe};

fn main() {
    let report = pollster::block_on(run_probe());
    print!("{}", format_report(&report));
}
