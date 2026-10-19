use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct StandaloneConfig {
    pub data_dir: PathBuf,
}

impl Default for StandaloneConfig {
    fn default() -> Self {
        Self {
            data_dir: PathBuf::from("./data/standalone"),
        }
    }
}
