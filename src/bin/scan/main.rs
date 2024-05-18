use clap::{Parser as ClapParser, Subcommand};
use log::info;
use scan::{Parser, Sc2CsVisitor};
use std::{error::Error, path::PathBuf};

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
    /// Parse and validate model XML file
    Validate,
}

fn main() -> Result<(), Box<dyn Error>> {
    env_logger::init();
    info!("SCAN starting up");

    let cli = Cli::parse();
    info!("cli arguments parsed");

    match &cli.command {
        Commands::Verify { runs: _ } => {
            println!("Validating model");

            info!("creating reader from file {0}", cli.model.display());
            // let mut reader = Reader::from_file(cli.model)?;

            info!("parsing model");
            let model = Parser::parse(cli.model.to_owned())?;
            // let model = Parser::parse(&mut reader)?;
            println!("{model:#?}");
            let cs = Sc2CsVisitor::visit(model)?;
            println!("{cs:#?}");

            println!("Model successfully validated");

            todo!();

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
        Commands::Validate => {
            println!("Validating model");

            info!("creating reader from file {0}", cli.model.display());
            // let mut reader = Reader::from_file(cli.model)?;

            info!("parsing model");
            // let model = Parser::parse(&mut reader)?;
            let model = Parser::parse(cli.model.to_owned())?;
            println!("{model:#?}");

            info!("building CS representation");
            let cs = Sc2CsVisitor::visit(model)?;
            println!("{cs:#?}");

            println!("Model successfully validated");
        }
    }

    info!("SCAN terminating");
    Ok(())
}
