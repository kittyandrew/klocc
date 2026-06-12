use std::path::PathBuf;

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "klocc")]
#[command(about = "Scan Nix runtime closures into a normalized SQLite artifact")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Subcommand)]
pub enum Commands {
    Scan {
        root: String,
        #[arg(long)]
        out: PathBuf,
    },
    Stats {
        artifact: PathBuf,
    },
    Check {
        artifact: PathBuf,
    },
    Explain {
        artifact: PathBuf,
        store_path: String,
    },
    Tree {
        artifact: PathBuf,
        #[arg(long, default_value_t = 12)]
        max_depth: usize,
        #[arg(long, default_value = "all", value_parser = ["all", "runtime", "build", "unique", "shared"])]
        source_filter: String,
    },
}
