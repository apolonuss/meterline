use anyhow::{Context, Result};
use directories::ProjectDirs;
use std::env;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug)]
pub struct AppPaths {
    data_dir: PathBuf,
}

impl AppPaths {
    pub fn discover() -> Result<Self> {
        if let Some(home) = env::var_os("METERLINE_HOME") {
            return Ok(Self {
                data_dir: PathBuf::from(home),
            });
        }

        let dirs = ProjectDirs::from("dev", "meterline", "Meterline")
            .context("could not determine a platform data directory")?;
        Ok(Self {
            data_dir: dirs.data_local_dir().to_path_buf(),
        })
    }

    pub fn from_dir(path: impl Into<PathBuf>) -> Self {
        Self {
            data_dir: path.into(),
        }
    }

    pub fn data_dir(&self) -> &Path {
        &self.data_dir
    }

    pub fn database_path(&self) -> PathBuf {
        self.data_dir.join("meterline.sqlite3")
    }

    pub fn ensure(&self) -> Result<()> {
        std::fs::create_dir_all(&self.data_dir)
            .with_context(|| format!("could not create {}", self.data_dir.display()))
    }
}
