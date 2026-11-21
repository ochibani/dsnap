use libbtrfs::subvolume::{new, remove, set_default_subvol, set_readonly, snapshot};
use std::cmp::Reverse;
use std::io::{Error, ErrorKind};
use std::process::Command;

// Create btrfs snapshot
pub fn create_snapshot(snapvol: &str, pathname: &str, readonly: bool) -> Result<(), Error> {
    snapshot(snapvol, pathname, readonly)?;
    Ok(())
}

// Create btrfs subvolume
pub fn create_subvolume(pathname: &str) -> Result<(), Error> {
    new(pathname)?;
    Ok(())
}

// Delete btrfs subvolume, including any nested subvolumes
pub fn delete_subvolume(pathname: &str) -> Result<(), Error> {
    match remove(pathname) {
        Ok(()) => Ok(()),
        Err(e) if e.raw_os_error() == Some(39) => {
            // btrfs refuses to remove a subvolume while it still contains
            // nested subvolumes (for example /var/lib/machines). Remove the
            // nested subvolumes deepest-first, then retry the parent.
            for nested in nested_subvolumes(pathname)? {
                remove(&nested)?;
            }
            remove(pathname)
        }
        Err(e) => Err(e),
    }
}

// Find subvolumes nested under pathname. btrfs reports paths relative to the
// top-level subvolume mounted at /.snapshots, so drop that first component
// and rebuild the absolute dsnap path.
fn nested_subvolumes(pathname: &str) -> Result<Vec<String>, Error> {
    let output = Command::new("btrfs")
        .args(["subvolume", "list", "-o", pathname])
        .output()?;
    if !output.status.success() {
        return Err(Error::new(
            ErrorKind::Other,
            format!("Failed to list nested subvolumes: {}",
                    String::from_utf8_lossy(&output.stderr).trim())));
    }
    let prefix = format!("{}/", pathname.trim_end_matches('/'));
    let mut nested: Vec<String> = String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|line| line.split_once(" path ").map(|(_, p)| p.trim()))
        .filter_map(|listed| {
            let mut parts: Vec<&str> = listed.split('/').collect();
            if parts.len() < 2 {
                return None;
            }
            parts.remove(0);
            let absolute = format!("/.snapshots/{}", parts.join("/"));
            if absolute.starts_with(&prefix) {
                Some(absolute)
            } else {
                None
            }
        })
        .collect();
    nested.sort_by_key(|p| Reverse(p.len()));
    Ok(nested)
}

// Set default subvolume
pub fn set_default(subvol: &str) -> Result<(), Error> {
    set_default_subvol(subvol)?;
    Ok(())
}

// Set read only flag for subvolume
pub fn set_subvolume_read_only(subv: &str, readonly: bool) -> Result<(), Error> {
    set_readonly(subv, readonly)?;
    Ok(())
}
