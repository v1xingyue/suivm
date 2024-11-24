mod utils;
mod version_manager;

use clap::{Parser, Subcommand};
use version_manager::SuiVersionManager;

#[derive(Parser)]
#[command(name = "suivm")]
#[command(about = "Sui Version Manager", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// List all available Sui versions
    List,
    /// Install a specific version
    Install {
        #[arg(name = "VERSION")]
        version: String,
    },
    /// Uninstall a specific version
    Uninstall {
        #[arg(name = "VERSION")]
        version: String,
    },
    /// Show shell configuration
    Config {
        #[arg(value_enum, default_value = "bash")]
        #[arg(help = "Shell type (bash/zsh/fish)")]
        shell: Shell,
    },
    /// Set default version
    Use {
        #[arg(name = "VERSION")]
        version: String,
    },
}

#[derive(clap::ValueEnum, Clone)]
enum Shell {
    Bash,
    Zsh,
    Fish,
}

#[tokio::main]
async fn main() {
    let cli = Cli::parse();

    // 验证操作系统和架构
    let os_name = utils::get_os_name();
    if os_name != "macos" {
        println!("Sorry, this program only supports macOS.");
        return;
    }
    let _cpu_arch = utils::get_cpu_arch();

    // 初始化版本管理器
    let manager = match SuiVersionManager::new() {
        Ok(m) => m,
        Err(e) => {
            println!("Failed to initialize version manager: {}", e);
            return;
        }
    };

    match cli.command {
        Commands::List => match manager.list_remote_versions().await {
            Ok(versions) => {
                println!("Available versions:");
                for (version, is_installed, is_default) in versions {
                    let install_marker = if is_installed { "[*]" } else { "[ ]" };
                    let default_marker = if is_default { " (default)" } else { "" };
                    println!("{} {}{}", install_marker, version, default_marker);
                }
            }
            Err(e) => println!("Failed to fetch versions: {}", e),
        },
        Commands::Install { version } => {
            println!("Installing version: {}", version);
            match manager.download_version(&version).await {
                Ok(_) => {
                    println!("Successfully installed version {}", version);
                    println!("\nTo configure shell integration, run:");
                    println!("  suivm config bash  # for bash");
                    println!("  suivm config zsh   # for zsh");
                }
                Err(e) => println!("Failed to install version {}: {}", version, e),
            }
        }
        Commands::Uninstall { version } => {
            println!("Uninstalling version: {}", version);
            match manager.uninstall_version(&version) {
                Ok(_) => println!("Successfully uninstalled version {}", version),
                Err(e) => println!("Failed to uninstall version {}: {}", version, e),
            }
        }
        Commands::Config { shell } => {
            let shell_str = match shell {
                Shell::Bash => "bash",
                Shell::Zsh => "zsh",
                Shell::Fish => "fish",
            };

            match manager.get_shell_config(shell_str) {
                Ok(config) => {
                    let rc_file = match shell {
                        Shell::Bash => "~/.bashrc",
                        Shell::Zsh => "~/.zshrc",
                        Shell::Fish => "~/.config/fish/config.fish",
                    };
                    println!("Add the following to your {}:", rc_file);
                    println!("\n{}\n", config);
                    println!("Then restart your shell or run:");
                    match shell {
                        Shell::Fish => println!("source {}", rc_file),
                        _ => println!("source {} # or restart your terminal", rc_file),
                    }
                }
                Err(e) => println!("Failed to generate shell config: {}", e),
            }
        }
        Commands::Use { version } => {
            println!("Setting default version to: {}", version);
            match manager.set_default_version(&version) {
                Ok(_) => println!("Successfully set default version to {}", version),
                Err(e) => println!("Failed to set default version: {}", e),
            }
        }
    }
}
