use std::fs::{read_dir, read_to_string};

// Capitalize first letter
fn capitalize(s: &str) -> String {
    let mut chars = s.chars();
    if let Some(first_char) = chars.next() {
        let capitalized = first_char.to_uppercase().chain(chars).collect();
        return capitalized;
    }
    String::new()
}

// Detect distro ID
pub fn distro_id() -> String {
    let mut distro_id = String::new();
    // Check if /etc/lsb-release exists and contains DISTRIB_ID
    if let Ok(file) = read_to_string("/etc/lsb-release") {
        for line in file.lines() {
            if line.starts_with("DISTRIB_ID=") {
                if let Some((_, value)) = line.split_once('=') {
                    distro_id = value.to_lowercase().trim_matches('"').to_string();
                    break;
                }
            }
        }
    }

    // If /etc/lsb-release check fails, check if /etc/os-release exists and contains ID
    if distro_id.is_empty() {
        if let Ok(file) = read_to_string("/etc/os-release") {
            for line in file.lines() {
                if line.starts_with("ID=") {
                    if let Some((_, value)) = line.split_once('=') {
                        distro_id = value.to_lowercase().trim_matches('"').to_string();
                        break;
                    }
                }
            }
        }
    }

    // If both checks fail, loop through all files in /etc/ and check for -release files
    if distro_id.is_empty() {
        // Sort entries for deterministic detection and exclude the
        // os-release/lsb-release files already checked above, otherwise
        // they would be detected as distro "os"/"lsb"
        let mut release_files: Vec<String> = Vec::new();
        if let Ok(entries) = read_dir("/etc") {
            for entry in entries.flatten() {
                if let Some(path) = entry.path().to_str() {
                    if entry.path().is_file()
                        && path.ends_with("-release")
                        && !path.ends_with("/os-release")
                        && !path.ends_with("/lsb-release") {
                        release_files.push(path.to_string());
                    }
                }
            }
            release_files.sort();
        }
        if let Some(path) = release_files.first() {
            let stem = path.rsplit('/').next().unwrap_or("");
            distro_id = stem.strip_suffix("-release")
                            .unwrap_or(stem)
                            .to_lowercase()
                            .split('-')
                            .next()
                            .unwrap_or("")
                            .to_string();
        }
    }
    distro_id
}

// Detect distro name
pub fn distro_name(snapshot: &str) -> String {
    let mut distro_name = String::new();
    // Check /etc/os-release first and use NAME for the distribution name
    if let Ok(file) = read_to_string(format!("/.snapshots/rootfs/snapshot-chr{}/etc/os-release", snapshot)) {
        for line in file.lines() {
            if line.starts_with("NAME=") {
                if let Some((_, value)) = line.split_once('=') {
                    distro_name = value.trim_matches('"').to_string();
                    break;
                }
            }
        }
    }

    // If /etc/os-release parsing fails, check /etc/lsb-release for DISTRIB_DESCRIPTION
    if distro_name.is_empty() {
        if let Ok(file) = read_to_string(format!("/.snapshots/rootfs/snapshot-chr{}/etc/lsb-release", snapshot)) {
            for line in file.lines() {
                if line.starts_with("DISTRIB_DESCRIPTION=") {
                    if let Some((_, value)) = line.split_once('=') {
                        distro_name = value.split(' ').next().unwrap_or("").trim_matches('"').to_string();
                        break;
                    }
                }
            }
        }
    }

    // If both checks fail, loop through all files in /etc/ and check for -release files
    if distro_name.is_empty() {
        // Sort entries for deterministic detection and exclude the
        // os-release/lsb-release files already checked above, otherwise
        // they would be detected as distro "Os"/"Lsb"
        let mut release_files: Vec<String> = Vec::new();
        if let Ok(entries) = read_dir(format!("/.snapshots/rootfs/snapshot-chr{}/etc", snapshot)) {
            for entry in entries.flatten() {
                if let Some(path) = entry.path().to_str() {
                    if entry.path().is_file()
                        && path.ends_with("-release")
                        && !path.ends_with("/os-release")
                        && !path.ends_with("/lsb-release") {
                        release_files.push(path.to_string());
                    }
                }
            }
            release_files.sort();
        }
        if let Some(path) = release_files.first() {
            let stem = path.rsplit('/').next().unwrap_or("");
            let distro_name_from_path = stem.strip_suffix("-release")
                                            .unwrap_or(stem)
                                            .to_lowercase()
                                            .split('-')
                                            .next()
                                            .unwrap_or("")
                                            .to_string();
            distro_name = capitalize(&distro_name_from_path);
        }
    }
    distro_name
}
