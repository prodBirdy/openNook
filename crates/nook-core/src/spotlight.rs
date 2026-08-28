//! Spotlight index queries via in-process MDQuery (not NSMetadataQuery, not `mdfind`).
//!
//! Query-string building and ranking are pure and tested on every target.
//! The CoreServices call is macOS-only; Linux returns an empty hit list.

use std::sync::atomic::{AtomicU64, Ordering};

pub const MAX_HITS: usize = 40;
pub const DEBOUNCE_MS: u64 = 200;

static QUERY_GEN: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedQuery {
    pub clipboard_only: bool,
    pub term: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SearchHit {
    pub path: String,
    pub display_name: String,
    pub content_type: String,
    pub is_app: bool,
}

/// `;term` (Raycast-style) or a leading tab means clipboard-only.
pub fn parse_search_query(raw: &str) -> ParsedQuery {
    let trimmed = raw.trim_start_matches([' ', '\n', '\r']);
    if let Some(rest) = trimmed.strip_prefix(';') {
        return ParsedQuery {
            clipboard_only: true,
            term: rest.trim().to_string(),
        };
    }
    if let Some(rest) = trimmed.strip_prefix('\t') {
        return ParsedQuery {
            clipboard_only: true,
            term: rest.trim().to_string(),
        };
    }
    ParsedQuery {
        clipboard_only: false,
        term: raw.trim().to_string(),
    }
}

/// Escape a user term for an MDQuery comparison value.
pub fn escape_mdquery_term(term: &str) -> String {
    let mut out = String::with_capacity(term.len());
    for c in term.chars() {
        match c {
            '\\' | '"' | '*' | '?' | '(' | ')' => {
                out.push('\\');
                out.push(c);
            }
            _ => out.push(c),
        }
    }
    out
}

/// Spotlight predicate: display-name substring or text-content prefix, case/diacritic insensitive.
pub fn build_mdquery(term: &str) -> Option<String> {
    let term = term.trim();
    if term.is_empty() {
        return None;
    }
    let escaped = escape_mdquery_term(term);
    Some(format!(
        "(kMDItemDisplayName == \"*{escaped}*\"cdw) || (kMDItemTextContent == \"{escaped}*\"cd)"
    ))
}

pub fn is_application(content_type: &str, type_tree: &[String]) -> bool {
    content_type == "com.apple.application-bundle"
        || content_type == "com.apple.application"
        || type_tree.iter().any(|t| {
            t == "com.apple.application-bundle" || t == "com.apple.application"
        })
}

/// Apps first, then prefix matches on the display name, then the rest.
pub fn rank_hits(mut hits: Vec<SearchHit>, term: &str) -> Vec<SearchHit> {
    let needle = term.trim().to_ascii_lowercase();
    hits.sort_by(|a, b| {
        rank_key(a, &needle)
            .cmp(&rank_key(b, &needle))
            .then_with(|| a.display_name.to_ascii_lowercase().cmp(&b.display_name.to_ascii_lowercase()))
    });
    if hits.len() > MAX_HITS {
        hits.truncate(MAX_HITS);
    }
    hits
}

fn rank_key(hit: &SearchHit, needle: &str) -> (u8, u8) {
    let name = hit.display_name.to_ascii_lowercase();
    let prefix = !needle.is_empty() && name.starts_with(needle);
    let app = if hit.is_app { 0 } else { 1 };
    let prefix_rank = if prefix { 0 } else { 1 };
    (app, prefix_rank)
}

/// Bump the generation so an in-flight MDQuery's results are discarded.
pub fn cancel_prior_query() -> u64 {
    QUERY_GEN.fetch_add(1, Ordering::Relaxed) + 1
}

pub fn current_query_gen() -> u64 {
    QUERY_GEN.load(Ordering::Relaxed)
}

/// Synchronous one-shot MDQuery on the calling thread. Caller should run this
/// on a background executor and ignore results whose generation no longer matches.
pub fn query(term: &str, gen: u64) -> Vec<SearchHit> {
    let Some(predicate) = build_mdquery(term) else {
        return Vec::new();
    };
    #[cfg(target_os = "macos")]
    {
        query_macos(&predicate, term, gen)
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (predicate, gen);
        Vec::new()
    }
}

#[cfg(target_os = "macos")]
fn query_macos(predicate: &str, term: &str, gen: u64) -> Vec<SearchHit> {
    use std::ffi::{CStr, CString};
    use std::os::raw::{c_char, c_void};
    use std::ptr;

    type CFTypeRef = *const c_void;
    type CFStringRef = *const c_void;
    type CFArrayRef = *const c_void;
    type CFAllocatorRef = *const c_void;
    type MDQueryRef = *mut c_void;
    type MDItemRef = *const c_void;
    type CFIndex = isize;

    const K_MD_QUERY_SYNCHRONOUS: u32 = 1;
    const K_CF_STRING_ENCODING_UTF8: u32 = 0x0800_0100;

    #[link(name = "CoreFoundation", kind = "framework")]
    extern "C" {
        fn CFRelease(cf: CFTypeRef);
        fn CFStringCreateWithCString(
            alloc: CFAllocatorRef,
            c_str: *const c_char,
            encoding: u32,
        ) -> CFStringRef;
        fn CFStringGetCString(
            the_string: CFStringRef,
            buffer: *mut c_char,
            buffer_size: CFIndex,
            encoding: u32,
        ) -> u8;
    }

    #[link(name = "CoreServices", kind = "framework")]
    extern "C" {
        fn MDQueryCreate(
            allocator: CFAllocatorRef,
            query_string: CFStringRef,
            value_list_attrs: CFArrayRef,
            sorting_attrs: CFArrayRef,
        ) -> MDQueryRef;
        fn MDQuerySetMaxCount(query: MDQueryRef, max: CFIndex);
        fn MDQueryExecute(query: MDQueryRef, flags: u32) -> u8;
        fn MDQueryGetResultCount(query: MDQueryRef) -> CFIndex;
        fn MDQueryGetResultAtIndex(query: MDQueryRef, idx: CFIndex) -> MDItemRef;
        fn MDItemCopyAttribute(item: MDItemRef, name: CFStringRef) -> CFTypeRef;
        fn MDQueryStop(query: MDQueryRef);
    }

    unsafe fn cfstring(s: &str) -> CFStringRef {
        let c = CString::new(s.replace('\0', "")).unwrap_or_default();
        CFStringCreateWithCString(ptr::null(), c.as_ptr(), K_CF_STRING_ENCODING_UTF8)
    }

    unsafe fn cfstring_to_rust(s: CFStringRef) -> Option<String> {
        if s.is_null() {
            return None;
        }
        let mut buf = [0i8; 4096];
        let ok = CFStringGetCString(
            s,
            buf.as_mut_ptr(),
            buf.len() as CFIndex,
            K_CF_STRING_ENCODING_UTF8,
        );
        if ok == 0 {
            return None;
        }
        CStr::from_ptr(buf.as_ptr()).to_str().ok().map(|s| s.to_string())
    }

    unsafe fn release(cf: CFTypeRef) {
        if !cf.is_null() {
            CFRelease(cf);
        }
    }

    unsafe {
        let query_str = cfstring(predicate);
        if query_str.is_null() {
            return Vec::new();
        }

        let path_key = cfstring("kMDItemPath");
        let name_key = cfstring("kMDItemDisplayName");
        let type_key = cfstring("kMDItemContentType");
        let tree_key = cfstring("kMDItemContentTypeTree");
        // Value-list attrs are optional; we copy attributes off each MDItemRef.
        let query = MDQueryCreate(ptr::null(), query_str, ptr::null(), ptr::null());
        release(query_str);
        if query.is_null() {
            release(path_key);
            release(name_key);
            release(type_key);
            release(tree_key);
            return Vec::new();
        }

        MDQuerySetMaxCount(query, MAX_HITS as CFIndex);
        if current_query_gen() != gen {
            MDQueryStop(query);
            release(query as CFTypeRef);
            release(path_key);
            release(name_key);
            release(type_key);
            release(tree_key);
            return Vec::new();
        }

        let ok = MDQueryExecute(query, K_MD_QUERY_SYNCHRONOUS);
        if current_query_gen() != gen || ok == 0 {
            MDQueryStop(query);
            release(query as CFTypeRef);
            release(path_key);
            release(name_key);
            release(type_key);
            release(tree_key);
            return Vec::new();
        }

        let count = MDQueryGetResultCount(query).max(0) as usize;
        let mut hits = Vec::with_capacity(count.min(MAX_HITS));
        for i in 0..count.min(MAX_HITS) {
            if current_query_gen() != gen {
                break;
            }
            let item = MDQueryGetResultAtIndex(query, i as CFIndex);
            if item.is_null() {
                continue;
            }
            let path_cf = MDItemCopyAttribute(item, path_key);
            let name_cf = MDItemCopyAttribute(item, name_key);
            let type_cf = MDItemCopyAttribute(item, type_key);
            let tree_cf = MDItemCopyAttribute(item, tree_key);
            let path = cfstring_to_rust(path_cf).unwrap_or_default();
            let display_name = cfstring_to_rust(name_cf).unwrap_or_else(|| {
                std::path::Path::new(&path)
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_else(|| path.clone())
            });
            let content_type = cfstring_to_rust(type_cf).unwrap_or_default();
            let mut tree = Vec::new();
            if !tree_cf.is_null() {
                // Best-effort: treat a string tree entry as a single type.
                if let Some(one) = cfstring_to_rust(tree_cf) {
                    tree.push(one);
                }
            }
            release(path_cf);
            release(name_cf);
            release(type_cf);
            release(tree_cf);
            if path.is_empty() {
                continue;
            }
            let is_app = is_application(&content_type, &tree)
                || path.ends_with(".app")
                || display_name.ends_with(".app");
            hits.push(SearchHit {
                path,
                display_name,
                content_type,
                is_app,
            });
        }

        MDQueryStop(query);
        release(query as CFTypeRef);
        release(path_key);
        release(name_key);
        release(type_key);
        release(tree_key);

        if current_query_gen() != gen {
            return Vec::new();
        }
        rank_hits(hits, term)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn query_string_builder_escapes_and_rejects_empty() {
        assert_eq!(build_mdquery("   "), None);
        assert_eq!(build_mdquery(""), None);
        let q = build_mdquery("Notes").unwrap();
        assert!(q.contains("kMDItemDisplayName == \"*Notes*\"cdw"));
        assert!(q.contains("kMDItemTextContent == \"Notes*\"cd"));
        let escaped = build_mdquery(r#"foo"bar*"#).unwrap();
        assert!(escaped.contains(r#"foo\"bar\*"#));
        assert_eq!(escape_mdquery_term(r#"(x?)"#), r#"\(x\?\)"#);
    }

    #[test]
    fn parse_query_semicolon_or_tab_is_clipboard_only() {
        assert_eq!(
            parse_search_query("safari"),
            ParsedQuery {
                clipboard_only: false,
                term: "safari".into(),
            }
        );
        assert_eq!(
            parse_search_query("; password"),
            ParsedQuery {
                clipboard_only: true,
                term: "password".into(),
            }
        );
        assert_eq!(
            parse_search_query("\tclip"),
            ParsedQuery {
                clipboard_only: true,
                term: "clip".into(),
            }
        );
        assert_eq!(
            parse_search_query(";"),
            ParsedQuery {
                clipboard_only: true,
                term: String::new(),
            }
        );
    }

    #[test]
    fn ranking_puts_apps_and_prefix_matches_first() {
        let hits = vec![
            SearchHit {
                path: "/tmp/notes.txt".into(),
                display_name: "notes.txt".into(),
                content_type: "public.plain-text".into(),
                is_app: false,
            },
            SearchHit {
                path: "/Applications/Notes.app".into(),
                display_name: "Notes".into(),
                content_type: "com.apple.application-bundle".into(),
                is_app: true,
            },
            SearchHit {
                path: "/Applications/Safari.app".into(),
                display_name: "Safari".into(),
                content_type: "com.apple.application-bundle".into(),
                is_app: true,
            },
            SearchHit {
                path: "/tmp/notebook.pdf".into(),
                display_name: "notebook.pdf".into(),
                content_type: "com.adobe.pdf".into(),
                is_app: false,
            },
        ];
        let ranked = rank_hits(hits, "no");
        assert!(ranked[0].is_app && ranked[0].display_name == "Notes");
        assert!(ranked[1].is_app && ranked[1].display_name == "Safari");
        assert_eq!(ranked[2].display_name, "notebook.pdf");
        assert_eq!(ranked[3].display_name, "notes.txt");
        assert!(is_application(
            "public.item",
            &["public.item".into(), "com.apple.application-bundle".into()]
        ));
    }

    #[test]
    fn rank_hits_caps_at_max() {
        let hits: Vec<SearchHit> = (0..80)
            .map(|i| SearchHit {
                path: format!("/tmp/{i}"),
                display_name: format!("f{i}"),
                content_type: "public.item".into(),
                is_app: false,
            })
            .collect();
        assert_eq!(rank_hits(hits, "f").len(), MAX_HITS);
    }
}
