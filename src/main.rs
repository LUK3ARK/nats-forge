use clap::Parser;
use natsforge::NatsForge;

#[derive(Parser)]
#[command(about = "NATS configuration generator")]
struct Cli {
    #[arg(short, long, default_value = "config.json")]
    config: String,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    let forge = NatsForge::from_json_file(&cli.config)?;
    let result = forge.initialize().await?;
    println!("Configuration generated: {:?}", result);
    Ok(())
}
