mod category;
mod check;
mod cli;
mod db;
mod explain;
mod format;
mod graph;
mod hierarchy;
mod model;
mod nix;
mod ownership;
mod scan;
mod source;
mod stats;
mod time;
mod tree;

use anyhow::Result;
use clap::Parser;
use cli::{Cli, Commands};
use klocc::artifact;

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Scan { root, out } => scan::run(&root, &out),
        Commands::Stats { artifact } => stats::run(&artifact),
        Commands::Check { artifact } => check::run(&artifact),
        Commands::Explain { artifact, store_path } => explain::run(&artifact, &store_path),
        Commands::Tree {
            artifact,
            max_depth,
            source_filter,
        } => tree::run(&artifact, max_depth, &source_filter),
    }
}
