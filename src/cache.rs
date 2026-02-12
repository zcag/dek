use std::fs;
use std::path::PathBuf;
use std::time::Duration;

fn base_dir() -> PathBuf {
    std::env::var("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            std::env::var("HOME")
                .map(|h| PathBuf::from(h).join(".cache"))
                .unwrap_or_else(|_| PathBuf::from("/tmp"))
        })
        .join("dek")
}

fn cache_dir() -> PathBuf {
    base_dir().join("url")
}

fn cache_path(url: &str) -> PathBuf {
    let hash = format!("{:x}", md5::compute(url));
    cache_dir().join(hash)
}

pub fn get(url: &str, max_age: Option<Duration>) -> Option<Vec<u8>> {
    let path = cache_path(url);
    if !path.exists() {
        return None;
    }
    if let Some(max_age) = max_age {
        let modified = fs::metadata(&path).ok()?.modified().ok()?;
        if modified.elapsed().ok()? > max_age {
            return None;
        }
    }
    fs::read(&path).ok()
}

pub fn set(url: &str, data: &[u8]) {
    let path = cache_path(url);
    let _ = fs::create_dir_all(path.parent().unwrap());
    let _ = fs::write(&path, data);
}

// =============================================================================
// State cache — stores cache_key values for step skipping
// =============================================================================

fn state_dir() -> PathBuf {
    base_dir().join("state")
}

fn state_path(item_id: &str) -> PathBuf {
    let hash = format!("{:x}", md5::compute(item_id));
    state_dir().join(hash)
}

pub fn get_state(item_id: &str) -> Option<String> {
    fs::read_to_string(state_path(item_id)).ok()
}

pub fn set_state(item_id: &str, value: &str) {
    let path = state_path(item_id);
    let _ = fs::create_dir_all(path.parent().unwrap());
    let _ = fs::write(&path, value);
}
