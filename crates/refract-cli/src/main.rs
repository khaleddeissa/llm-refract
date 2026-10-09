mod external;
mod service;
use anyhow::Result;
use clap::{Parser, Subcommand};
use refract_core::Run;
use std::{
    fs,
    io::{IsTerminal, Write},
    path::PathBuf,
};

const COMMUNITY_MESSAGE: &str = concat!(
    "    /\\\n",
    "   /  \\\n",
    "  / /\\ \\   REFRACT\n",
    "  \\ \\/ /\n",
    "   \\  /\n",
    "    \\/\n\n",
    "Thanks for using Refract! If it helps you, a GitHub star or feedback would be appreciated.\n",
    "Star: https://github.com/khaleddeissa/llm-refract\n",
    "Feedback: https://github.com/khaleddeissa/llm-refract/issues",
);

#[derive(Parser)]
#[command(name = "refract", version, about = "Portable AI executions", after_help = COMMUNITY_MESSAGE)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}
#[derive(Subcommand)]
enum Command {
    Serve,
    /// Search recorded runs by meaning using a configured project embedding model.
    Search {
        query: String,
        #[arg(long)]
        profile: Option<String>,
        #[arg(long, default_value = "auto", value_parser = ["auto", "exact", "approximate"])]
        mode: String,
        #[arg(long, default_value_t = 20, value_parser = clap::value_parser!(u32).range(1..=100))]
        limit: u32,
    },
    /// List operator-approved models and the project's enabled profiles/index status.
    EmbeddingModels,
    /// List configured generation models and domain grading profiles.
    GenerationModels,
    /// Execute model steps through a server profile into a stored branch.
    RerunModels {
        run_id: String,
        #[arg(long)]
        profile: String,
        #[arg(long)]
        from: String,
        #[arg(long)]
        allow_live: bool,
        #[arg(long = "reuse-recorded")]
        reuse_recorded: Vec<String>,
        #[arg(long = "approve")]
        approved_events: Vec<String>,
        #[arg(short, long)]
        output: Option<PathBuf>,
    },
    /// Compare stored runs using the offline heuristic or a configured model grader.
    CompareRuns {
        left: String,
        right: String,
        #[arg(long)]
        grader: Option<String>,
        #[arg(long)]
        allow_live: bool,
        #[arg(long, default_value_t = 0.75)]
        threshold: f64,
    },
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
    let cli = Cli::parse();
    if std::io::stderr().is_terminal() && std::env::var("REFRACT_NO_BANNER").as_deref() != Ok("1") {
        let _ = writeln!(std::io::stderr(), "{COMMUNITY_MESSAGE}");
    }
    match cli.command {
        Command::Serve => refract_server::serve().await?,
        Command::Search {
            query,
            profile,
            mode,
            limit,
        } => {
            let result = service::request(
                "/v1/search/text",
                Some(
                    serde_json::json!({"query":query,"profile":profile,"mode":mode,"limit":limit}),
                ),
            )
            .await?;
            println!("{}", serde_json::to_string_pretty(&result)?);
        }
        Command::EmbeddingModels => {
            let models = service::request("/v1/embedding-models", None).await?;
            let project = service::request("/v1/project/embeddings", None).await?;
            println!(
                "{}",
                serde_json::to_string_pretty(
                    &serde_json::json!({"models":models["models"],"project":project})
                )?
            );
        }
        Command::GenerationModels => println!(
            "{}",
            serde_json::to_string_pretty(&service::request("/v1/generation-models", None).await?)?
        ),
        Command::RerunModels {
            run_id,
            profile,
            from,
            allow_live,
            reuse_recorded,
            approved_events,
            output,
        } => {
            anyhow::ensure!(allow_live, "model rerun requires --allow-live");
            if let Some(path) = &output {
                anyhow::ensure!(!path.exists(), "output already exists");
            }
            let branch = service::request(&format!("{}/rerun", service::run_path(&run_id)?), Some(serde_json::json!({"profile":profile,"from_event":from,"allow_live":allow_live,"reuse_recorded":reuse_recorded,"approved_events":approved_events}))).await?;
            if let Some(path) = output {
                write(
                    path,
                    &refract_artifact::pack(&serde_json::from_value(branch.clone())?)?,
                )?;
            }
            println!("{}", serde_json::to_string_pretty(&branch)?);
        }
        Command::CompareRuns {
            left,
            right,
            grader,
            allow_live,
            threshold,
        } => {
            anyhow::ensure!(
                grader.is_none() || allow_live,
                "model grading requires --allow-live"
            );
            let result = service::request("/v1/diff", Some(serde_json::json!({"left":left,"right":right,"semantic":true,"grader":grader,"allow_live":allow_live,"options":{"similarity_threshold":threshold}}))).await?;
            println!("{}", serde_json::to_string_pretty(&result)?);
            if result["semantic_report"]["passed"] == false {
                std::process::exit(1);
            }
        }
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
