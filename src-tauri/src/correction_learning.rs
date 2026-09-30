//! Learns correction rules from small edits the user makes to dictated text before sending it.

use serde::Serialize;
use std::ops::Range;

const MAX_EDIT_CHARS: usize = 12;
/// Each edited phrase becomes its own rule; more than this in one pass reads as a rewrite.
const MAX_CORRECTIONS: usize = 8;
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

/// Compares the dictated text with how it reads now. Every replaced phrase becomes its own
/// correction; rewrites, deletions and added content do not.
#[cfg(test)]
fn detect_corrections(baseline: &str, inserted: &str, edited: &str) -> Vec<Correction> {
    detect_with_region(baseline, inserted, edited).0
}

/// Like [`detect_corrections`], also returning how the dictated part reads in `edited`, so a
/// later pass only counts edits made after this one.
fn detect_with_region(
    baseline: &str,
    inserted: &str,
    edited: &str,
) -> (Vec<Correction>, Option<String>) {
    let Some(start) = baseline.rfind(inserted) else {
        return (Vec::new(), None);
    };
    let (lead, tail) = (&baseline[..start], &baseline[start + inserted.len()..]);
    // Only the dictated part is compared; the text around it must be untouched.
    let Some(region) = edited
        .strip_prefix(lead)
        .and_then(|rest| rest.strip_suffix(tail))
    else {
        return (Vec::new(), None);
    };
    let region_text = Some(region.to_string());
    let old: Vec<char> = inserted.chars().collect();
    let new: Vec<char> = region.chars().collect();
    if old == new || old.len() > MAX_DIFF_CHARS || new.len() > MAX_DIFF_CHARS {
        return (Vec::new(), region_text);
    }

    let hunks = diff_hunks(&old, &new);
    let changed: usize = hunks.iter().map(|h| h.old.len()).sum();
    // Most of the sentence replaced: a rewrite, not a set of corrections.
    if changed * 10 > old.len().max(4) * 7 {
        return (Vec::new(), region_text);
    }

    let mut hunks = merge_same_word_hunks(hunks, &old, &new);
    widen_hunks(&mut hunks, &old, &new);
    let replacements: Vec<&Hunk> = hunks
        .iter()
        .filter(|h| !h.old.is_empty() && !h.new.is_empty())
        .collect();
    if replacements.len() > MAX_CORRECTIONS {
        return (Vec::new(), region_text);
    }

    let mut corrections = Vec::new();
    for hunk in replacements {
        let pattern: String = old[hunk.old.clone()].iter().collect();
        let replacement: String = new[hunk.new.clone()].iter().collect();
        let (pattern, replacement) = (pattern.trim(), replacement.trim());
        if pattern.chars().count() > MAX_EDIT_CHARS || replacement.chars().count() > MAX_EDIT_CHARS
        {
            return (Vec::new(), region_text);
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
    (corrections, region_text)
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

/// Normalize browser text serialization without merging words or collapsing paragraphs.
fn normalize_text(text: &str) -> String {
    text.replace("\r\n", "\n")
        .replace('\r', "\n")
        .replace('\u{00a0}', " ")
        .trim_end_matches('\n')
        .to_string()
}

pub use platform::{prepare_frontmost_app, stop_watching, watch_after_insert};

#[cfg(target_os = "macos")]
mod platform {
    use super::{detect_with_region, normalize_text, Correction, LearnedCorrection};
    use crate::storage::DictionaryStore;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{Duration, Instant};
    use tauri::{Emitter, Manager};

    const SETTLE_AFTER_INSERT: Duration = Duration::from_millis(400);
    const POLL_INTERVAL: Duration = Duration::from_millis(100);
    /// How long the text must stay unchanged before it counts as a finished edit.
    const EDIT_SETTLE: Duration = Duration::from_secs(3);
    /// Stops following a field that nobody has touched for this long.
    const WATCH_IDLE_LIMIT: Duration = Duration::from_secs(300);
    const MAX_FIELD_CHARS: usize = 20_000;
    const FOCUS_ATTEMPTS: usize = 8;
    const FOCUS_RETRY_INTERVAL: Duration = Duration::from_millis(250);

    static WATCH_GENERATION: AtomicU64 = AtomicU64::new(0);

    pub fn stop_watching() {
        WATCH_GENERATION.fetch_add(1, Ordering::SeqCst);
    }

    /// Chromium browsers and Electron apps only build their accessibility tree once an
    /// assistive client asks for it, so the focused text field is invisible until then.
    /// Asking as recording starts gives the tree time to appear before the text lands.
    pub fn prepare_frontmost_app() {
        std::thread::spawn(ax::enable_frontmost_app_accessibility);
    }

    /// Follows the focused text field after dictated text lands in it, and learns
    /// from the user's edits once the field is sent (cleared) or closed.
    pub fn watch_after_insert(app: tauri::AppHandle, inserted: String) {
        let generation = WATCH_GENERATION.fetch_add(1, Ordering::SeqCst) + 1;
        std::thread::spawn(move || {
            let active = || WATCH_GENERATION.load(Ordering::SeqCst) == generation;
            std::thread::sleep(SETTLE_AFTER_INSERT);
            if !active() {
                return;
            }
            let inserted = normalize_text(&inserted);
            if inserted.trim().is_empty() {
                return;
            }
            // Resolve the app independently of AX: Chromium may not have an AX tree yet.
            let Some(target) = ax::FrontmostApp::get() else {
                tracing::info!("Correction watch skipped: no frontmost app (bundle unknown)");
                return;
            };
            target.enable_accessibility();
            let mut matched = None;
            for attempt in 0..FOCUS_ATTEMPTS {
                if attempt > 0 {
                    std::thread::sleep(FOCUS_RETRY_INTERVAL);
                }
                if !active() {
                    return;
                }
                // Retry text readiness too, not just the existence of a focus object.
                match target.focused_field(&inserted) {
                    Ok(found) => {
                        matched = Some(found);
                        break;
                    }
                    Err(reason) if attempt + 1 == FOCUS_ATTEMPTS => {
                        tracing::info!(
                            "Correction watch skipped: {reason} (pid {}, bundle {})",
                            target.pid,
                            target.bundle_id
                        );
                    }
                    Err(_) => {}
                }
            }
            let Some((field, baseline)) = matched else {
                return;
            };
            let role = field.string_attribute("AXRole").unwrap_or_default();
            tracing::info!(
                "Correction watch started on {role} (pid {}, bundle {})",
                target.pid,
                target.bundle_id
            );

            // Learn as the user edits: once the field has stayed unchanged for a while, compare
            // it with the last learned state, then carry on until it is sent or closed.
            let mut baseline = baseline;
            let mut inserted = inserted;
            let mut last_text = baseline.clone();
            let mut evaluated_text = baseline.clone();
            let mut last_change = Instant::now();
            let mut learned_total = 0usize;
            let outcome = loop {
                std::thread::sleep(POLL_INTERVAL);
                if !active() {
                    // A new dictation started: settle what has been edited so far.
                    break "superseded";
                }
                if last_change.elapsed() >= WATCH_IDLE_LIMIT {
                    break "timed out";
                }
                match field.text().map(|value| normalize_text(&value)) {
                    Some(value) if value.trim().is_empty() => break "sent",
                    Some(value) if value.chars().count() > MAX_FIELD_CHARS => {
                        break "field too large"
                    }
                    Some(value) => {
                        if value != last_text {
                            last_text = value;
                            last_change = Instant::now();
                        } else if last_change.elapsed() >= EDIT_SETTLE
                            && last_text != evaluated_text
                        {
                            evaluated_text = last_text.clone();
                            learned_total +=
                                learn_from_edits(&app, &mut baseline, &mut inserted, &last_text);
                        }
                    }
                    None => break "closed",
                }
            };

            if last_text != evaluated_text {
                learned_total += learn_from_edits(&app, &mut baseline, &mut inserted, &last_text);
            }
            tracing::info!(
                "Correction watch ended ({outcome}); {learned_total} correction(s) found"
            );
        });
    }

    /// Learns from the edits made since the last pass and moves the baseline forward.
    /// Returns how many new rules were saved.
    fn learn_from_edits(
        app: &tauri::AppHandle,
        baseline: &mut String,
        inserted: &mut String,
        current: &str,
    ) -> usize {
        let (corrections, region) = detect_with_region(baseline, inserted, current);
        if corrections.is_empty() {
            return 0;
        }
        if let Some(region) = region {
            *baseline = current.to_string();
            *inserted = region;
        }
        save_and_announce(app, corrections)
    }

    fn save_and_announce(app: &tauri::AppHandle, corrections: Vec<Correction>) -> usize {
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
                Err(_) => tracing::warn!("Failed to save learned correction"),
            }
        }
        let count = learned.len();
        if !learned.is_empty() {
            tracing::info!("Learned {count} correction rule(s) from user edits");
            let _ = app.emit("dictionary:learned", learned);
        }
        count
    }

    mod ax {
        use super::{normalize_text, MAX_FIELD_CHARS};
        use objc2::{class, msg_send, rc::autoreleasepool, runtime::AnyObject};
        use std::collections::VecDeque;
        use std::ffi::c_void;

        type CFTypeRef = *const c_void;
        type AXUIElementRef = *const c_void;

        #[repr(C)]
        struct CFRange {
            location: isize,
            length: isize,
        }

        const K_AX_ERROR_SUCCESS: i32 = 0;
        const K_AX_VALUE_CF_RANGE_TYPE: u32 = 4;
        const K_CF_NUMBER_SINT64_TYPE: isize = 4;
        const K_CF_STRING_ENCODING_UTF8: u32 = 0x0800_0100;

        #[link(name = "ApplicationServices", kind = "framework")]
        extern "C" {
            fn AXUIElementCreateApplication(pid: i32) -> AXUIElementRef;
            fn AXUIElementCopyParameterizedAttributeValue(
                element: AXUIElementRef,
                attribute: CFTypeRef,
                parameter: CFTypeRef,
                value: *mut CFTypeRef,
            ) -> i32;
            fn AXValueCreate(value_type: u32, value: *const c_void) -> CFTypeRef;
            fn AXUIElementCopyAttributeValue(
                element: AXUIElementRef,
                attribute: CFTypeRef,
                value: *mut CFTypeRef,
            ) -> i32;
            fn AXUIElementGetTypeID() -> usize;
            fn AXUIElementSetAttributeValue(
                element: AXUIElementRef,
                attribute: CFTypeRef,
                value: CFTypeRef,
            ) -> i32;
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
            fn CFAttributedStringGetTypeID() -> usize;
            fn CFAttributedStringGetString(value: CFTypeRef) -> CFTypeRef;
            fn CFNumberGetTypeID() -> usize;
            fn CFNumberGetValue(number: CFTypeRef, number_type: isize, value: *mut c_void) -> u8;
            fn CFArrayGetTypeID() -> usize;
            fn CFArrayGetCount(array: CFTypeRef) -> isize;
            fn CFArrayGetValueAtIndex(array: CFTypeRef, index: isize) -> CFTypeRef;
            fn CFRetain(value: CFTypeRef) -> CFTypeRef;
            fn CFGetTypeID(cf: CFTypeRef) -> usize;
            fn CFStringGetLength(string: CFTypeRef) -> isize;
            fn CFStringGetCharacters(string: CFTypeRef, range: CFRange, buffer: *mut u16);
            fn CFRelease(cf: CFTypeRef);
            static kCFBooleanTrue: CFTypeRef;
        }

        /// An owned AXUIElement; AX calls on it are safe from any thread.
        pub struct Element(AXUIElementRef);

        unsafe impl Send for Element {}

        impl Drop for Element {
            fn drop(&mut self) {
                unsafe { CFRelease(self.0) }
            }
        }

        pub struct FrontmostApp {
            element: Element,
            pub pid: i32,
            pub bundle_id: String,
        }

        impl FrontmostApp {
            pub fn get() -> Option<Self> {
                autoreleasepool(|_| unsafe {
                    let workspace: *mut AnyObject = msg_send![class!(NSWorkspace), sharedWorkspace];
                    let app: *mut AnyObject = msg_send![workspace, frontmostApplication];
                    if app.is_null() {
                        return None;
                    }
                    let pid: i32 = msg_send![app, processIdentifier];
                    if pid <= 0 {
                        return None;
                    }
                    let bundle: *mut AnyObject = msg_send![app, bundleIdentifier];
                    let bundle_id = if bundle.is_null() {
                        "unknown".to_string()
                    } else {
                        read_string(bundle.cast()).unwrap_or_else(|| "unknown".to_string())
                    };
                    let element = AXUIElementCreateApplication(pid);
                    if element.is_null() {
                        return None;
                    }
                    Some(Self {
                        element: Element(element),
                        pid,
                        bundle_id,
                    })
                })
            }

            /// Asks the app to build its accessibility tree. `AXManualAccessibility` (Electron,
            /// Chromium) has no side effects; `AXEnhancedUserInterface` also works in older
            /// Chromium builds but slows window animations and window managers, so it is only
            /// used when the app does not support the first one.
            pub fn enable_accessibility(&self) {
                const K_AX_ERROR_ATTRIBUTE_UNSUPPORTED: i32 = -25205;
                unsafe {
                    for name in ["AXManualAccessibility", "AXEnhancedUserInterface"] {
                        let attribute = cf_string(name);
                        if attribute.is_null() {
                            continue;
                        }
                        let code =
                            AXUIElementSetAttributeValue(self.element.0, attribute, kCFBooleanTrue);
                        CFRelease(attribute);
                        tracing::debug!(
                            "Correction AX enable {name}: AX error {code} (pid {}, bundle {})",
                            self.pid,
                            self.bundle_id
                        );
                        if code != K_AX_ERROR_ATTRIBUTE_UNSUPPORTED {
                            break;
                        }
                    }
                }
            }

            pub fn focused_field(&self, inserted: &str) -> Result<(Element, String), String> {
                let focused = self
                    .element
                    .element_attribute("AXFocusedUIElement")
                    .map_err(|code| format!("no focused element (AX error {code})"))?;
                let role = focused.string_attribute("AXRole").unwrap_or_default();
                let mut reason = format!("focused {role} exposes no matching text");
                let mut queue = VecDeque::from([(focused, 0)]);
                let mut visited = 0;
                // ponytail: bounded focused-subtree search; add targeted traversal if real
                // editors exceed 128 nodes / 8 levels. Never search unrelated windows.
                while let Some((element, depth)) = queue.pop_front() {
                    visited += 1;
                    let role = element.string_attribute("AXRole").unwrap_or_default();
                    let subrole = element.string_attribute("AXSubrole").unwrap_or_default();
                    if role == "AXSecureTextField" || subrole == "AXSecureTextField" {
                        continue;
                    }
                    if matches!(role.as_str(), "AXTextArea" | "AXTextField" | "AXWebArea") {
                        if let Some(text) = element.text() {
                            let count = text.chars().count();
                            let text = normalize_text(&text);
                            if count <= MAX_FIELD_CHARS && text.contains(inserted) {
                                return Ok((element, text));
                            }
                            reason = format!(
                                "{role} text ({count} chars) exceeds limit or does not contain dictated text ({} chars)",
                                inserted.chars().count()
                            );
                        }
                    }
                    if visited >= 128 {
                        break;
                    }
                    if depth < 8 {
                        for child in element.children(128 - visited - queue.len()) {
                            queue.push_back((child, depth + 1));
                        }
                    }
                }
                Err(reason)
            }
        }

        pub fn enable_frontmost_app_accessibility() {
            if let Some(app) = FrontmostApp::get() {
                app.enable_accessibility();
            }
        }

        impl Element {
            fn element_attribute(&self, name: &str) -> Result<Element, i32> {
                unsafe {
                    let value = copy_attribute(self.0, name)?;
                    if CFGetTypeID(value) == AXUIElementGetTypeID() {
                        Ok(Element(value))
                    } else {
                        CFRelease(value);
                        Err(-1)
                    }
                }
            }

            fn children(&self, limit: usize) -> Vec<Element> {
                unsafe {
                    let Ok(array) = copy_attribute(self.0, "AXChildren") else {
                        return Vec::new();
                    };
                    let mut children = Vec::new();
                    if CFGetTypeID(array) == CFArrayGetTypeID() {
                        for index in 0..CFArrayGetCount(array).min(limit as isize) {
                            let child = CFArrayGetValueAtIndex(array, index);
                            if !child.is_null() && CFGetTypeID(child) == AXUIElementGetTypeID() {
                                children.push(Element(CFRetain(child)));
                            }
                        }
                    }
                    CFRelease(array);
                    children
                }
            }

            pub fn string_attribute(&self, name: &str) -> Option<String> {
                unsafe {
                    let value = copy_attribute(self.0, name).ok()?;
                    let text = read_string(value);
                    CFRelease(value);
                    text
                }
            }

            pub fn text(&self) -> Option<String> {
                let value = self.string_attribute("AXValue");
                if value.as_ref().is_some_and(|text| !text.is_empty()) {
                    return value;
                }
                // A selection is only a fragment (often a zero-length caret). Read the
                // entire UTF-16 range so moving the caret cannot look like a text edit.
                self.range_text().or(value)
            }

            fn range_text(&self) -> Option<String> {
                unsafe {
                    let number = copy_attribute(self.0, "AXNumberOfCharacters").ok()?;
                    let mut length = 0i64;
                    let valid = CFGetTypeID(number) == CFNumberGetTypeID()
                        && CFNumberGetValue(
                            number,
                            K_CF_NUMBER_SINT64_TYPE,
                            (&mut length as *mut i64).cast(),
                        ) != 0;
                    CFRelease(number);
                    // AX ranges use UTF-16 code units, including surrogate pairs.
                    if !valid || !(0..=(MAX_FIELD_CHARS * 2) as i64).contains(&length) {
                        return None;
                    }
                    let range = CFRange {
                        location: 0,
                        length: length as isize,
                    };
                    let parameter =
                        AXValueCreate(K_AX_VALUE_CF_RANGE_TYPE, (&range as *const CFRange).cast());
                    if parameter.is_null() {
                        return None;
                    }
                    let attribute = cf_string("AXStringForRange");
                    if attribute.is_null() {
                        CFRelease(parameter);
                        return None;
                    }
                    let mut value = std::ptr::null();
                    let status = AXUIElementCopyParameterizedAttributeValue(
                        self.0, attribute, parameter, &mut value,
                    );
                    CFRelease(attribute);
                    CFRelease(parameter);
                    let text = if status == K_AX_ERROR_SUCCESS && !value.is_null() {
                        read_string(value)
                    } else {
                        None
                    };
                    if !value.is_null() {
                        CFRelease(value);
                    }
                    text
                }
            }
        }

        unsafe fn read_string(value: CFTypeRef) -> Option<String> {
            let string = if CFGetTypeID(value) == CFAttributedStringGetTypeID() {
                CFAttributedStringGetString(value)
            } else if CFGetTypeID(value) == CFStringGetTypeID() {
                value
            } else {
                return None;
            };
            let length = CFStringGetLength(string);
            if !(0..=(MAX_FIELD_CHARS * 2) as isize).contains(&length) {
                return None;
            }
            let mut buffer = vec![0u16; length as usize];
            CFStringGetCharacters(
                string,
                CFRange {
                    location: 0,
                    length,
                },
                buffer.as_mut_ptr(),
            );
            Some(String::from_utf16_lossy(&buffer))
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

        #[cfg(test)]
        mod tests {
            use super::*;

            #[link(name = "CoreFoundation", kind = "framework")]
            extern "C" {
                fn CFAttributedStringCreate(
                    allocator: *const c_void,
                    text: CFTypeRef,
                    attributes: CFTypeRef,
                ) -> CFTypeRef;
            }

            #[test]
            fn reads_plain_and_attributed_utf16_text_and_rejects_non_text() {
                unsafe {
                    let text = "中文🙂\nAsk Cloud";
                    let plain = cf_string(text);
                    assert!(!plain.is_null());
                    let attributed =
                        CFAttributedStringCreate(std::ptr::null(), plain, std::ptr::null());
                    assert!(!attributed.is_null());
                    assert_eq!(read_string(plain).as_deref(), Some(text));
                    assert_eq!(read_string(attributed).as_deref(), Some(text));
                    assert_eq!(read_string(kCFBooleanTrue), None);
                    CFRelease(attributed);
                    CFRelease(plain);
                    let oversized = cf_string(&"x".repeat(MAX_FIELD_CHARS * 2 + 1));
                    assert_eq!(read_string(oversized), None);
                    CFRelease(oversized);
                }
            }
        }
    }
}

#[cfg(not(target_os = "macos"))]
mod platform {
    pub fn stop_watching() {}

    pub fn watch_after_insert(_app: tauri::AppHandle, _inserted: String) {}

    pub fn prepare_frontmost_app() {}
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
    fn normalizes_browser_serialization_without_collapsing_content() {
        assert_eq!(normalize_text("A\u{a0}B\r\nC\rD\n\n"), "A B\nC\nD");
        assert_eq!(normalize_text("  A  B\tC\n\nD "), "  A  B\tC\n\nD ");
        assert_eq!(normalize_text("中文🙂 e\u{301}"), "中文🙂 e\u{301}");
        assert_eq!(normalize_text("\r\n"), "");
    }

    #[test]
    fn learns_with_normalized_baseline_insert_and_poll_text() {
        let inserted = "Ask Cloud\r\nto review it\n";
        let baseline = "前缀：Ask\u{a0}Cloud\nto review it\n\n";
        let edited = "前缀：Ask Claude\r\nto review it\n";
        assert!(!baseline.contains(inserted)); // The old watch rejected this field.
        assert_eq!(
            learn(
                &normalize_text(baseline),
                &normalize_text(inserted),
                &normalize_text(edited)
            ),
            vec![pair("Cloud", "Claude")]
        );
        assert!(learn(
            &normalize_text(baseline),
            &normalize_text(inserted),
            &normalize_text("前缀：Ask Cloud\r\nto review it\n")
        )
        .is_empty());
    }

    #[test]
    fn normalized_region_still_rejects_edits_outside_dictation() {
        let baseline = normalize_text("Hello Tom. Ask\u{a0}Cloud to review it\n");
        let inserted = normalize_text("Ask Cloud to review it\r\n");
        let edited = normalize_text("Hello Tim. Ask Claude to review it\n");
        assert!(learn(&baseline, &inserted, &edited).is_empty());
        assert!(!normalize_text("some other field").contains(&inserted));
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
    fn learns_each_of_three_replacements_as_its_own_phrase() {
        let text = "请把同意、拓肯和克劳德这几个词都记下来";
        assert_eq!(
            learn(text, text, "请把通义、Token和Claude这几个词都记下来"),
            vec![
                pair("同意", "通义"),
                pair("拓肯", "Token"),
                pair("克劳德", "Claude")
            ]
        );
    }

    #[test]
    fn tracks_the_edited_region_so_later_passes_only_see_new_edits() {
        let text = "我们用同意千问和拓肯来做识别";
        let (first, region) = detect_with_region(text, text, "我们用通义千问和拓肯来做识别");
        assert_eq!(first.len(), 1);
        let region = region.unwrap();
        let (second, _) = detect_with_region(
            "我们用通义千问和拓肯来做识别",
            &region,
            "我们用通义千问和Token来做识别",
        );
        assert_eq!(second.len(), 1);
        assert_eq!(
            (second[0].pattern.as_str(), second[0].replacement.as_str()),
            ("拓肯", "Token")
        );
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
