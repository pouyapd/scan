mod parser;

use crate::parser::Parser;

use clap::{Parser as ClapParser, Subcommand};
use log::{debug, error, info, trace, warn};
use quick_xml::Reader;
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
        Commands::Verify { runs } => {
            info!("verifying model ({runs} runs)");
            error!("unimplemented functionality");
            todo!();
        }
        Commands::Validate => {
            println!("Validating model");

            info!("creating reader from file {0}", cli.model.display());
            let mut reader = Reader::from_file(cli.model)?;

            info!("parsing model");
            let _model = Parser::parse(&mut reader)?;

            println!("Model successfully validated");
        }
    }

    info!("SCAN terminating");
    Ok(())
}
