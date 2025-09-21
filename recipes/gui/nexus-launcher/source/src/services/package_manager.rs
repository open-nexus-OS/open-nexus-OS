// src/services/package_manager.rs
// Package discovery and management logic

use std::collections::BTreeMap;
use std::path::Path;
use orbclient::Renderer;
use crate::services::package_service::{IconSource, Package};

#[cfg(target_os = "redox")]
static UI_PATH: &'static str = "/ui";
#[cfg(not(target_os = "redox"))]
static UI_PATH: &'static str = "ui";

/// Discover and load all available packages
pub fn get_packages() -> Vec<Package> {
    let mut packages: Vec<Package> = Vec::new();

    // Read Redox /ui/apps
    if let Ok(read_dir) = Path::new(&format!("{}/apps/", UI_PATH)).read_dir() {
        for entry_res in read_dir {
            let entry = match entry_res {
                Ok(x) => x,
                Err(_) => continue,
            };
            if entry.file_type().expect("failed to get file_type").is_file() {
                packages.push(Package::from_path(&entry.path().display().to_string()));
            }
        }
    }

    // Read XDG .desktop applications (if available)
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

    packages.sort_by(|a, b| a.name.cmp(&b.name));
    packages
}

/// Organize packages by category
pub fn organize_packages_by_category(all_packages: Vec<Package>) -> (Vec<Package>, BTreeMap<String, Vec<Package>>, Vec<Package>) {
    let mut root_packages = Vec::new();
    let mut category_packages = BTreeMap::<String, Vec<Package>>::new();
    
    // Split packages by category
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

    // Create start menu packages (categories + system actions)
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

        packages.push({
            let mut package = Package::new();
            package.name = "Go back".to_string();
            package.icon.source = IconSource::Name("mimetypes/inode-directory".into());
            package.icon_small.source = IconSource::Name("mimetypes/inode-directory".into());
            package.exec = "exit".to_string();
            package
        });
    }

    // System actions
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

/// Filter packages that can handle a specific file path
pub fn filter_packages_for_path(packages: &[Package], path: &str) -> Vec<Package> {
    packages.iter()
        .filter(|package| {
            package.accepts.iter().any(|accept| {
                (accept.starts_with('*') && path.ends_with(&accept[1..]))
                    || (accept.ends_with('*') && path.starts_with(&accept[..accept.len() - 1]))
            })
        })
        .cloned()
        .collect()
}

/// Load start icon for the taskbar
pub fn load_start_icon() -> orbimage::Image {
    use orbimage::Image;
    use libnexus::themes::{IconVariant, THEME};
    
    // Start icon on the bar (theme-managed, fallback PNG)
    THEME
        .load_icon_sized("system/start", IconVariant::Auto, None)
        .unwrap_or_else(|| load_png(format!("{}/icons/places/start-here.png", UI_PATH), icon_size() as u32))
}

/// Load PNG icon with fallback
fn load_png<P: AsRef<Path>>(path: P, target: u32) -> orbimage::Image {
    use orbimage::Image;
    let icon = Image::from_path(path).unwrap_or(Image::default());
    size_icon(icon, target)
}

/// Resize icon to target size
fn size_icon(icon: orbimage::Image, target: u32) -> orbimage::Image {
    if icon.width() == target && icon.height() == target {
        return icon;
    }
    icon.resize(target, target, orbimage::ResizeType::Lanczos3).unwrap()
}

/// Get icon size for taskbar
fn icon_size() -> i32 {
    use crate::config::settings::{BAR_HEIGHT, ICON_SCALE};
    (BAR_HEIGHT as f32 * ICON_SCALE).round() as i32
}
