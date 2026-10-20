//! Tidies what a model returns before it is typed. Some models leave markup
//! that is not part of the answer: GPT models wrap citations in 【…】 and
//! sometimes emit an empty, inverted 】【 pair, typically right before the
//! final full stop.

/// Bracket pairs with nothing between them, which no answer needs.
const EMPTY_BRACKET_PAIRS: &[&str] = &["】【", "【】"];

/// Removes empty or inverted 【】 pairs the transcript did not contain, and
/// whitespace around the answer. Real 【标题】 headings are kept.
pub fn clean_polished_text(text: &str, transcript: &str) -> String {
    let mut cleaned = text.to_string();
    loop {
        let before = cleaned.len();
        for pair in EMPTY_BRACKET_PAIRS {
            if !transcript.contains(pair) {
                cleaned = cleaned.replace(pair, "");
            }
        }
        // Removing one pair can join the halves of another ("】【】【").
        if cleaned.len() == before {
            break;
        }
    }
    cleaned.trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drops_the_inverted_pair_left_before_the_final_stop() {
        assert_eq!(
            clean_polished_text("下周再确认一下预算】【。", "下周再确认一下预算。"),
            "下周再确认一下预算。"
        );
        assert_eq!(clean_polished_text("好的】【】【。", "好的。"), "好的。");
    }

    #[test]
    fn drops_empty_pairs_but_keeps_real_headings() {
        assert_eq!(
            clean_polished_text("【工作】开会\n【生活】买菜【】", "工作开会，生活买菜"),
            "【工作】开会\n【生活】买菜"
        );
    }

    #[test]
    fn keeps_brackets_the_speaker_dictated() {
        assert_eq!(
            clean_polished_text("这个符号是】【，别删", "这个符号是】【，别删"),
            "这个符号是】【，别删"
        );
    }

    #[test]
    fn trims_whitespace_around_the_answer() {
        assert_eq!(
            clean_polished_text("\n好的，我听\n", "好的我听"),
            "好的，我听"
        );
        assert_eq!(
            clean_polished_text("1. 第一\n2. 第二\n", "第一第二"),
            "1. 第一\n2. 第二"
        );
    }
}
