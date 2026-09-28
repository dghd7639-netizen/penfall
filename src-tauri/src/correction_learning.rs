//! Learns correction rules from small edits the user makes to dictated text before sending it.

use serde::Serialize;
use std::ops::Range;

const MAX_EDIT_CHARS: usize = 12;
const MAX_CORRECTIONS: usize = 2;
const MAX_DIFF_CHARS: usize = 1_000;

#[derive(Debug, Clone, PartialEq, Eq)]
struct Correction {
    pattern: String,
    replacement: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct LearnedCorrection {
    pub id: i64,
    pub pattern: String,
    pub replacement: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Hunk {
    old: Range<usize>,
    new: Range<usize>,
}

/// Compares the dictated text with how it read just before the field was sent.
/// Up to two replacements of a couple of words count; rewrites, deletions and
/// added content do not.
fn detect_corrections(baseline: &str, inserted: &str, edited: &str) -> Vec<Correction> {
    let Some(start) = baseline.rfind(inserted) else {
        return Vec::new();
    };
    let (lead, tail) = (&baseline[..start], &baseline[start + inserted.len()..]);
    // Only the dictated part is compared; the text around it must be untouched.
    let Some(region) = edited
        .strip_prefix(lead)
        .and_then(|rest| rest.strip_suffix(tail))
    else {
        return Vec::new();
    };
    let old: Vec<char> = inserted.chars().collect();
    let new: Vec<char> = region.chars().collect();
    if old == new || old.len() > MAX_DIFF_CHARS || new.len() > MAX_DIFF_CHARS {
        return Vec::new();
    }

    let hunks = diff_hunks(&old, &new);
    let changed: usize = hunks.iter().map(|h| h.old.len()).sum();
    if changed * 2 > old.len().max(4) {
        return Vec::new();
    }

    let mut hunks = merge_same_word_hunks(hunks, &old, &new);
    widen_hunks(&mut hunks, &old, &new);
    let replacements: Vec<&Hunk> = hunks
        .iter()
        .filter(|h| !h.old.is_empty() && !h.new.is_empty())
        .collect();
    if replacements.len() > MAX_CORRECTIONS {
        return Vec::new();
    }

    let mut corrections = Vec::new();
    for hunk in replacements {
        let pattern: String = old[hunk.old.clone()].iter().collect();
        let replacement: String = new[hunk.new.clone()].iter().collect();
        let (pattern, replacement) = (pattern.trim(), replacement.trim());
        if pattern.chars().count() > MAX_EDIT_CHARS || replacement.chars().count() > MAX_EDIT_CHARS
        {
            return Vec::new();
        }
        let has_content = |s: &str| s.chars().any(char::is_alphanumeric);
        if pattern == replacement
            || !has_content(pattern)
            || !has_content(replacement)
            || pattern.contains('\n')
            || replacement.contains('\n')
            || looks_like_ime_composition(pattern, replacement)
        {
            continue;
        }
        corrections.push(Correction {
            pattern: pattern.to_string(),
            replacement: replacement.to_string(),
        });
    }
    corrections
}

/// Character-level LCS diff, returned as the changed ranges.
fn diff_hunks(old: &[char], new: &[char]) -> Vec<Hunk> {
    let (n, m) = (old.len(), new.len());
    let mut lcs = vec![vec![0u16; m + 1]; n + 1];
    for i in (0..n).rev() {
        for j in (0..m).rev() {
            lcs[i][j] = if old[i] == new[j] {
                lcs[i + 1][j + 1] + 1
            } else {
                lcs[i + 1][j].max(lcs[i][j + 1])
            };
        }
    }

    let mut hunks = Vec::new();
    let mut open: Option<(usize, usize)> = None;
    let (mut i, mut j) = (0, 0);
    while i < n || j < m {
        if i < n && j < m && old[i] == new[j] {
            if let Some((si, sj)) = open.take() {
                hunks.push(Hunk {
                    old: si..i,
                    new: sj..j,
                });
            }
            i += 1;
            j += 1;
            continue;
        }
        open.get_or_insert((i, j));
        if j < m && (i == n || lcs[i][j + 1] >= lcs[i + 1][j]) {
            j += 1;
        } else {
            i += 1;
        }
    }
    if let Some((si, sj)) = open {
        hunks.push(Hunk {
            old: si..n,
            new: sj..m,
        });
    }
    hunks
}

/// Joins hunks inside one Latin word that the two spellings partly share ("Cloud" -> "Claude").
fn merge_same_word_hunks(hunks: Vec<Hunk>, old: &[char], new: &[char]) -> Vec<Hunk> {
    let word_at = |o: Option<&char>, n: Option<&char>| {
        o.is_some_and(|c| is_word_char(*c)) || n.is_some_and(|c| is_word_char(*c))
    };
    let mut merged: Vec<Hunk> = Vec::new();
    for hunk in hunks {
        if let Some(last) = merged.last_mut() {
            let gap = &old[last.old.end..hunk.old.start];
            let joins_word = !gap.is_empty()
                && gap.iter().all(|c| is_word_char(*c))
                && word_at(old[last.old.clone()].last(), new[last.new.clone()].last())
                && word_at(old[hunk.old.clone()].first(), new[hunk.new.clone()].first());
            if joins_word {
                last.old.end = hunk.old.end;
                last.new.end = hunk.new.end;
                continue;
            }
        }
        merged.push(hunk);
    }
    merged
}

/// Widens partial Latin words to the whole word, and a lone CJK character to
/// include a neighbour so the rule carries some context.
fn widen_hunks(hunks: &mut [Hunk], old: &[char], new: &[char]) {
    for index in 0..hunks.len() {
        let lower = if index == 0 {
            0
        } else {
            hunks[index - 1].old.end
        };
        let upper = hunks.get(index + 1).map_or(old.len(), |h| h.old.start);
        let hunk = &mut hunks[index];
        if hunk.old.is_empty() || hunk.new.is_empty() {
            continue;
        }
        let edge_is_word = |o: usize, n: usize| is_word_char(old[o]) || is_word_char(new[n]);
        if edge_is_word(hunk.old.start, hunk.new.start) {
            while hunk.old.start > lower && is_word_char(old[hunk.old.start - 1]) {
                hunk.old.start -= 1;
                hunk.new.start -= 1;
            }
        }
        if edge_is_word(hunk.old.end - 1, hunk.new.end - 1) {
            while hunk.old.end < upper && is_word_char(old[hunk.old.end]) {
                hunk.old.end += 1;
                hunk.new.end += 1;
            }
        }
        if hunk.old.len() == 1 && is_cjk(old[hunk.old.start]) {
            if hunk.old.start > lower && is_cjk(old[hunk.old.start - 1]) {
                hunk.old.start -= 1;
                hunk.new.start -= 1;
            } else if hunk.old.end < upper && is_cjk(old[hunk.old.end]) {
                hunk.old.end += 1;
                hunk.new.end += 1;
            }
        }
    }
}

fn is_word_char(c: char) -> bool {
    c.is_ascii_alphanumeric()
}

fn is_cjk(c: char) -> bool {
    matches!(c as u32, 0x3400..=0x4DBF | 0x4E00..=0x9FFF | 0xF900..=0xFAFF | 0x20000..=0x2FA1F)
}

/// Pinyin still being composed in an input method is not a finished correction.
fn looks_like_ime_composition(pattern: &str, replacement: &str) -> bool {
    pattern.chars().any(is_cjk)
        && replacement
            .chars()
            .all(|c| c.is_ascii_lowercase() || c == '\'' || c == ' ')
}

pub use platform::{stop_watching, watch_after_insert};

#[cfg(target_os = "macos")]
mod platform {
    use super::{detect_corrections, Correction, LearnedCorrection};
    use crate::storage::DictionaryStore;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{Duration, Instant};
    use tauri::{Emitter, Manager};

    const SETTLE_AFTER_INSERT: Duration = Duration::from_millis(400);
    const POLL_INTERVAL: Duration = Duration::from_millis(100);
    const WATCH_FOR: Duration = Duration::from_secs(300);
    const MAX_FIELD_CHARS: usize = 20_000;

    static WATCH_GENERATION: AtomicU64 = AtomicU64::new(0);

    pub fn stop_watching() {
        WATCH_GENERATION.fetch_add(1, Ordering::SeqCst);
    }

    /// Follows the focused text field after dictated text lands in it, and learns
    /// from the user's edits once the field is sent (cleared) or closed.
    pub fn watch_after_insert(app: tauri::AppHandle, inserted: String) {
        let generation = WATCH_GENERATION.fetch_add(1, Ordering::SeqCst) + 1;
        std::thread::spawn(move || {
            let active = || WATCH_GENERATION.load(Ordering::SeqCst) == generation;
            std::thread::sleep(SETTLE_AFTER_INSERT);
            let Some(field) = ax::focused_element() else {
                return;
            };
            let role = field.string_attribute("AXRole").unwrap_or_default();
            if role == "AXSecureTextField" {
                return;
            }
            let Some(baseline) = field.string_attribute("AXValue") else {
                tracing::info!("Correction watch skipped: focused {role} exposes no text");
                return;
            };
            if baseline.chars().count() > MAX_FIELD_CHARS || !baseline.contains(&inserted) {
                tracing::info!(
                    "Correction watch skipped: focused {role} text ({} chars) does not contain the dictated text ({} chars)",
                    baseline.chars().count(),
                    inserted.chars().count()
                );
                return;
            }
            tracing::info!("Correction watch started on {role}");

            let mut last_text = baseline.clone();
            let deadline = Instant::now() + WATCH_FOR;
            let outcome = loop {
                std::thread::sleep(POLL_INTERVAL);
                if !active() {
                    return;
                }
                if Instant::now() >= deadline {
                    tracing::info!("Correction watch timed out before the text was sent");
                    return;
                }
                match field.string_attribute("AXValue") {
                    Some(value) if value.trim().is_empty() => break "sent",
                    Some(value) => last_text = value,
                    None => break "closed",
                }
            };

            let corrections = detect_corrections(&baseline, &inserted, &last_text);
            tracing::info!(
                "Correction watch ended (field {outcome}); {} correction(s) found",
                corrections.len()
            );
            if !corrections.is_empty() {
                save_and_announce(&app, corrections);
            }
        });
    }

    fn save_and_announce(app: &tauri::AppHandle, corrections: Vec<Correction>) {
        let store = app.state::<DictionaryStore>();
        let mut learned = Vec::new();
        for correction in corrections {
            let saved = tauri::async_runtime::block_on(
                store.add_correction(&correction.pattern, &correction.replacement),
            );
            match saved {
                Ok(id) => learned.push(LearnedCorrection {
                    id,
                    pattern: correction.pattern,
                    replacement: correction.replacement,
                }),
                Err(error) if error.to_string() == "correction_duplicate" => {}
                Err(error) => tracing::warn!("Failed to save learned correction: {error}"),
            }
        }
        if !learned.is_empty() {
            tracing::info!(
                "Learned {} correction rule(s) from user edits",
                learned.len()
            );
            let _ = app.emit("dictionary:learned", learned);
        }
    }

    mod ax {
        use std::ffi::c_void;

        type CFTypeRef = *const c_void;
        type AXUIElementRef = *const c_void;

        #[repr(C)]
        struct CFRange {
            location: isize,
            length: isize,
        }

        const K_AX_ERROR_SUCCESS: i32 = 0;
        const K_CF_STRING_ENCODING_UTF8: u32 = 0x0800_0100;

        #[link(name = "ApplicationServices", kind = "framework")]
        extern "C" {
            fn AXUIElementCreateSystemWide() -> AXUIElementRef;
            fn AXUIElementCopyAttributeValue(
                element: AXUIElementRef,
                attribute: CFTypeRef,
                value: *mut CFTypeRef,
            ) -> i32;
            fn AXUIElementGetTypeID() -> usize;
        }

        #[link(name = "CoreFoundation", kind = "framework")]
        extern "C" {
            fn CFStringCreateWithBytes(
                allocator: *const c_void,
                bytes: *const u8,
                num_bytes: isize,
                encoding: u32,
                is_external_representation: u8,
            ) -> CFTypeRef;
            fn CFStringGetTypeID() -> usize;
            fn CFGetTypeID(cf: CFTypeRef) -> usize;
            fn CFStringGetLength(string: CFTypeRef) -> isize;
            fn CFStringGetCharacters(string: CFTypeRef, range: CFRange, buffer: *mut u16);
            fn CFRelease(cf: CFTypeRef);
        }

        /// An owned AXUIElement; AX calls on it are safe from any thread.
        pub struct Element(AXUIElementRef);

        unsafe impl Send for Element {}

        impl Drop for Element {
            fn drop(&mut self) {
                unsafe { CFRelease(self.0) }
            }
        }

        pub fn focused_element() -> Option<Element> {
            unsafe {
                let system = AXUIElementCreateSystemWide();
                if system.is_null() {
                    return None;
                }
                let focused = copy_attribute(system, "AXFocusedUIElement");
                CFRelease(system);
                let focused = match focused {
                    Ok(focused) => focused,
                    Err(code) => {
                        // WeChat 4 exposes no focus or text at all (AX error -25212).
                        tracing::info!(
                            "Correction watch skipped: no focused element (AX error {code})"
                        );
                        return None;
                    }
                };
                if CFGetTypeID(focused) != AXUIElementGetTypeID() {
                    CFRelease(focused);
                    return None;
                }
                Some(Element(focused))
            }
        }

        impl Element {
            pub fn string_attribute(&self, name: &str) -> Option<String> {
                unsafe {
                    let value = copy_attribute(self.0, name).ok()?;
                    if CFGetTypeID(value) != CFStringGetTypeID() {
                        CFRelease(value);
                        return None;
                    }
                    let length = CFStringGetLength(value);
                    let mut buffer = vec![0u16; length.max(0) as usize];
                    CFStringGetCharacters(
                        value,
                        CFRange {
                            location: 0,
                            length,
                        },
                        buffer.as_mut_ptr(),
                    );
                    CFRelease(value);
                    Some(String::from_utf16_lossy(&buffer))
                }
            }
        }

        unsafe fn cf_string(text: &str) -> CFTypeRef {
            CFStringCreateWithBytes(
                std::ptr::null(),
                text.as_ptr(),
                text.len() as isize,
                K_CF_STRING_ENCODING_UTF8,
                0,
            )
        }

        /// Returns an owned value, or the AX error code.
        unsafe fn copy_attribute(element: AXUIElementRef, name: &str) -> Result<CFTypeRef, i32> {
            let attribute = cf_string(name);
            if attribute.is_null() {
                return Err(-1);
            }
            let mut value: CFTypeRef = std::ptr::null();
            let status = AXUIElementCopyAttributeValue(element, attribute, &mut value);
            CFRelease(attribute);
            match status {
                K_AX_ERROR_SUCCESS if !value.is_null() => Ok(value),
                K_AX_ERROR_SUCCESS => Err(-1),
                code => Err(code),
            }
        }
    }
}

#[cfg(not(target_os = "macos"))]
mod platform {
    pub fn stop_watching() {}

    pub fn watch_after_insert(_app: tauri::AppHandle, _inserted: String) {}
}

#[cfg(test)]
mod tests {
    use super::*;

    fn learn(baseline: &str, inserted: &str, edited: &str) -> Vec<(String, String)> {
        detect_corrections(baseline, inserted, edited)
            .into_iter()
            .map(|c| (c.pattern, c.replacement))
            .collect()
    }

    fn pair(pattern: &str, replacement: &str) -> (String, String) {
        (pattern.to_string(), replacement.to_string())
    }

    #[test]
    fn learns_a_replaced_chinese_word() {
        let text = "我们用同意千问来做识别";
        assert_eq!(
            learn(text, text, "我们用通义千问来做识别"),
            vec![pair("同意", "通义")]
        );
    }

    #[test]
    fn learns_two_separate_replacements() {
        let text = "我们用同意千问和拓肯来做识别";
        assert_eq!(
            learn(text, text, "我们用通义千问和Token来做识别"),
            vec![pair("同意", "通义"), pair("拓肯", "Token")]
        );
    }

    #[test]
    fn ignores_three_or_more_replacements() {
        let text = "请把同意、拓肯和克劳德这几个词都记下来";
        assert!(learn(text, text, "请把通义、Token和Claude这几个词都记下来").is_empty());
    }

    #[test]
    fn widens_a_single_changed_cjk_character_to_include_a_neighbour() {
        let text = "我们用通一千问来做识别";
        assert_eq!(
            learn(text, text, "我们用通义千问来做识别"),
            vec![pair("通一", "通义")]
        );
    }

    #[test]
    fn widens_partial_latin_edits_to_the_whole_word() {
        let text = "Ask Cloud to review it";
        assert_eq!(
            learn(text, text, "Ask Claude to review it"),
            vec![pair("Cloud", "Claude")]
        );
    }

    #[test]
    fn does_not_pull_adjacent_latin_letters_into_a_cjk_edit() {
        let text = "用ABC同意千问";
        assert_eq!(
            learn(text, text, "用ABC通义千问"),
            vec![pair("同意", "通义")]
        );
    }

    #[test]
    fn only_counts_edits_inside_the_dictated_text() {
        let baseline = "Hello Tom. 我们用同意千问";
        let inserted = "我们用同意千问";
        assert!(learn(baseline, inserted, "Hello Tim. 我们用同意千问").is_empty());
        assert_eq!(
            learn(baseline, inserted, "Hello Tom. 我们用通义千问"),
            vec![pair("同意", "通义")]
        );
    }

    #[test]
    fn keeps_a_fix_when_new_content_is_also_added() {
        let text = "我们用同意千问来做识别";
        assert_eq!(
            learn(text, text, "我们用通义千问来做识别，明天再测一下"),
            vec![pair("同意", "通义")]
        );
    }

    #[test]
    fn ignores_rewrites_deletions_and_additions() {
        let text = "今天下午三点开会讨论一下项目进度";
        assert!(learn(text, text, "明天上午十点再约时间聊聊").is_empty());
        assert!(learn(text, text, "今天下午三点开会").is_empty());
        assert!(learn(text, text, "今天下午三点开会讨论一下项目进度，记得带电脑").is_empty());
    }

    #[test]
    fn ignores_pinyin_still_being_composed() {
        let text = "我们用同意千问来做识别";
        assert!(learn(text, text, "我们用tong'yi千问来做识别").is_empty());
    }

    #[test]
    fn ignores_punctuation_only_changes() {
        let text = "好的，明天见";
        assert!(learn(text, text, "好的。明天见").is_empty());
    }
}
