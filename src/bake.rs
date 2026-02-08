use anyhow::{bail, Result};
use owo_colors::OwoColorize;
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

const MAGIC: &[u8; 8] = b"DEKBAKE\0";
const FOOTER_SIZE: usize = 8 + 32 + 8 + 64 + 64; // magic + hash + size + timestamp + user_host

/// Check if current binary has embedded data, extract if needed, return config path
pub fn check_embedded() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let mut file = File::open(&exe).ok()?;

    // Read footer
    let file_size = file.metadata().ok()?.len() as usize;
    if file_size < FOOTER_SIZE {
        return None;
    }

    use std::io::Seek;
    file.seek(std::io::SeekFrom::End(-(FOOTER_SIZE as i64))).ok()?;

    let mut footer = [0u8; FOOTER_SIZE];
    file.read_exact(&mut footer).ok()?;

    // Check magic
    if &footer[0..8] != MAGIC {
        return None;
    }

    // Parse footer
    let hash = std::str::from_utf8(&footer[8..40]).ok()?.trim_end_matches('\0');
    let tar_size = u64::from_le_bytes(footer[40..48].try_into().ok()?);

    // Cache path
    let cache_dir = PathBuf::from(format!("/tmp/dek-{}", hash));

    // Already extracted?
    if cache_dir.exists() {
        return Some(cache_dir);
    }

    // Extract
    file.seek(std::io::SeekFrom::End(-((FOOTER_SIZE + tar_size as usize) as i64))).ok()?;
    let mut tar_data = vec![0u8; tar_size as usize];
    file.read_exact(&mut tar_data).ok()?;

    // Decompress and untar
    let decoder = flate2::read::GzDecoder::new(&tar_data[..]);
    let mut archive = tar::Archive::new(decoder);
    fs::create_dir_all(&cache_dir).ok()?;
    archive.unpack(&cache_dir).ok()?;

    Some(cache_dir)
}

/// Get bake info from embedded footer
pub fn get_bake_info() -> Option<String> {
    let exe = std::env::current_exe().ok()?;
    let mut file = File::open(&exe).ok()?;

    let file_size = file.metadata().ok()?.len() as usize;
    if file_size < FOOTER_SIZE {
        return None;
    }

    use std::io::Seek;
    file.seek(std::io::SeekFrom::End(-(FOOTER_SIZE as i64))).ok()?;

    let mut footer = [0u8; FOOTER_SIZE];
    file.read_exact(&mut footer).ok()?;

    if &footer[0..8] != MAGIC {
        return None;
    }

    let timestamp = std::str::from_utf8(&footer[48..112]).ok()?.trim_end_matches('\0');
    let user_host = std::str::from_utf8(&footer[112..176]).ok()?.trim_end_matches('\0');

    Some(format!("Baked on {} by {}", timestamp, user_host))
}

/// Bake a config path into a standalone binary
pub fn run(config_path: Option<PathBuf>, output: PathBuf) -> Result<()> {
    let config_path = config_path
        .or_else(|| crate::config::find_default_config())
        .ok_or_else(|| anyhow::anyhow!("No config found"))?;

    println!("{}", c!("Baking", bold));
    println!();
    println!("  {} Config: {}", c!("•", blue), config_path.display());
    println!("  {} Output: {}", c!("•", blue), output.display());
    println!();

    // Handle tar.gz input - extract first, then re-tarball
    let actual_path = if crate::util::is_tar_gz(&config_path) {
        println!("  {} Extracting archive...", c!("→", yellow));
        crate::util::extract_tar_gz(&config_path)?
    } else {
        config_path
    };

    // Create tarball of the config path
    println!("  {} Creating archive...", c!("→", yellow));
    let tar_data = create_tarball(&actual_path)?;

    // Hash for cache key
    let hash = format!("{:x}", md5::compute(&tar_data));
    let hash_short = &hash[..32.min(hash.len())];

    // Get current exe
    let exe = std::env::current_exe()?;

    // Copy exe to output
    println!("  {} Writing binary...", c!("→", yellow));
    fs::copy(&exe, &output)?;

    // Append tar data and footer
    let mut out_file = fs::OpenOptions::new().append(true).open(&output)?;
    out_file.write_all(&tar_data)?;

    // Build footer
    let mut footer = [0u8; FOOTER_SIZE];
    footer[0..8].copy_from_slice(MAGIC);

    // Hash (32 bytes, null-padded)
    let hash_bytes = hash_short.as_bytes();
    footer[8..8 + hash_bytes.len().min(32)].copy_from_slice(&hash_bytes[..hash_bytes.len().min(32)]);

    // Tar size (8 bytes)
    footer[40..48].copy_from_slice(&(tar_data.len() as u64).to_le_bytes());

    // Timestamp (64 bytes, null-padded)
    let timestamp = chrono::Local::now().format("%Y-%m-%d %H:%M").to_string();
    let ts_bytes = timestamp.as_bytes();
    footer[48..48 + ts_bytes.len().min(64)].copy_from_slice(&ts_bytes[..ts_bytes.len().min(64)]);

    // User@host (64 bytes, null-padded)
    let user = std::env::var("USER").unwrap_or_else(|_| "unknown".to_string());
    let host = hostname::get()
        .map(|h| h.to_string_lossy().to_string())
        .unwrap_or_else(|_| "unknown".to_string());
    let user_host = format!("{}@{}", user, host);
    let uh_bytes = user_host.as_bytes();
    footer[112..112 + uh_bytes.len().min(64)].copy_from_slice(&uh_bytes[..uh_bytes.len().min(64)]);

    out_file.write_all(&footer)?;

    // Make executable
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(&output)?.permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&output, perms)?;
    }

    let size = fs::metadata(&output)?.len();
    println!("  {} Created {} ({})", c!("✓", green), output.display(), format_size(size));

    Ok(())
}

fn create_tarball(path: &Path) -> Result<Vec<u8>> {
    let mut tar_data = Vec::new();
    {
        let encoder = flate2::write::GzEncoder::new(&mut tar_data, flate2::Compression::default());
        let mut tar = tar::Builder::new(encoder);

        if path.is_file() {
            // Single file - add it with just the filename
            let name = path.file_name().unwrap_or_default();
            tar.append_path_with_name(path, name)?;
        } else if path.is_dir() {
            // Directory - add all contents
            tar.append_dir_all(".", path)?;
        } else {
            bail!("Config path does not exist: {}", path.display());
        }

        tar.into_inner()?.finish()?;
    }
    Ok(tar_data)
}

fn format_size(bytes: u64) -> String {
    if bytes < 1024 {
        format!("{} B", bytes)
    } else if bytes < 1024 * 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    }
}
