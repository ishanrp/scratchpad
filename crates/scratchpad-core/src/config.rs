use std::{env, path::PathBuf};

#[derive(Debug, Clone)]
pub struct Paths {
    pub data_dir: PathBuf,
    pub runtime_dir: PathBuf,
    pub socket: PathBuf,
}

impl Paths {
    pub fn discover() -> Self {
        let data_dir = env::var_os("SCRATCHPAD_DATA_DIR").map(PathBuf::from).unwrap_or_else(|| {
            dirs_fallback("XDG_DATA_HOME", ".local/share").join("system-scratchpad")
        });
        let runtime_dir = env::var_os("SCRATCHPAD_RUNTIME_DIR").map(PathBuf::from).unwrap_or_else(|| {
            env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from).unwrap_or_else(|| env::temp_dir().join(format!("system-scratchpad-{}", std::process::id())))
        });
        let socket = env::var_os("SCRATCHPAD_SOCKET").map(PathBuf::from).unwrap_or_else(|| runtime_dir.join("scratchpad.sock"));
        Self { data_dir, runtime_dir, socket }
    }

    pub fn db(&self) -> PathBuf { self.data_dir.join("scratchpad.db") }
    pub fn blobs(&self) -> PathBuf { self.data_dir.join("blobs") }
    pub fn exports(&self) -> PathBuf { self.runtime_dir.join("exports") }
}

fn dirs_fallback(var: &str, suffix: &str) -> PathBuf {
    if let Some(v) = env::var_os(var) { return PathBuf::from(v); }
    let home = env::var_os("HOME").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("."));
    home.join(suffix)
}
