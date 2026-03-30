mod admin;
mod cli;
mod config;
mod db;
mod helpers;
mod metrics;
mod mpp;
mod npm_resolver;
mod payout;
mod proxy;
mod rate_limit;
mod serve;
mod server;
mod sessions;
mod sponsor;
mod tip;
mod verifier;
mod webhook;

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "paygate", version, about = "Micropayment-gated API gateway")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Start the gateway proxy
    Serve {
        #[arg(short, long, default_value = "paygate.toml")]
        config: String,
    },
    /// Interactive setup wizard
    Init {
        #[arg(long)]
        force: bool,
    },
    /// Show gateway status
    Status {
        #[arg(short, long, default_value = "paygate.toml")]
        config: String,
    },
    /// Display pricing table
    Pricing {
        #[arg(short, long, default_value = "paygate.toml")]
        config: String,
        #[arg(long)]
        html: bool,
    },
    /// Revenue summary
    Revenue {
        #[arg(short, long, default_value = "paygate.toml")]
        config: String,
    },
    /// Show provider wallet balance
    Wallet {
        #[arg(short, long, default_value = "paygate.toml")]
        config: String,
    },
    /// Run demo with echo server
    Demo,
    /// End-to-end test on testnet
    Test,
    /// List active sessions
    Sessions {
        #[arg(short, long, default_value = "paygate.toml")]
        config: String,
    },
    /// Register service in on-chain PayGateRegistry
    Register {
        /// Service name
        #[arg(long)]
        name: String,

        /// Price per request in USDC (e.g., "0.001")
        #[arg(long)]
        price: String,

        /// Accepted TIP-20 token address
        #[arg(long, default_value = "0x20c0000000000000000000000000000000000000")]
        token: String,

        /// URL to pricing/metadata JSON
        #[arg(long, default_value = "")]
        metadata_url: String,

        /// PayGateRegistry contract address
        #[arg(long)]
        registry: String,

        /// Config file path
        #[arg(short, long, default_value = "paygate.toml")]
        config: String,
    },
}

#[tokio::main]
async fn main() {
    let cli = Cli::parse();

    match cli.command {
        Commands::Serve { config } => serve::cmd_serve(&config).await,
        Commands::Init { force } => cli::cmd_init(force),
        Commands::Status { config } => cli::cmd_status(&config).await,
        Commands::Pricing { config, html } => cli::cmd_pricing(&config, html),
        Commands::Revenue { config } => cli::cmd_revenue(&config),
        Commands::Wallet { config } => cli::cmd_wallet(&config).await,
        Commands::Demo => cli::cmd_test(true).await,
        Commands::Test => cli::cmd_test(false).await,
        Commands::Sessions { config } => cli::cmd_sessions(&config),
        Commands::Register { name, price, token, metadata_url, registry, config } => {
            cli::cmd_register(&name, &price, &token, &metadata_url, &registry, &config).await
        }
    }
}
