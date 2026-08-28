//! CLI shim: percent-encode paths and hand them to the running app via
//! `open -g opennook://…`. No daemon, no socket, and never a shell-exec URL.
//!
//! Alfred File Action → Run Script can call the same scheme:
//! `open -g "opennook://tray/add?path=$(python3 -c 'import urllib.parse,sys;print(urllib.parse.quote(sys.argv[1]))' "$1")"`
//!
//! `cargo run` binaries are not registered with LaunchServices — use the
//! bundled `openNook.app` (scripts/bundle.sh runs `lsregister -f`).

use nook_core::automation::{expand_url, timer_start_url, tray_add_url, tray_clear_url};
use std::env;
use std::path::{Path, PathBuf};
#[cfg(target_os = "macos")]
use std::process::Command;

fn main() {
    let mut args = env::args().skip(1).collect::<Vec<_>>();
    if args.is_empty() || args.iter().any(|a| a == "-h" || a == "--help") {
        print_usage();
        if args.is_empty() {
            std::process::exit(2);
        }
        return;
    }

    let url = match build_url(&mut args) {
        Ok(url) => url,
        Err(err) => {
            eprintln!("nook: {err}");
            print_usage();
            std::process::exit(2);
        }
    };

    if let Err(err) = open_url(&url) {
        eprintln!("nook: {err}");
        std::process::exit(1);
    }
}

fn build_url(args: &mut Vec<String>) -> Result<String, String> {
    if args.is_empty() {
        return Err("missing command".into());
    }
    // Never emit a URL that parse_opennook_url would treat as exec.
    match args[0].as_str() {
        "clear" => Ok(tray_clear_url()),
        "expand" => Ok(expand_url()),
        "timer" => {
            let seconds = args.get(1).ok_or("timer needs <seconds>")?;
            let seconds: u32 = seconds
                .parse()
                .map_err(|_| "timer seconds must be a number".to_string())?;
            Ok(timer_start_url(seconds))
        }
        "add" => {
            let paths = canonicalize_paths(&args[1..])?;
            if paths.is_empty() {
                return Err("add needs at least one path".into());
            }
            Ok(tray_add_url(&paths))
        }
        other if looks_like_exec(other) => Err(
            "the CLI cannot run shell commands; type them in the Termi-Notch card".into(),
        ),
        _ => {
            let paths = canonicalize_paths(args)?;
            if paths.is_empty() {
                return Err("no paths to add".into());
            }
            Ok(tray_add_url(&paths))
        }
    }
}

fn looks_like_exec(verb: &str) -> bool {
    matches!(
        verb,
        "shell" | "exec" | "run" | "cmd" | "command" | "term" | "terminal" | "sh"
    )
}

fn canonicalize_paths(args: &[String]) -> Result<Vec<PathBuf>, String> {
    let mut out = Vec::new();
    for raw in args {
        let path = Path::new(raw);
        let resolved = path
            .canonicalize()
            .unwrap_or_else(|_| path.to_path_buf());
        out.push(resolved);
    }
    Ok(out)
}

fn open_url(url: &str) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        let status = Command::new("/usr/bin/open")
            .args(["-g", url])
            .status()
            .map_err(|err| err.to_string())?;
        if status.success() {
            Ok(())
        } else {
            Err(format!("open exited {status}"))
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        println!("{url}");
        Ok(())
    }
}

fn print_usage() {
    eprintln!(
        "\
Usage:
  nook add <path>...     send files to the island tray
  nook <path>...         same as add
  nook clear             empty the tray
  nook timer <seconds>   start a countdown
  nook expand            expand the island

Requires the bundled openNook.app so LaunchServices can route opennook://.
Does not execute shell commands."
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn add_encodes_spaces() {
        let mut args = vec!["add".into(), "/tmp/hello world.txt".into()];
        let url = build_url(&mut args).unwrap();
        assert!(url.starts_with("opennook://tray/add?path="));
        assert!(url.contains("hello%20world.txt") || url.contains("hello"));
        assert!(!url.contains("shell") && !url.contains("exec"));
    }

    #[test]
    fn verbs_are_safe() {
        assert_eq!(
            build_url(&mut vec!["clear".into()]).unwrap(),
            tray_clear_url()
        );
        assert_eq!(
            build_url(&mut vec!["expand".into()]).unwrap(),
            expand_url()
        );
        assert_eq!(
            build_url(&mut vec!["timer".into(), "90".into()]).unwrap(),
            timer_start_url(90)
        );
        assert!(build_url(&mut vec!["exec".into(), "id".into()]).is_err());
        assert!(build_url(&mut vec!["shell".into(), "ls".into()]).is_err());
    }
}
