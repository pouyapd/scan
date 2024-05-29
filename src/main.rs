use clap::{Parser as ClapParser, Subcommand};
use scan_fmt_xml::{Parser, Sc2CsVisitor};
use std::{error::Error, path::PathBuf};

/// SCAN (StoChastic ANalyzer)
/// is a statistical model checker based on channel systems
#[derive(ClapParser)]
#[command(author, version, about, long_about = None)]
struct Cli {
    /// Action to perform on model
    #[command(subcommand)]
    command: Commands,
    /// Select model XML file
    model: PathBuf,
}

#[derive(Subcommand)]
enum Commands {
    /// Verify model
    Verify {
        /// lists test values
        #[arg(short, long)]
        runs: usize,
    },
    /// Validate model XML file
    Parse,
    /// Parse and validate model XML file
    Validate,
    /// Execute model once
    Execute,
}

fn main() -> Result<(), Box<dyn Error>> {
    env_logger::init();
    let cli = Cli::parse();
    match &cli.command {
        Commands::Verify { runs: _ } => {
            println!("Verifying model - NOT YET IMPLEMENTED");

            // info!("parsing model");
            // let model = Parser::parse(cli.model.to_owned())?;
            // let cs = Sc2CsVisitor::visit(model)?;

            // for run in 0..*runs {
            //     info!("verify model, run {run}");
            //     let mut model = model.clone();
            //     while let Some((pg_id, action, post)) = model.possible_transitions().first() {
            //         model
            //             .transition(*pg_id, *action, *post)
            //             .expect("transition possible");
            //         println!("{model:#?}");
            //     }
            //     info!("model verified");
            // }
        }
        Commands::Parse => {
            println!("Parsing model");
            let model = Parser::parse(cli.model.to_owned())?;
            println!("{model:#?}");
            println!("Model successfully parsed");
        }
        Commands::Validate => {
            println!("Validating model");
            let model = Parser::parse(cli.model.to_owned())?;
            let model = Sc2CsVisitor::visit(model)?;
            println!("{model:#?}");
            println!("Model successfully validated");
        }
        Commands::Execute => {
            println!("Executing model");
            let model = Parser::parse(cli.model.to_owned())?;
            let mut model = Sc2CsVisitor::visit(model)?;
            println!("Transitions list:");
            let mut trans: u32 = 0;
            while let Some((pg_id, action, destination)) = model
                .cs
                .possible_transitions()
                .take(1)
                .collect::<Vec<_>>()
                .pop()
            {
                let pg = model
                    .fsm_names
                    .get(&pg_id)
                    .cloned()
                    .unwrap_or_else(|| format!("{pg_id:?}"));
                trans += 1;
                println!("#{trans:04}: PG {pg} by {action:?} to {destination:?}");
                model.cs.transition(pg_id, action, destination)?;
            }
            println!("Model run to termination");
        }
    }
    Ok(())
}
