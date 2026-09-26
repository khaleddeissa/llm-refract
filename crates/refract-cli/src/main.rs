mod external;
use anyhow::Result;
use clap::{Parser, Subcommand};
use refract_core::Run;
use std::{fs, io::Write, path::PathBuf};
#[derive(Parser)]
#[command(name = "refract", version, about = "Portable AI executions")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}
#[derive(Subcommand)]
enum Command {
    Serve,
    Inspect {
        file: PathBuf,
    },
    Validate {
        file: PathBuf,
    },
    Replay {
        file: PathBuf,
    },
    Pack {
        file: PathBuf,
        #[arg(short, long)]
        output: PathBuf,
    },
    Unpack {
        file: PathBuf,
        #[arg(short, long)]
        output: PathBuf,
    },
    Fork {
        file: PathBuf,
        #[arg(long)]
        from: String,
        #[arg(short, long)]
        output: PathBuf,
    },
    Diff {
        left: PathBuf,
        right: PathBuf,
        #[arg(long)]
        semantic: bool,
        #[arg(long, default_value_t = 0.75)]
        threshold: f64,
        #[arg(long)]
        max_cost_increase_percent: Option<f64>,
        #[arg(long)]
        max_latency_increase_percent: Option<f64>,
        #[arg(long)]
        max_token_increase_percent: Option<f64>,
        #[arg(long)]
        grader_command: Option<PathBuf>,
        #[arg(long = "grader-arg", allow_hyphen_values = true)]
        grader_args: Vec<String>,
    },
    Metrics {
        file: PathBuf,
    },
    Eval {
        /// Dataset directory containing dataset.json, or an explicit manifest path.
        dataset: PathBuf,
        #[arg(long)]
        report: Option<PathBuf>,
        #[arg(long)]
        grader_command: Option<PathBuf>,
        #[arg(long = "grader-arg", allow_hyphen_values = true)]
        grader_args: Vec<String>,
    },
    Rerun {
        file: PathBuf,
        #[arg(long)]
        from: String,
        #[arg(long)]
        executor: PathBuf,
        #[arg(long = "executor-arg", allow_hyphen_values = true)]
        executor_args: Vec<String>,
        #[arg(long)]
        model: Option<String>,
        #[arg(long = "replace-model")]
        replace_models: Vec<String>,
        #[arg(long = "approve")]
        approved_events: Vec<String>,
        #[arg(long)]
        allow_live: bool,
        #[arg(short, long)]
        output: PathBuf,
    },
    Doctor,
}
fn read(path: &PathBuf) -> Result<Run> {
    anyhow::ensure!(
        fs::metadata(path)?.len() <= 17 * 1024 * 1024,
        "input exceeds size limit"
    );
    let bytes = fs::read(path)?;
    let run = if path.extension().is_some_and(|x| x == "rfr") {
        refract_artifact::unpack(&bytes)?
    } else {
        serde_json::from_slice(&bytes)?
    };
    run.validate()?;
    Ok(run)
}
fn write(path: PathBuf, bytes: &[u8]) -> Result<()> {
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)?;
    file.write_all(bytes)?;
    Ok(())
}
#[tokio::main]
async fn main() -> Result<()> {
    match Cli::parse().command {
        Command::Serve => refract_server::serve().await?,
        Command::Inspect { file } => println!("{}", serde_json::to_string_pretty(&read(&file)?)?),
        Command::Validate { file } => {
            read(&file)?;
            println!("valid refract.execution.v1");
        }
        Command::Replay { file } => println!(
            "{}",
            serde_json::to_string_pretty(&refract_replay::exact(&read(&file)?)?)?
        ),
        Command::Pack { file, output } => write(output, &refract_artifact::pack(&read(&file)?)?)?,
        Command::Unpack { file, output } => {
            write(output, &serde_json::to_vec_pretty(&read(&file)?)?)?
        }
        Command::Fork { file, from, output } => write(
            output,
            &refract_artifact::pack(&refract_replay::fork(&read(&file)?, &from)?)?,
        )?,
        Command::Diff {
            left,
            right,
            semantic,
            threshold,
            max_cost_increase_percent,
            max_latency_increase_percent,
            max_token_increase_percent,
            grader_command,
            grader_args,
        } => {
            let left = read(&left)?;
            let right = read(&right)?;
            if semantic
                || grader_command.is_some()
                || max_cost_increase_percent.is_some()
                || max_latency_increase_percent.is_some()
                || max_token_increase_percent.is_some()
            {
                let options = refract_diff::SemanticOptions {
                    similarity_threshold: threshold,
                    max_cost_increase_percent,
                    max_latency_increase_percent,
                    max_token_increase_percent,
                };
                options.validate().map_err(anyhow::Error::msg)?;
                let report = if let Some(executable) = grader_command {
                    refract_diff::compare_with_grader(
                        &left,
                        &right,
                        &options,
                        &external::JsonCommand {
                            executable,
                            args: grader_args,
                            timeout: std::time::Duration::from_secs(60),
                        },
                    )
                } else {
                    refract_diff::compare_semantic(&left, &right, &options)
                };
                println!("{}", serde_json::to_string_pretty(&report)?);
                if !report.passed {
                    std::process::exit(1)
                }
            } else {
                let diff = refract_diff::compare(&left, &right);
                println!("{}", serde_json::to_string_pretty(&diff)?);
                if !diff.is_empty() {
                    std::process::exit(1);
                }
            }
        }
        Command::Metrics { file } => println!(
            "{}",
            serde_json::to_string_pretty(&refract_core::metrics(&read(&file)?))?
        ),
        Command::Eval {
            dataset,
            report,
            grader_command,
            grader_args,
        } => {
            let manifest = if dataset.is_dir() {
                dataset.join("dataset.json")
            } else {
                dataset
            };
            let data: refract_eval::Dataset = serde_json::from_slice(&fs::read(&manifest)?)?;
            let root = manifest.parent().unwrap_or(std::path::Path::new("."));
            let results = if let Some(executable) = grader_command {
                refract_eval::evaluate_with_grader(
                    root,
                    &data,
                    &external::JsonCommand {
                        executable,
                        args: grader_args,
                        timeout: std::time::Duration::from_secs(60),
                    },
                )?
            } else {
                refract_eval::evaluate(root, &data)?
            };
            let bytes = serde_json::to_vec_pretty(&results)?;
            if let Some(path) = report {
                write(path, &bytes)?;
            }
            println!("{}", String::from_utf8(bytes)?);
            if !results.passed {
                std::process::exit(1)
            }
        }
        Command::Rerun {
            file,
            from,
            executor,
            executor_args,
            model,
            replace_models,
            approved_events,
            allow_live,
            output,
        } => {
            anyhow::ensure!(
                !output.exists(),
                "output already exists; refusing to execute"
            );
            let mut replacements = std::collections::BTreeMap::new();
            for value in replace_models {
                let (old, new) = value
                    .split_once('=')
                    .ok_or_else(|| anyhow::anyhow!("model replacement must be old=new"))?;
                anyhow::ensure!(
                    !old.is_empty() && !new.is_empty(),
                    "model names cannot be empty"
                );
                replacements.insert(old.to_owned(), new.to_owned());
            }
            let options = refract_replay::RerunOptions {
                from_event: from,
                model,
                replace_models: replacements,
                approved_events: approved_events.into_iter().collect(),
                allow_live,
            };
            let branch = refract_replay::rerun(
                &read(&file)?,
                &options,
                &external::JsonCommand {
                    executable: executor,
                    args: executor_args,
                    timeout: std::time::Duration::from_secs(60),
                },
            )?;
            write(output, &refract_artifact::pack(&branch)?)?;
        }
        Command::Doctor => println!(
            "refract {}\nspec: {}\nreplay: recorded; explicit executor rerun\nstorage: SQLite/PostgreSQL\nsemantic diff: offline or command grader\nevaluation: dataset manifests",
            env!("CARGO_PKG_VERSION"),
            refract_core::SPEC_VERSION
        ),
    }
    Ok(())
}
