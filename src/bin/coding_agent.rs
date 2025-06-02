use clap::Parser;
use dgm::{
    agent::AgenticSystem,
    utils::git::GitManager,
    init_logging, load_env, DgmResult,
};
use std::path::PathBuf;
use tokio::fs;
use tracing::info;

#[derive(Parser)]
#[command(name = "coding_agent")]
#[command(about = "Coding Agent: Solve coding problems using LLM with tools")]
struct Cli {
    /// The problem statement to process
    #[arg(long, required = true)]
    problem_statement: String,

    /// Path to git repository directory
    #[arg(long, required = true)]
    git_dir: PathBuf,

    /// Base commit hash to compare against
    #[arg(long, required = true)]
    base_commit: String,

    /// Path to chat history file
    #[arg(long, required = true)]
    chat_history_file: PathBuf,

    /// Output directory
    #[arg(long, default_value = "/dgm/")]
    outdir: PathBuf,

    /// Description of how to test the repository
    #[arg(long)]
    test_description: Option<String>,

    /// Whether to self-improve the repository or solving swe
    #[arg(long)]
    self_improve: bool,

    /// Instance ID for SWE issue
    #[arg(long)]
    instance_id: Option<String>,

    /// Model to use for the agent
    #[arg(long, default_value = "bedrock/us.anthropic.claude-3-5-sonnet-20241022-v2:0")]
    model: String,
}

#[tokio::main]
async fn main() -> DgmResult<()> {
    load_env();
    init_logging()?;

    let cli = Cli::parse();

    info!("Starting coding agent for problem: {}", cli.problem_statement);
    info!("Git directory: {}", cli.git_dir.display());
    info!("Base commit: {}", cli.base_commit);

    // Create the agentic system
    let agentic_system = AgenticSystem::new(
        cli.problem_statement,
        cli.git_dir.clone(),
        cli.base_commit.clone(),
        cli.chat_history_file,
        cli.test_description,
        cli.self_improve,
        cli.instance_id,
        &cli.model,
    ).await?;

    // Run the agentic system to try to solve the problem
    agentic_system.forward().await?;

    // Get code diff and save to model_patch.diff
    let git_manager = GitManager::new(&cli.git_dir)?;
    let model_patch = git_manager.diff_versus_commit(&cli.base_commit)?;
    
    let model_patch_outfile = cli.outdir.join("model_patch.diff");
    
    // Ensure output directory exists
    if let Some(parent) = model_patch_outfile.parent() {
        fs::create_dir_all(parent).await?;
    }
    
    fs::write(&model_patch_outfile, &model_patch).await?;
    
    info!("Model patch saved to: {}", model_patch_outfile.display());
    info!("Coding agent completed successfully");

    Ok(())
}
