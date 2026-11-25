//! tacmon: a terminal activity monitor for Tacit services. This file is the composition root: it reads the
//! configuration, builds the monitors with their adapters, and hands them to the shell.

#![forbid(unsafe_code)]

mod config;
mod fmt;
mod monitors;
mod shell;

use anyhow::{Result, anyhow};
use clap::Parser;
use config::Config;
use shell::{Shell, theme::Theme};
use std::{
    path::{Path, PathBuf},
    process::ExitCode,
    time::Duration,
};

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
    /// Skip the animated start screen
    #[arg(long)]
    no_splash: bool,
    /// Draw one page once to stdout as plain text and exit: PAGE, MONITOR/PAGE (fast-bridge/demand) or "splash"
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
async fn main() -> Result<ExitCode> {
    let args = Args::parse();
    let (cfg, read_from) = Config::load(args.config.as_deref())?;
    if args.print_config {
        print!("{}", toml::to_string_pretty(&cfg)?);
        return Ok(ExitCode::SUCCESS);
    }
    let monitors = monitors::create_monitors(&cfg, args.demo);
    if monitors.is_empty() {
        // not an error to trace: the first run of anyone who has not written a configuration yet ends here
        eprint!("{}", setup_help(read_from.as_deref(), Config::default_path().as_deref()));
        return Ok(ExitCode::from(2));
    }
    let mut shell = Shell::new(monitors, Theme::from_config(&cfg.theme))?;
    run(&mut shell, &args, cfg.splash).await?;
    Ok(ExitCode::SUCCESS)
}

async fn run(shell: &mut Shell, args: &Args, splash: bool) -> Result<()> {
    match &args.snapshot {
        Some(target) => {
            let (w, h) = args
                .size
                .split_once('x')
                .and_then(|(w, h)| Some((w.parse().ok()?, h.parse().ok()?)))
                .ok_or_else(|| anyhow!("--size is COLSxROWS, like 140x40"))?;
            if target == "splash" {
                println!("{}", shell.splash_snapshot(w, h)?);
                return Ok(());
            }
            shell.show(target)?;
            println!("{}", shell.snapshot(Duration::from_secs(args.wait), w, h).await?);
            Ok(())
        }
        None => shell.run(splash && !args.no_splash).await,
    }
}

/// The widest line of `setup_help`, so it reads whole in a terminal of 80 columns.
const HELP_WIDTH: usize = 78;

/// `text` as lines of at most `HELP_WIDTH`, broken at spaces. A word longer than that stays whole.
fn wrapped(text: &str) -> String {
    let mut out = String::new();
    let mut len = 0;
    for word in text.split_whitespace() {
        let n = word.chars().count();
        if len > 0 && len + 1 + n > HELP_WIDTH {
            out.push('\n');
            len = 0;
        } else if len > 0 {
            out.push(' ');
            len += 1;
        }
        out.push_str(word);
        len += n;
    }
    out
}

/// What to say when no monitor is on: which file was read or where one goes, how to make it, and the line that
/// turns each monitor on. `read_from` is the file the configuration came from; `default` is where one is looked for.
/// Prose is wrapped to `HELP_WIDTH`; a path or a command is a line of its own, never broken, so it can be copied.
fn setup_help(read_from: Option<&Path>, default: Option<&Path>) -> String {
    let lines = |ls: &[String]| ls.iter().map(|l| format!("    {l}")).collect::<Vec<_>>().join("\n");
    let mut parts =
        vec!["tacmon: no monitor is enabled.".to_string(), wrapped("Every monitor is off until you turn it on: each one needs a service of yours to watch.")];
    match (read_from, default) {
        (Some(file), _) => {
            parts.push("Your configuration has no monitor turned on:".into());
            parts.push(lines(&[file.display().to_string()]));
            parts.push("Add the lines for what you run:".into());
        }
        (None, Some(file)) => {
            parts.push(wrapped("There is no configuration yet. It goes here, or anywhere with --config FILE:"));
            parts.push(lines(&[file.display().to_string()]));
            parts.push("To write one with every setting at its default:".into());
            parts.push(lines(&[format!("mkdir -p {}", file.parent().unwrap_or(file).display()), format!("tacmon --print-config > {}", file.display())]));
            parts.push("Then turn on what you run:".into());
        }
        (None, None) => {
            parts.push(wrapped(
                "There is no configuration yet, and no home directory to keep one in. Write one with every setting at its default and name it with --config:",
            ));
            parts.push(lines(&["tacmon --print-config > tacmon.toml".into(), "tacmon --config tacmon.toml".into()]));
            parts.push("First turn on what you run:".into());
        }
    }
    for (id, section, watches) in monitors::KNOWN {
        parts.push(lines(&[format!("# {id}: {watches}"), format!("[{section}]"), "enabled = true".into()]));
    }
    parts.push(wrapped("The same file, with every setting, is config.example.toml in the repository, and docs/ there says what each setting does:"));
    parts.push(lines(&["https://github.com/dnznlabs/tacmon".into()]));
    parts.push(wrapped("tacmon --demo runs every monitor on made-up data and needs no configuration."));
    parts.join("\n\n") + "\n"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn help_without_a_configuration_says_where_one_goes_and_how_to_write_it() {
        let default = Path::new("/home/op/.config/tacmon/config.toml");

        let out = setup_help(None, Some(default));

        assert!(out.contains("    mkdir -p /home/op/.config/tacmon\n"), "{out}");
        assert!(out.contains("tacmon --print-config > /home/op/.config/tacmon/config.toml\n"), "{out}");
        assert!(out.contains("    [fast_bridge]\n    enabled = true"), "{out}");
    }

    #[test]
    fn help_with_a_configuration_names_it_and_never_says_to_write_over_it() {
        let file = Path::new("/etc/tacmon.toml");

        let out = setup_help(Some(file), Some(Path::new("/home/op/.config/tacmon/config.toml")));

        assert!(out.contains("turned on:\n\n    /etc/tacmon.toml\n"), "{out}");
        assert!(!out.contains("--print-config >"), "{out}");
        assert!(out.contains("    [fast_bridge]\n    enabled = true"), "{out}");
    }

    #[test]
    fn help_keeps_to_its_width_and_leaves_a_long_path_whole() {
        let long = format!("/home/{}/.config/tacmon/config.toml", "o".repeat(90));
        let cases = [
            (None, None),
            (None, Some(Path::new("/home/op/.config/tacmon/config.toml"))),
            (Some(Path::new("/etc/tacmon.toml")), None),
            (None, Some(Path::new(&long))),
        ];

        let out = cases.map(|(read_from, default)| setup_help(read_from, default));

        for text in &out[..3] {
            assert!(text.lines().all(|l| l.chars().count() <= HELP_WIDTH), "{text}");
        }
        // only the lines that carry the path run over, and each carries all of it
        let over: Vec<&str> = out[3].lines().filter(|l| l.chars().count() > HELP_WIDTH).collect();
        assert!(over.len() == 3 && over.iter().all(|l| l.starts_with("    ") && l.contains(&long[..long.len() - 12])), "{}", out[3]);
    }
}
