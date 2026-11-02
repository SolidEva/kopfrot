use clap::Parser;
use std::{env::var, fs, path::PathBuf, process::Command};

use anyhow::{Context, Error, Result};
use chrono::Local;

mod settings;

#[derive(Parser)]
#[command(version)]
struct Cli {
    /// path to the root of the vault, otherwise assumes we are in the vault
    #[arg(short, long)]
    config: Option<PathBuf>,
    #[clap(long, short)]
    subdir: Option<String>,
    // if no notetype or title passed in, assume we are making or opening the daily note
    notetype: Option<String>,
    title: Option<String>,
}

fn main() -> Result<(), Error> {
    let cli = Cli::try_parse().unwrap_or_else(|error| error.exit());

    let conf = settings::load_config(cli.config.as_deref())?;

    let notetype_str = match cli.notetype {
        Some(notetype_str) => notetype_str,
        None => "daily".to_string(),
    };

    let title_str = match cli.title {
        Some(title_str) => title_str,
        None => {
            if notetype_str != "daily" {
                panic!("title must be set if notetype is not 'daily'");
            }
            format!(
                "Daily Note {}",
                &Local::now().format("%Y-%m-%d").to_string()
            )
        }
    };

    let template_dir = conf.template_directory;
    template_dir
        .try_exists()
        .context(format!("{template_dir:?} does not exist"))?;
    let template_file = template_dir.join(format!(".{}.template", notetype_str));
    template_file
        .try_exists()
        .context(format!("{template_file:?} does not exist"))?;

    let template = fs::read_to_string(template_file.clone())
        .context(format!("unable to read {template_file:?}"))?;
    let out = template
        .replace("{{title}}", title_str.as_ref())
        .replace("{{date}}", &Local::now().format("%Y-%m-%d").to_string());

    let out_path = match cli.subdir {
        None => conf.vault_directory.join(format!("{}.md", title_str)),
        Some(dir) => conf
            .vault_directory
            .join(dir)
            .join(format!("{}.md", title_str)),
    };

    if !out_path.try_exists()? {
        fs::write(out_path.clone(), out)?;
    }

    Command::new("hx")
        .arg("--working-dir")
        .arg(conf.vault_directory)
        .arg(out_path)
        .status()
        .context("failed running helix")?;

    Ok(())
}
