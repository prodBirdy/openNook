// Embeds the icon SVGs (assets/icons, assets/agents as "agent-<name>") and the shipped mods
// (mods/<name>/...) as lists the binary carries.

use std::fmt::Write;
use std::path::Path;

fn files(dir: &Path, prefix: &str, out: &mut Vec<(String, String)>) {
    let mut entries: Vec<_> = std::fs::read_dir(dir).unwrap().flatten().collect();
    entries.sort_by_key(|e| e.file_name());
    for e in entries {
        let path = e.path();
        let name = e.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') {
            continue;
        }
        if path.is_dir() {
            files(&path, &format!("{prefix}{name}/"), out);
        } else {
            out.push((format!("{prefix}{name}"), path.canonicalize().unwrap().to_string_lossy().into_owned()));
        }
    }
}

fn main() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let out_dir = std::env::var("OUT_DIR").unwrap();
    println!("cargo:rerun-if-changed=assets");
    println!("cargo:rerun-if-changed=mods");

    let mut icons = String::from("pub static ICON_FILES: &[(&str, &str)] = &[\n");
    for (dir, prefix) in [("assets/icons", ""), ("assets/agents", "agent-")] {
        let mut found = Vec::new();
        files(&root.join(dir), "", &mut found);
        for (name, path) in found {
            if let Some(stem) = name.strip_suffix(".svg") {
                writeln!(icons, "    ({:?}, include_str!({:?})),", format!("{prefix}{stem}"), path).unwrap();
            }
        }
    }
    icons.push_str("];\n");
    std::fs::write(Path::new(&out_dir).join("icons.rs"), icons).unwrap();

    let mut mods = String::from("pub static SHIPPED_MODS: &[(&str, &[u8])] = &[\n");
    let mut found = Vec::new();
    files(&root.join("mods"), "", &mut found);
    for (name, path) in found {
        writeln!(mods, "    ({name:?}, include_bytes!({path:?})),").unwrap();
    }
    mods.push_str("];\n");
    std::fs::write(Path::new(&out_dir).join("mods.rs"), mods).unwrap();
}
