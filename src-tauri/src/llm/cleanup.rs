//! Tidies what a model returns before it is typed. Some models leave markup
//! that is not part of the answer: GPT models wrap citations in 【…】 and
//! sometimes emit an empty, inverted 】【 pair, typically right before the
//! final full stop.

const OPEN: char = '【';
const CLOSE: char = '】';

/// Removes 【 and 】 that have no partner, and empty 【】 pairs, unless the
/// transcript itself contained these brackets. Matched labels such as
/// 【重要】【紧急】 are kept. Whitespace around the answer is trimmed.
pub fn clean_polished_text(text: &str, transcript: &str) -> String {
    if transcript.contains([OPEN, CLOSE]) {
        return text.trim().to_string();
    }
    drop_stray_brackets(text).trim().to_string()
}

fn drop_stray_brackets(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut keep = vec![true; chars.len()];
    let mut open = Vec::new();
    for (index, &c) in chars.iter().enumerate() {
        if c == OPEN {
            open.push(index);
        } else if c == CLOSE {
            match open.pop() {
                // An empty pair holds nothing worth keeping.
                Some(start) if start + 1 == index => {
                    keep[start] = false;
                    keep[index] = false;
                }
                Some(_) => {}
                None => keep[index] = false,
            }
        }
    }
    for start in open {
        keep[start] = false;
    }
    chars
        .into_iter()
        .zip(keep)
        .filter_map(|(c, keep)| keep.then_some(c))
        .collect()
}

/// A replacement for selected text keeps the selection's own leading and
/// trailing whitespace, such as the line break that ends a selected paragraph.
pub fn with_whitespace_of(selection: &str, replacement: &str) -> String {
    let trimmed_start = selection.trim_start();
    let leading = &selection[..selection.len() - trimmed_start.len()];
    let trailing = &trimmed_start[trimmed_start.trim_end().len()..];
    format!("{leading}{}{trailing}", replacement.trim())
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
    fn keeps_adjacent_labels_and_real_headings() {
        assert_eq!(
            clean_polished_text("【重要】【紧急】周五前处理合同", "重要紧急周五前处理合同"),
            "【重要】【紧急】周五前处理合同"
        );
        assert_eq!(
            clean_polished_text("【工作】开会\n【生活】买菜【】", "工作开会，生活买菜"),
            "【工作】开会\n【生活】买菜"
        );
        assert_eq!(clean_polished_text("【【嵌套】】", "嵌套"), "【【嵌套】】");
    }

    #[test]
    fn drops_brackets_left_open_or_never_opened() {
        assert_eq!(
            clean_polished_text("【未闭合的标签", "未闭合的标签"),
            "未闭合的标签"
        );
        assert_eq!(
            clean_polished_text("多出来的】括号", "多出来的括号"),
            "多出来的括号"
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

    #[test]
    fn a_selection_replacement_keeps_the_selections_whitespace() {
        assert_eq!(
            with_whitespace_of("旧的段落。\n", "新的段落。"),
            "新的段落。\n"
        );
        assert_eq!(
            with_whitespace_of("  缩进\n\n", "\n改好了\n"),
            "  改好了\n\n"
        );
        assert_eq!(with_whitespace_of("没有空白", " 替换 "), "替换");
        assert_eq!(with_whitespace_of("   ", "全是空白"), "   全是空白");
    }
}
