use clap::Parser;
use env_logger::Env;
use scan::Cli;

fn main() -> anyhow::Result<()> {
    env_logger::Builder::from_env(Env::default().default_filter_or("off")).init();
    let cli = Cli::parse();
    cli.run()
}
