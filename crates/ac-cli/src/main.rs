use ac_core::{Presentation, data::load_training, replay_certificate};
use ac_search::{SearchLimits, solve_exact_state_bfs};
use clap::{Parser, Subcommand};
use std::{fs, path::PathBuf, process::Command};
#[derive(Parser)]
#[command(name = "ac", about = "Local ordinary Andrews–Curtis tools")]
struct Cli {
    #[command(subcommand)]
    command: CommandKind,
}
#[derive(Subcommand)]
enum CommandKind {
    Replay {
        #[arg(long)]
        start: PathBuf,
        #[arg(long)]
        certificate: PathBuf,
    },
    Verify {
        #[arg(long)]
        manifest: PathBuf,
        #[arg(long)]
        submission: PathBuf,
    },
    WriteSubmission {
        #[arg(long)]
        challenge_id: String,
        #[arg(long)]
        certificate: PathBuf,
        #[arg(long)]
        output: PathBuf,
    },
    Solve {
        #[arg(long)]
        challenge_id: String,
        #[arg(long)]
        training: PathBuf,
    },
}
fn main() -> Result<(), String> {
    match Cli::parse().command {
        CommandKind::Replay { start, certificate } => {
            let p: Presentation =
                serde_json::from_str(&fs::read_to_string(start).map_err(|e| e.to_string())?)
                    .map_err(|e| e.to_string())?;
            let cert: Vec<u8> =
                serde_json::from_str(&fs::read_to_string(certificate).map_err(|e| e.to_string())?)
                    .map_err(|e| e.to_string())?;
            let end = replay_certificate(&p, &cert).map_err(|e| e.to_string())?;
            println!(
                "{}",
                serde_json::to_string_pretty(&end).map_err(|e| e.to_string())?
            );
        }
        CommandKind::Verify {
            manifest,
            submission,
        } => run_verifier(&manifest, &submission)?,
        CommandKind::WriteSubmission {
            challenge_id,
            certificate,
            output,
        } => {
            let cert: Vec<u8> =
                serde_json::from_str(&fs::read_to_string(certificate).map_err(|e| e.to_string())?)
                    .map_err(|e| e.to_string())?;
            fs::write(
                output,
                format!(
                    "{challenge_id}: {}\n",
                    serde_json::to_string(&cert).map_err(|e| e.to_string())?
                ),
            )
            .map_err(|e| e.to_string())?;
        }
        CommandKind::Solve {
            challenge_id,
            training,
        } => {
            let item = load_training(training)?
                .into_iter()
                .find(|x| x.training_id == challenge_id)
                .ok_or_else(|| format!("unknown training id: {challenge_id}"))?;
            let result = solve_exact_state_bfs(&item.presentation(), &SearchLimits::default());
            println!(
                "exact-state BFS is the next milestone; solver status: {:?}",
                result.status
            );
        }
    }
    Ok(())
}
fn run_verifier(manifest: &std::path::Path, submission: &std::path::Path) -> Result<(), String> {
    let root = std::env::var("SAIR_ACC_ROOT")
        .map_err(|_| "SAIR_ACC_ROOT must point to the official SAIR checkout".to_string())?;
    let manifest = manifest.canonicalize().map_err(|e| e.to_string())?;
    let submission = submission.canonicalize().map_err(|e| e.to_string())?;
    let output = Command::new("python3")
        .current_dir(&root)
        .env("PYTHONPATH", "competition/tools")
        .args(["-m", "verifier", "--manifest"])
        .arg(manifest)
        .args(["--submission"])
        .arg(submission)
        .arg("--pretty")
        .output()
        .map_err(|e| e.to_string())?;
    print!("{}", String::from_utf8_lossy(&output.stdout));
    eprint!("{}", String::from_utf8_lossy(&output.stderr));
    if output.status.success() {
        Ok(())
    } else {
        Err(format!("official verifier exited with {}", output.status))
    }
}
