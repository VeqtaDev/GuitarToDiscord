//! VST3 plugin discovery.
//!
//! Scans the standard Windows VST3 folders plus any folder the user adds.
//! Hosting (running the plugins on the guitar signal) is the next milestone;
//! see the roadmap in README.md.

use serde::Deserialize;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

#[derive(Clone, Debug)]
pub struct PluginInfo {
    pub name: String,
    pub vendor: Option<String>,
    pub category: Option<String>,
    pub path: PathBuf,
    /// Folder it was found in (for display).
    #[allow(dead_code)]
    pub source: PathBuf,
}

/// Standard locations where installers (Steinberg, Ableton, Native
/// Instruments, Neural DSP, etc.) drop VST3 plugins on Windows.
pub fn default_dirs() -> Vec<PathBuf> {
    let mut v = Vec::new();
    for var in ["CommonProgramFiles", "CommonProgramFiles(x86)", "CommonProgramW6432"] {
        if let Ok(p) = std::env::var(var) {
            v.push(PathBuf::from(p).join("VST3"));
        }
    }
    if let Some(local) = dirs::data_local_dir() {
        v.push(local.join("Programs").join("Common").join("VST3"));
    }
    v.push(PathBuf::from(r"C:\Program Files\Common Files\VST3"));
    v.push(PathBuf::from(r"C:\Program Files\VSTPlugins"));
    v.push(PathBuf::from(r"C:\Program Files\Steinberg\VSTPlugins"));
    // Ableton users often keep a custom "VST3 folder" next to their library.
    if let Some(docs) = dirs::document_dir() {
        v.push(docs.join("Ableton").join("User Library").join("VST3"));
        v.push(docs.join("VST3"));
    }
    v.sort();
    v.dedup();
    v.retain(|p| p.is_dir());
    v
}

#[derive(Deserialize)]
struct ModuleInfo {
    #[serde(rename = "Name")]
    name: Option<String>,
    #[serde(rename = "Factory Info")]
    factory: Option<FactoryInfo>,
    #[serde(rename = "Classes", default)]
    classes: Vec<ClassInfo>,
}

#[derive(Deserialize)]
struct FactoryInfo {
    #[serde(rename = "Vendor")]
    vendor: Option<String>,
}

#[derive(Deserialize)]
struct ClassInfo {
    #[serde(rename = "Category")]
    category: Option<String>,
    #[serde(rename = "Sub Categories", default)]
    sub: Vec<String>,
}

fn read_moduleinfo(bundle: &Path) -> Option<ModuleInfo> {
    let p = bundle.join("Contents").join("Resources").join("moduleinfo.json");
    let p = if p.exists() { p } else { bundle.join("Contents").join("moduleinfo.json") };
    let text = std::fs::read_to_string(p).ok()?;
    // moduleinfo.json is JSON5-ish; most files parse as JSON once trailing commas go.
    let cleaned = text.replace(",\n}", "\n}").replace(",\n]", "\n]");
    serde_json::from_str(&cleaned).ok()
}

/// Scan every folder and return a de-duplicated, sorted list.
pub fn scan(dirs: &[PathBuf]) -> Vec<PluginInfo> {
    let mut out: Vec<PluginInfo> = Vec::new();
    for dir in dirs {
        let mut it = WalkDir::new(dir).follow_links(true).max_depth(6).into_iter();
        while let Some(entry) = it.next() {
            let Ok(entry) = entry else { continue };
            let path = entry.path();
            let is_vst3 = path
                .extension()
                .map(|e| e.eq_ignore_ascii_case("vst3"))
                .unwrap_or(false);
            if !is_vst3 {
                continue;
            }
            if entry.file_type().is_dir() {
                // A bundle: do not descend into it.
                it.skip_current_dir();
            }
            let stem = path
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default();
            let info = if entry.file_type().is_dir() { read_moduleinfo(path) } else { None };
            let (name, vendor, category) = match info {
                Some(m) => {
                    let cat = m
                        .classes
                        .iter()
                        .find(|c| c.category.as_deref() == Some("Audio Module Class"))
                        .and_then(|c| c.sub.first().cloned());
                    (m.name.unwrap_or_else(|| stem.clone()), m.factory.and_then(|f| f.vendor), cat)
                }
                None => (stem.clone(), None, None),
            };
            out.push(PluginInfo {
                name,
                vendor,
                category,
                path: path.to_path_buf(),
                source: dir.clone(),
            });
        }
    }
    out.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    out.dedup_by(|a, b| a.path == b.path);
    out
}
