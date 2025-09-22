// src/services/app_catalog.rs
// Discovery of applications and simple filtering for file-open scenarios.
// Also provides category organization and a start-button icon helper.
// This consolidates what you had in package_manager.rs.

use std::path::{Path, PathBuf};

use orbimage::{Image, ResizeType};
use orbclient::Renderer;
use libnexus::themes::{THEME, IconVariant};

use crate::services::package_service::{Package, IconSource};
use crate::config::settings::{BAR_HEIGHT, ICON_SCALE};

#[cfg(target_os = "redox")]
pub const UI_PATH: &str = "/ui";
#[cfg(not(target_os = "redox"))]
pub const UI_PATH: &str = "ui";

/// Discover and load all available packages (Redox /ui/apps + XDG .desktop)
pub fn get_packages() -> Vec<Package> {
    let mut packages: Vec<Package> = Vec::new();

    // 1) Redox /ui/apps
    if let Ok(read_dir) = Path::new(&format!("{}/apps/", UI_PATH)).read_dir() {
        for entry_res in read_dir {
            let Ok(entry) = entry_res else { continue; };
            if entry.file_type().map(|t| t.is_file()).unwrap_or(false) {
                packages.push(Package::from_path(&entry.path().display().to_string()));
            }
        }
    }

    // 2) XDG .desktop applications (if available)
    if let Ok(xdg_dirs) = xdg::BaseDirectories::new() {
        for path in xdg_dirs.find_data_files("applications") {
            if let Ok(read_dir) = path.read_dir() {
                for dir_entry_res in read_dir {
                    let Ok(dir_entry) = dir_entry_res else { continue; };
                    let Ok(id) = dir_entry.file_name().into_string() else { continue; };
                    if let Some(package) = Package::from_desktop_entry(id, &dir_entry.path()) {
                        packages.push(package);
                    }
                }
            }
        }
    }

    // Finalize
    packages.retain(|p| !p.exec.trim().is_empty());
    packages.sort_by(|a, b| a.name.cmp(&b.name));
    packages
}

/// Organize packages into root (no category), map<category, vec>, and start-menu entries.
/// This mirrors your package_manager.rs logic so the bar can source it directly.
pub fn organize_packages_by_category(
    all_packages: Vec<Package>
) -> (Vec<Package>, std::collections::BTreeMap<String, Vec<Package>>, Vec<Package>) {
    use std::collections::BTreeMap;

    let mut root_packages = Vec::new();
    let mut category_packages = BTreeMap::<String, Vec<Package>>::new();

    for package in all_packages {
        if package.categories.is_empty() {
            root_packages.push(package);
        } else {
            for category in package.categories.iter() {
                match category_packages.get_mut(category) {
                    Some(vec) => vec.push(package.clone()),
                    None => {
                        category_packages.insert(category.clone(), vec![package.clone()]);
                    }
                }
            }
        }
    }

    root_packages.sort_by(|a, b| a.id.cmp(&b.id));
    root_packages.retain(|p| !p.exec.trim().is_empty());

    // Build start menu entries (categories + system actions)
    let mut start_packages = Vec::new();

    // Category launchers in start menu — use logical icon ids (theme-managed)
    for (category, packages) in category_packages.iter_mut() {
        start_packages.push({
            let mut package = Package::new();
            package.name = category.to_string();
            package.icon.source = IconSource::Name("mimetypes/inode-directory".into());
            package.icon_small.source = IconSource::Name("mimetypes/inode-directory".into());
            package.exec = format!("category={}", category);
            package
        });

        // Back item for nested UI
        packages.push({
            let mut package = Package::new();
            package.name = "Go back".to_string();
            package.icon.source = IconSource::Name("mimetypes/inode-directory".into());
            package.icon_small.source = IconSource::Name("mimetypes/inode-directory".into());
            package.exec = "exit".to_string();
            package
        });
    }

    // System action: Logout
    start_packages.push({
        let mut package = Package::new();
        package.name = "Logout".to_string();
        package.icon.source = IconSource::Name("actions/system-log-out".into());
        package.icon_small.source = IconSource::Name("actions/system-log-out".into());
        package.exec = "exit".to_string();
        package
    });

    (root_packages, category_packages, start_packages)
}

/// Filter packages that can handle a specific file path (simple wildcard support)
pub fn filter_packages_for_path(packages: &[Package], path: &str) -> Vec<Package> {
    packages.iter()
        .filter(|package| {
            package.accepts.iter().any(|accept| {
                (accept.starts_with('*') && path.ends_with(&accept[1..]))
                    || (accept.ends_with('*') && path.starts_with(&accept[..accept.len().saturating_sub(1)]))
            })
        })
        .cloned()
        .collect()
}

/// Load the Start icon for the taskbar using theme first, PNG fallback second.
pub fn load_start_icon() -> Image {
    THEME
        .load_icon_sized("system/start", IconVariant::Auto, None)
        .unwrap_or_else(|| load_png(format!("{}/icons/places/start-here.png", UI_PATH), icon_size() as u32))
}

/// PNG loader + sizing (legacy fallback)
pub fn load_png<P: AsRef<Path>>(path: P, target: u32) -> Image {
    let icon = Image::from_path(path).unwrap_or(Image::default());
    size_icon(icon, target)
}

/// Resize an icon to the target square size.
pub fn size_icon(icon: Image, target: u32) -> Image {
    if icon.width() == target && icon.height() == target { return icon; }
    icon.resize(target, target, ResizeType::Lanczos3).unwrap()
}

/// Base icon size for the taskbar (BAR_HEIGHT * ICON_SCALE).
fn icon_size() -> i32 {
    (BAR_HEIGHT as f32 * ICON_SCALE).round() as i32
}
