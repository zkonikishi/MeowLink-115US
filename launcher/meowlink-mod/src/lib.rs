pub mod client_host;
pub mod generic;
pub mod generic_bridges;
pub mod generic_target;
pub mod native;
pub mod server_settings;
pub mod web;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Serialize)]
pub struct AppError {
    pub code: &'static str,
    pub message: String,
}
pub type Result<T> = std::result::Result<T, AppError>;
pub fn error(code: &'static str, message: impl Into<String>) -> AppError {
    AppError {
        code,
        message: message.into(),
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Config {
    pub schema_version: u32,
    pub target: generic::Target,
    pub player_target_file: PathBuf,
    pub host_executable: PathBuf,
    pub client_directory: PathBuf,
}
impl Config {
    pub fn validate(&self) -> Result<()> {
        if self.schema_version != 1 {
            return Err(error("configuration", "配置版本不匹配。"));
        }
        self.target.validate()?;
        server_settings::updated_target(
            &self.target,
            self.target.host.clone(),
            self.target.port,
            self.target.server_name.clone(),
        )?;
        if [
            &self.player_target_file,
            &self.host_executable,
            &self.client_directory,
        ]
        .iter()
        .any(|p| !p.is_absolute() || p.as_os_str().len() > 4096)
        {
            return Err(error(
                "configuration",
                "频道描述文件、客户端宿主和游戏目录必须填写绝对路径。",
            ));
        }
        Ok(())
    }
    pub fn read(path: &Path) -> Result<Self> {
        let meta = std::fs::symlink_metadata(path)
            .map_err(|_| error("configuration", "配置文件尚未创建或无法读取。"))?;
        if !meta.is_file() || meta.file_type().is_symlink() || meta.len() > 16384 {
            return Err(error("configuration", "配置文件类型或大小无效。"));
        }
        let value: Self = serde_json::from_slice(
            &std::fs::read(path).map_err(|_| error("configuration", "无法读取配置。"))?,
        )
        .map_err(|_| error("configuration", "配置格式无效。"))?;
        value.validate()?;
        Ok(value)
    }
    pub fn mapping(&self) -> Result<generic_target::RemoteMapping> {
        self.validate()?;
        let meta = std::fs::symlink_metadata(&self.player_target_file)
            .map_err(|_| error("descriptor", "无法读取服主提供的频道描述文件。"))?;
        if !meta.is_file() || meta.file_type().is_symlink() || meta.len() > 65536 {
            return Err(error("descriptor", "频道描述文件类型或大小无效。"));
        }
        let bytes = std::fs::read(&self.player_target_file)
            .map_err(|_| error("descriptor", "无法读取频道描述文件。"))?;
        generic_target::parse_remote_artifact(&bytes, &self.target)
    }
}
