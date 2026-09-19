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
        Command::Diff { left, right } => {
            let diff = refract_diff::compare(&read(&left)?, &read(&right)?);
            println!("{}", serde_json::to_string_pretty(&diff)?);
            if !diff.is_empty() {
                std::process::exit(1);
            }
        }
        Command::Doctor => println!(
            "refract {}\nspec: {}\nreplay: recorded playback only\nstorage: SQLite\nOTLP/gRPC: planned",
            env!("CARGO_PKG_VERSION"),
            refract_core::SPEC_VERSION
        ),
    }
    Ok(())
}
