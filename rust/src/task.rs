use std::{fs, path::Path, path::PathBuf};

use anyhow::{Context, Error, Result};
use serde::{Deserialize, Serialize};
use serde_yaml;

use crate::note;

#[derive(Clone, PartialEq, Debug)]
pub enum TaskState {
    Open,
    InProgress,
    Done,
}

impl TaskState {
    pub fn from_string(raw_str: &str) -> Result<TaskState, Error> {
        match raw_str.to_lowercase().as_str() {
            "open" => Ok(TaskState::Open),
            "inprogress" => Ok(TaskState::InProgress),
            "done" => Ok(TaskState::Done),
            _ => Err(Error::msg(format!("unsupported task state {raw_str}"))),
        }
    }
}

#[derive(PartialEq, Debug, Clone)]
pub struct Task {
    pub title: String,
    pub state: TaskState,
    pub path: PathBuf,
}

impl Task {
    pub fn from_path(path: &PathBuf) -> Result<Option<Task>, Error> {
        let file_string = fs::read_to_string(path)?;
        Task::from_string(file_string, path.clone())
    }

    pub fn from_string(raw_str: String, path: PathBuf) -> Result<Option<Task>, Error> {
        let raw = note::RawNote::from_string(raw_str)?;

        let state_str: String = match raw.properties.get("state") {
            Some(state) => serde_yaml::from_value(state.clone())?,
            None => {
                println!("ignoring note at path {path:?} as it does not have a task state");
                return Ok(None);
            }
        };

        let state = TaskState::from_string(&state_str)?;

        // first heading on note is assumed to be the title
        let title = raw
            .content
            .lines()
            .filter(|line| line.starts_with("# "))
            .next()
            .context("unable to find first header to treat as title")?
            .strip_prefix("# ")
            .context("unable to remove prefix from title line")?
            .to_string();

        Ok(Some(Task { title, state, path }))
    }

    pub const fn ref_array(&self) -> [&String; 1] {
        [&self.title]
    }

    pub fn title(&self) -> &str {
        &self.title
    }

    fn state(&self) -> &TaskState {
        &self.state
    }
}

pub fn tasks_from_vault(directory: PathBuf) -> Result<Vec<Task>, Error> {
    let note_paths = note::recurse_files(directory, false)?
        .iter()
        .filter_map(|path| {
            if path.extension().map_or(false, |ext| ext == "md") {
                Some(path)
            } else {
                None
            }
        })
        .filter_map(|path| {
            if path.file_name().map_or(false, |file_name| {
                file_name.to_string_lossy().starts_with(".")
            }) {
                None
            } else {
                Some(path.to_owned())
            }
        })
        .collect::<Vec<_>>();

    println!("{note_paths:?}");

    let tasks = note_paths
        .iter()
        .filter_map(|path| {
            let res = Task::from_path(path);
            match res {
                Ok(res) => Some(res),
                Err(res) => {
                    eprintln!("note at path {path:?} is malformed: {res}");
                    None
                }
            }
        })
        .flatten()
        .collect::<Vec<Task>>();
    for task in tasks.clone() {
        println!("Read YAML string: {:?}", task);
    }

    Ok(tasks)
}
