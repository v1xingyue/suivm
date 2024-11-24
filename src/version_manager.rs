use anyhow::{anyhow, Result};
use futures_util::StreamExt;
use indicatif::{ProgressBar, ProgressStyle};
use reqwest;
use serde::Deserialize;
use std::fs;
use std::path::{Path, PathBuf};
use tokio::io::AsyncWriteExt;

#[derive(Deserialize)]
struct Release {
    tag_name: String,
    assets: Vec<Asset>,
}

#[derive(Deserialize)]
struct Asset {
    name: String,
    browser_download_url: String,
}

pub struct SuiVersionManager {
    base_url: String,
    base_dir: PathBuf,
}

impl SuiVersionManager {
    pub fn new() -> Result<Self> {
        // 在用户目录下创建 .suivm 目录
        let home_dir = dirs::home_dir().ok_or_else(|| anyhow!("Could not find home directory"))?;
        let base_dir = home_dir.join(".suivm");

        // 创建必要的目录结构
        fs::create_dir_all(&base_dir)?;
        fs::create_dir_all(base_dir.join("versions"))?;

        Ok(Self {
            base_url: "https://api.github.com/repos/MystenLabs/sui/releases".to_string(),
            base_dir,
        })
    }

    pub async fn list_remote_versions(&self) -> Result<Vec<(String, bool, bool)>> {
        let client = reqwest::Client::new();
        let releases: Vec<Release> = client
            .get(&self.base_url)
            .header("User-Agent", "sui-version-manager")
            .send()
            .await?
            .json()
            .await?;

        // 获取已安装的版本列表
        let installed_versions = self.list_installed_versions()?;
        // 获取当前默认版本
        let current_version = self.get_current_version().ok();

        // 将远程版本与本地安装状态和默认状态配对
        let versions: Vec<(String, bool, bool)> = releases
            .into_iter()
            .map(|r| {
                let version = r.tag_name;
                let is_installed = installed_versions.contains(&version);
                let is_default = current_version.as_ref().map_or(false, |v| v == &version);
                (version, is_installed, is_default)
            })
            .collect();

        Ok(versions)
    }

    pub async fn download_version(&self, version: &str) -> Result<()> {
        let client = reqwest::Client::new();
        let releases: Vec<Release> = client
            .get(&self.base_url)
            .header("User-Agent", "sui-version-manager")
            .send()
            .await?
            .json()
            .await?;

        // 找到对应版本的 release
        let release = releases
            .into_iter()
            .find(|r| r.tag_name == version)
            .ok_or_else(|| anyhow!("Version {} not found", version))?;

        // 根据系统和架构找到对应的资源文件
        let asset = release
            .assets
            .into_iter()
            .find(|a| a.name.contains("macos") && a.name.contains("arm64"))
            .ok_or_else(|| anyhow!("No compatible binary found for version {}", version))?;

        // 创建版本目录
        let version_dir = self.base_dir.join("versions").join(version);
        fs::create_dir_all(&version_dir)?;

        // 开始下载
        println!("Downloading: {}", asset.name);
        let response = client
            .get(&asset.browser_download_url)
            .header("User-Agent", "sui-version-manager")
            .send()
            .await?;

        // 获取文件大小
        let total_size = response.content_length().unwrap_or(0);

        // 设置进度条
        let pb = ProgressBar::new(total_size);
        pb.set_style(ProgressStyle::default_bar()
            .template("{spinner:.green} [{elapsed_precise}] [{bar:40.cyan/blue}] {bytes}/{total_bytes} ({eta})")
            .unwrap()
            .progress_chars("#>-"));

        // 下载文件并更新进度条
        let tgz_path = version_dir.join(&asset.name);
        let mut file = tokio::fs::File::create(&tgz_path).await?;
        let mut downloaded: u64 = 0;
        let mut stream = response.bytes_stream();

        while let Some(chunk) = stream.next().await {
            let chunk = chunk?;
            file.write_all(&chunk).await?;
            downloaded = downloaded.saturating_add(chunk.len() as u64);
            pb.set_position(downloaded);
        }

        pb.finish_with_message("Download completed");

        println!("Extracting files...");
        // 解压文件
        self.extract_tgz(&tgz_path, &version_dir)?;

        // 下载完成后删除 tgz 文件
        fs::remove_file(tgz_path)?;

        println!("Installation completed successfully!");

        // 在安装完成后添加 shell 配置建议
        self.suggest_shell_config()?;

        Ok(())
    }

    fn extract_tgz<P: AsRef<Path>>(&self, tgz_path: P, target_dir: P) -> Result<()> {
        use flate2::read::GzDecoder;
        use std::fs::File;
        use tar::Archive;

        let tar_gz = File::open(tgz_path)?;
        let tar = GzDecoder::new(tar_gz);
        let mut archive = Archive::new(tar);
        archive.unpack(target_dir)?;

        Ok(())
    }

    pub fn uninstall_version(&self, version: &str) -> Result<()> {
        let version_dir = self.base_dir.join("versions").join(version);

        // 检查版本是否存在
        if !version_dir.exists() {
            return Err(anyhow!("Version {} is not installed", version));
        }

        // 如果这个版本正在使用中，阻止删除
        if let Ok(current_version) = self.get_current_version() {
            if current_version == version {
                return Err(anyhow!("Cannot uninstall the currently active version. Please switch to another version first."));
            }
        }

        // 删除版本目录
        fs::remove_dir_all(version_dir)?;

        Ok(())
    }

    // 辅助方法：获取当前使用的版本
    pub fn get_current_version(&self) -> Result<String> {
        let current_link = self.base_dir.join("current");
        if !current_link.exists() {
            return Err(anyhow!("No version currently in use"));
        }

        let target = fs::read_link(current_link)?;
        let version = target
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| anyhow!("Invalid version link"))?;

        Ok(version.to_string())
    }

    // 新增：获取已安装的版本列表
    pub fn list_installed_versions(&self) -> Result<Vec<String>> {
        let versions_dir = self.base_dir.join("versions");
        let mut versions = Vec::new();

        if versions_dir.exists() {
            for entry in fs::read_dir(versions_dir)? {
                let entry = entry?;
                if entry.file_type()?.is_dir() {
                    if let Some(version) = entry.file_name().to_str() {
                        versions.push(version.to_string());
                    }
                }
            }
        }

        Ok(versions)
    }

    pub fn get_shell_config(&self, shell: &str) -> Result<String> {
        let bin_path = self.base_dir.join("current").join("bin");

        let config = match shell.to_lowercase().as_str() {
            "fish" => format!("set -gx PATH {} $PATH", bin_path.display()),
            "bash" | "zsh" => format!("export PATH=\"{}:$PATH\"", bin_path.display()),
            _ => return Err(anyhow!("Unsupported shell: {}", shell)),
        };

        Ok(config)
    }

    pub fn suggest_shell_config(&self) -> Result<()> {
        // 为每种 shell 类型生成配置
        let bash_config = self.get_shell_config("bash")?;
        let zsh_config = self.get_shell_config("zsh")?;
        let fish_config = self.get_shell_config("fish")?;

        println!("\nTo configure your shell, add the following to your shell config file:");

        println!("\nFor bash (~/.bashrc):");
        println!("{}", bash_config);

        println!("\nFor zsh (~/.zshrc):");
        println!("{}", zsh_config);

        println!("\nFor fish (~/.config/fish/config.fish):");
        println!("{}", fish_config);

        println!("\nThen restart your shell or run:");
        println!("source ~/.bashrc  # for bash");
        println!("source ~/.zshrc   # for zsh");
        println!("source ~/.config/fish/config.fish  # for fish");

        Ok(())
    }

    pub fn set_default_version(&self, version: &str) -> Result<()> {
        let version_dir = self.base_dir.join("versions").join(version);

        // 检查版本是否已安装
        if !version_dir.exists() {
            return Err(anyhow!(
                "Version {} is not installed. Please install it first.",
                version
            ));
        }

        // 创建或更新 current 符号链接
        let current_link = self.base_dir.join("current");
        if current_link.exists() {
            fs::remove_file(&current_link)?;
        }

        // 直接链接到版本目录，因为二进制文件就在这里
        #[cfg(unix)]
        std::os::unix::fs::symlink(&version_dir, current_link)?;

        #[cfg(windows)]
        std::os::windows::fs::symlink_dir(&version_dir, current_link)?;

        // 验证二进制文件是否存在
        let sui_binary = version_dir.join("sui");
        if !sui_binary.exists() {
            return Err(anyhow!("Sui binary not found at: {}", sui_binary.display()));
        }

        Ok(())
    }
}
