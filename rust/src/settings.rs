use anyhow::{Context, Error, Result};
use config::Config;
use serde::Deserialize;
use std::path::{Path, PathBuf};

#[derive(Debug, Deserialize)]
pub struct Settings {
    pub vault_directory: PathBuf,
    pub template_directory: PathBuf,
}

pub fn load_config(config_file: Option<&Path>) -> Result<Settings, Error> {
    let mut conf = Config::builder()
        .add_source(config::File::from(Path::new("~/.config/kopfrot/config.toml")).required(false))
        .add_source(config::File::from(Path::new("/etc/xdg/kopfrot/config.toml")).required(false))
        .add_source(config::File::from(Path::new(".kopfrot.toml")).required(false));

    conf = match config_file {
        Some(config_file) => conf.add_source(config::File::from(config_file)),
        None => conf,
    };
    // Add in settings from the environment (with a prefix of APP)
    // Eg.. `APP_DEBUG=1 ./target/app` would set the `debug` key
    let settings = conf
        .clone()
        .add_source(config::Environment::with_prefix("KOPFROT"))
        .build()?
        .try_deserialize::<Settings>()
        .context(format!(
            "failed to deserialize settings conf {conf:?}, did you create a config file?"
        ));

    // println!("{:?}", settings);

    settings
}
