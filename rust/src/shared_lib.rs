use clap::Parser;
use std::{fs, path::Path, path::PathBuf};

use anyhow::{Context, Error, Result};
use serde::{Deserialize, Serialize};
use serde_yaml;

#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub struct RawNote {
    pub properties: serde_yaml::Value,
    pub content: String,
}

impl RawNote {
    pub fn from_string(raw: String) -> Result<RawNote, Error> {
        let has_start_properties = raw
            .lines()
            .next()
            .is_some_and(|line| line.trim_end() == "---");

        if has_start_properties {
            let closed = raw["---".len()..]
                .find("---")
                .context("has properties opener but no closer")?;

            return Ok(RawNote {
                content: raw[(closed + 2 * "...".len())..].trim().to_string(),
                properties: serde_yaml::from_str(raw["...".len()..(closed + "...".len())].trim())?,
            });
        }

        Err(Error::msg("note without properties unsupported"))
    }
}

// adapted from https://stackoverflow.com/questions/26076005/how-can-i-list-files-of-a-directory-in-rust
pub fn recurse_files(path: impl AsRef<Path>, follow_hidden: bool) -> std::io::Result<Vec<PathBuf>> {
    let mut buf = vec![];
    let entries = fs::read_dir(path)?;

    for entry in entries {
        let entry = entry?;
        let meta = entry.metadata()?;

        if meta.is_dir() {
            if !entry
                .path()
                .file_name()
                .expect("directory does not have a filename")
                .to_string_lossy()
                .starts_with(".")
                || entry
                    .path()
                    .file_name()
                    .expect("directory does not have a file name")
                    .to_string_lossy()
                    .starts_with(".")
                    && follow_hidden == true
            {
                let mut subdir = recurse_files(entry.path(), follow_hidden)?;
                buf.append(&mut subdir);
            }
        }

        if meta.is_file() {
            buf.push(entry.path());
        }
    }

    Ok(buf)
}
