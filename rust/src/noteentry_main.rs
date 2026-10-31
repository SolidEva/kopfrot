use clap::Parser;
use std::{fs, path::PathBuf};

use anyhow::{Context, Error, Result};
use chrono::Local;

mod shared_lib;

#[derive(Parser)]
#[command(version)]
struct Cli {
    /// path to the root of the vault, otherwise assumes we are in the vault
    #[arg(short, long)]
    directory: Option<PathBuf>,
    #[clap(long, short)]
    shared: bool,
    notetype: String,
    title: String,
}

fn main() -> Result<(), Error> {
    let cli = Cli::try_parse().unwrap_or_else(|error| error.exit());

    let directory = match cli.directory {
        Some(directory) => directory,
        None => std::env::current_dir().unwrap_or_default(),
    };

    let template_dir = directory.join("shared/.templates");
    template_dir
        .try_exists()
        .context(format!("{template_dir:?} does not exist"))?;
    let template_file = template_dir.join(format!(".{}.template", cli.notetype));
    template_file
        .try_exists()
        .context(format!("{template_file:?} does not exist"))?;

    let template = fs::read_to_string(template_file.clone())
        .context(format!("unable to read {template_file:?}"))?;
    let out = template
        .replace("{{title}}", cli.title.as_ref())
        .replace("{{date}}", &Local::now().format("%Y-%m-%d").to_string());

    if cli.shared {
        let out_path = directory
            .join("shared")
            .join(format!("\"{}.md\"", cli.title));
        fs::write(out_path.clone(), out)?;
        print!("{out_path:?}");
    } else {
        let out_path = directory.join(format!("{}.md", cli.title));
        fs::write(out_path.clone(), out)?;
        print!("{out_path:?}");
    }

    Ok(())
}
