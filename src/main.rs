use clap::Parser;
use dgm::{dgm::DgmRunner, init_logging, load_env, DgmResult};
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "dgm")]
#[command(about = "Darwin Gödel Machine: Open-Ended Evolution of Self-Improving Agents")]
struct Cli {
    /// Maximum number of evolution iterations
    #[arg(long, default_value = "80")]
    max_generation: u32,

    /// Number of self-improvement attempts per DGM generation
    #[arg(long, default_value = "2")]
    selfimprove_size: u32,

    /// Number of parallel workers for self-improvement attempts
    #[arg(long, default_value = "2")]
    selfimprove_workers: u32,

    /// Method to choose self-improve attempts
    #[arg(long, default_value = "score_child_prop")]
    choose_selfimproves_method: String,

    /// Directory to continue the run from
    #[arg(long)]
    continue_from: Option<PathBuf>,

    /// Method to update the archive
    #[arg(long, default_value = "keep_all")]
    update_archive: String,

    /// Number of repeated SWE evaluations to run for each self-improve attempt
    #[arg(long, default_value = "1")]
    num_swe_evals: u32,

    /// Diagnose the self-improvement after evaluation
    #[arg(long)]
    post_improve_diagnose: bool,

    /// Run single shallow evaluation for self-improvement on swe
    #[arg(long)]
    shallow_eval: bool,

    /// Run on polyglot benchmark instead of SWE-bench
    #[arg(long)]
    polyglot: bool,

    /// Noise leeway for evaluation
    #[arg(long, default_value = "0.1")]
    eval_noise: f64,

    /// Do not run full evaluation on swe if a node is the top N highest performing
    #[arg(long)]
    no_full_eval: bool,

    /// Baseline to run
    #[arg(long)]
    run_baseline: Option<String>,
}

#[tokio::main]
async fn main() -> DgmResult<()> {
    load_env();
    init_logging()?;

    let cli = Cli::parse();

    let runner = DgmRunner::new(
        cli.max_generation,
        cli.selfimprove_size,
        cli.selfimprove_workers,
        cli.choose_selfimproves_method,
        cli.continue_from,
        cli.update_archive,
        cli.num_swe_evals,
        cli.post_improve_diagnose,
        cli.shallow_eval,
        cli.polyglot,
        cli.eval_noise,
        cli.no_full_eval,
        cli.run_baseline,
    )?;

    runner.run().await?;

    Ok(())
}
