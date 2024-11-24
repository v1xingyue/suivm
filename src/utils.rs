pub fn get_os_name() -> String {
    std::env::consts::OS.to_string()
}

pub fn get_cpu_arch() -> String {
    match std::env::consts::ARCH {
        "x86_64" => "x86_64".to_string(),
        "aarch64" => "arm64".to_string(),
        _ => "unknown".to_string(),
    }
}
