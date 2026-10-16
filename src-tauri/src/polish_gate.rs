//! Decides on the device, without a model call, whether a dictation needs AI
//! polish. Short dictations that came out clean are typed as recognized: the
//! polish round trip takes about 2 seconds and rarely changes them beyond
//! punctuation. Anything with a sign of spoken mess goes to the model.

/// Why a dictation still goes to the model.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PolishReason {
    TooLong,
    Filler,
    Repetition,
    SelfCorrection,
    ListItems,
    SeveralSentences,
    CorrectionRule,
}

/// Hesitation sounds and verbal fillers that polish removes.
const FILLERS: &[&str] = &["嗯", "呃", "唔", "那个", "就是", "然后然后"];
/// Fillers only when they open a clause; elsewhere they are particles ("好啊").
const CLAUSE_START_FILLERS: &[char] = &['啊', '额'];
/// Sounds that, on their own, are a deliberate answer rather than a filler.
const STANDALONE_SOUNDS: &[char] = &['嗯', '呃', '唔', '啊', '额', '哦'];
const ENGLISH_FILLERS: &[&str] = &["um", "umm", "uh", "uhh", "erm", "hmm"];
/// The speaker taking something back; polish keeps only the final version.
const SELF_CORRECTIONS: &[&str] = &[
    "不对",
    "不是不是",
    "我是说",
    "我的意思是",
    "说错了",
    "重新说",
    "算了",
    "哦不",
    "改成",
    "i mean",
    "scratch that",
    "no wait",
];
/// Enumeration words the structured style turns into a list.
const LIST_MARKERS: &[&str] = &[
    "第一",
    "第二",
    "首先",
    "其次",
    "一是",
    "二是",
    "另外",
    "以及",
    "还有就是",
];
/// Repeated on purpose (laughter), not a stutter.
const LAUGHTER: &[char] = &['哈', '呵', '嘿'];
const SENTENCE_ENDS: &[char] = &['。', '？', '！', '?', '!'];

pub struct PolishGateInput<'a> {
    pub text: &'a str,
    /// Longest dictation, in characters without punctuation, that may skip polish.
    pub max_chars: usize,
    pub structured_style: bool,
    /// Phrases from the correction dictionary; the model applies them in context.
    pub correction_patterns: &'a [&'a str],
}

/// `None` means the dictation is clean enough to type as recognized.
pub fn polish_reason(input: &PolishGateInput<'_>) -> Option<PolishReason> {
    let text = input.text.trim();
    let content: Vec<char> = text.chars().filter(|c| c.is_alphanumeric()).collect();
    if content.len() > input.max_chars {
        return Some(PolishReason::TooLong);
    }
    let lower = text.to_lowercase();

    if has_filler(text, &content, &lower) {
        return Some(PolishReason::Filler);
    }
    if has_repetition(&content) {
        return Some(PolishReason::Repetition);
    }
    if SELF_CORRECTIONS.iter().any(|cue| lower.contains(cue)) {
        return Some(PolishReason::SelfCorrection);
    }
    if input.structured_style && LIST_MARKERS.iter().any(|marker| text.contains(marker)) {
        return Some(PolishReason::ListItems);
    }
    if has_several_sentences(text) {
        return Some(PolishReason::SeveralSentences);
    }
    if input
        .correction_patterns
        .iter()
        .any(|pattern| !pattern.trim().is_empty() && text.contains(pattern.trim()))
    {
        return Some(PolishReason::CorrectionRule);
    }
    None
}

fn has_filler(text: &str, content: &[char], lower: &str) -> bool {
    // "嗯" on its own is an answer, and polish would keep it anyway.
    if content.iter().all(|c| STANDALONE_SOUNDS.contains(c)) {
        return false;
    }
    if FILLERS.iter().any(|filler| text.contains(filler)) {
        return true;
    }
    let mut at_clause_start = true;
    for c in text.chars() {
        if at_clause_start && CLAUSE_START_FILLERS.contains(&c) {
            return true;
        }
        at_clause_start = !c.is_alphanumeric();
    }
    lower
        .split(|c: char| !c.is_ascii_alphabetic())
        .any(|word| ENGLISH_FILLERS.contains(&word))
}

/// A stutter: one character three times ("我我我") or a two-character word
/// said twice in a row ("我们我们"). Letters and digits are left alone.
fn has_repetition(content: &[char]) -> bool {
    let spoken = |c: &char| !c.is_ascii() && !LAUGHTER.contains(c);
    let tripled = content
        .windows(3)
        .any(|w| spoken(&w[0]) && w[0] == w[1] && w[1] == w[2]);
    let doubled_word = content
        .windows(4)
        .any(|w| spoken(&w[0]) && spoken(&w[1]) && w[0] != w[1] && w[0] == w[2] && w[1] == w[3]);
    tripled || doubled_word
}

/// More than one sentence means pauses split the speech into fragments that
/// polish joins back together. A closing mark at the very end does not count.
fn has_several_sentences(text: &str) -> bool {
    let body = text.trim_end_matches(|c: char| SENTENCE_ENDS.contains(&c) || c.is_whitespace());
    let mut chars = body.chars().peekable();
    while let Some(c) = chars.next() {
        if SENTENCE_ENDS.contains(&c) {
            return true;
        }
        if c == '.' && chars.peek().is_some_and(|next| next.is_whitespace()) {
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reason(text: &str) -> Option<PolishReason> {
        polish_reason(&PolishGateInput {
            text,
            max_chars: 30,
            structured_style: false,
            correction_patterns: &[],
        })
    }

    #[test]
    fn clean_short_dictations_skip_polish() {
        for text in [
            "继续",
            "好的，没问题。",
            "帮我看一下这个报错",
            "明天下午三点开会？",
            "好啊",
            "嗯",
            "嗯嗯。",
            "哈哈哈，太好了",
            "版本号是 1.1.62",
            "Sounds good to me.",
        ] {
            assert_eq!(reason(text), None, "{text}");
        }
    }

    #[test]
    fn long_dictations_always_polish() {
        let text = "我想把这个功能再调整一下让它在出错的时候也能保留原来的文字不要丢掉";
        assert_eq!(reason(text), Some(PolishReason::TooLong));
    }

    #[test]
    fn spoken_mess_goes_to_the_model() {
        let cases = [
            ("嗯我想问一下", PolishReason::Filler),
            ("那个方案你看了吗", PolishReason::Filler),
            ("呃，好的", PolishReason::Filler),
            ("啊，我忘了", PolishReason::Filler),
            ("um let me check", PolishReason::Filler),
            ("我我我觉得可以", PolishReason::Repetition),
            ("我们我们先开始吧", PolishReason::Repetition),
            ("周三，不对，周四开会", PolishReason::SelfCorrection),
            ("I mean tomorrow", PolishReason::SelfCorrection),
            ("先这样。 然后再说", PolishReason::SeveralSentences),
            ("Done. Next one", PolishReason::SeveralSentences),
        ];
        for (text, expected) in cases {
            assert_eq!(reason(text), Some(expected), "{text}");
        }
    }

    #[test]
    fn list_markers_matter_only_for_the_structured_style() {
        let text = "首先改代码，其次写测试";
        assert_eq!(reason(text), None);
        let structured = polish_reason(&PolishGateInput {
            text,
            max_chars: 30,
            structured_style: true,
            correction_patterns: &[],
        });
        assert_eq!(structured, Some(PolishReason::ListItems));
    }

    #[test]
    fn a_dictionary_phrase_is_left_to_the_model() {
        let input = |text| PolishGateInput {
            text,
            max_chars: 30,
            structured_style: false,
            correction_patterns: &["同意", " "],
        };
        assert_eq!(
            polish_reason(&input("我同意这个说法")),
            Some(PolishReason::CorrectionRule)
        );
        assert_eq!(polish_reason(&input("我赞成这个说法")), None);
    }

    #[test]
    fn the_length_limit_counts_characters_without_punctuation() {
        let input = |text, max_chars| PolishGateInput {
            text,
            max_chars,
            structured_style: false,
            correction_patterns: &[],
        };
        assert_eq!(polish_reason(&input("好的，谢谢！", 4)), None);
        assert_eq!(
            polish_reason(&input("好的，谢谢你！", 4)),
            Some(PolishReason::TooLong)
        );
    }
}
