use figment::{
    Figment,
    providers::{Env, Format, Toml},
};
use serde::de::DeserializeOwned;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum ConfigError {
    #[error("failed to load config: {0}")]
    Load(#[from] Box<figment::Error>),
}

impl From<figment::Error> for ConfigError {
    fn from(err: figment::Error) -> Self {
        ConfigError::Load(Box::new(err))
    }
}

/// 加载配置，优先级: 环境变量 > config.toml > 默认值
pub fn load_config<T: DeserializeOwned>(path: &str) -> Result<T, ConfigError> {
    let figment = Figment::new()
        .merge(Toml::file(path))
        .merge(Env::prefixed("SIES_").split("__"));

    Ok(figment.extract()?)
}
