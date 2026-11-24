//! tacmon: a terminal activity monitor for Tacit services. This file is the composition root: it reads the
//! configuration, builds the monitors with their adapters, and hands them to the shell.

mod config;
mod fmt;
mod monitors;
mod shell;

use anyhow::{Result, anyhow};
use clap::Parser;
use config::Config;
use shell::{Shell, theme::Theme};
use std::{path::PathBuf, time::Duration};

#[derive(Parser)]
#[command(name = "tacmon", version, about = "A terminal activity monitor for Tacit services")]
struct Args {
    /// Config file (default: ~/.config/tacmon/config.toml when it exists)
    #[arg(short, long)]
    config: Option<PathBuf>,
    /// Print the effective configuration as TOML and exit
    #[arg(long)]
    print_config: bool,
    /// Run on made-up data: nothing is read from this machine or the network
    #[arg(long)]
    demo: bool,
    /// Draw one page once to stdout as plain text and exit: PAGE or MONITOR/PAGE (fast-bridge/demand)
    #[arg(long, value_name = "PAGE")]
    snapshot: Option<String>,
    /// With --snapshot: seconds to gather data before drawing
    #[arg(long, default_value_t = 4)]
    wait: u64,
    /// With --snapshot: the size to draw at, as COLSxROWS
    #[arg(long, default_value = "140x40")]
    size: String,
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<()> {
    let args = Args::parse();
    let cfg = Config::load(args.config.as_deref())?;
    if args.print_config {
        print!("{}", toml::to_string_pretty(&cfg)?);
        return Ok(());
    }
    let mut shell = Shell::new(monitors::create_monitors(&cfg, args.demo), Theme::from_config(&cfg.theme))?;
    match &args.snapshot {
        Some(target) => {
            shell.show(target)?;
            let (w, h) = args
                .size
                .split_once('x')
                .and_then(|(w, h)| Some((w.parse().ok()?, h.parse().ok()?)))
                .ok_or_else(|| anyhow!("--size is COLSxROWS, like 140x40"))?;
            println!("{}", shell.snapshot(Duration::from_secs(args.wait), w, h).await?);
            Ok(())
        }
        None => shell.run().await,
    }
}
