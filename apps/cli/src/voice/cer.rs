//! 字错率：`(替换 + 删除 + 插入) / 参考答案字数`，**按字符**算。
//!
//! 不能复用 Core 里那个 `edit_distance` —— 它是按字节的（注释写死了「键串都是 ASCII」），
//! 中文按字节算会把一个汉字算成三个，CER 直接失真。
//!
//! **比之前先归一化**：只留字母与数字、英文转小写，标点与空白不计入。
//! 模型（尤其 Whisper）会自己补标点和大小写，而参考转写常常是「全大写、无标点」——
//! 不归一化的话，一个逐字全对的句子也能算出很高的 CER，那就把「识别准不准」和
//! 「标点对不对」两件事混成一把尺子了。

/// 返回 `(编辑距离, 归一化后的参考答案字符数)`。
pub fn cer(reference: &str, hypothesis: &str) -> (usize, usize) {
    let reference = normalize(reference);
    let hypothesis = normalize(hypothesis);
    (distance(&reference, &hypothesis), reference.len())
}

/// 只留字母与数字（汉字属于字母），英文转小写；标点、空白、符号全丢掉。
fn normalize(text: &str) -> Vec<char> {
    text.chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(|c| c.to_lowercase())
        .collect()
}

/// Levenshtein 距离，按字符。滚动两行。
fn distance(reference: &[char], hypothesis: &[char]) -> usize {
    if reference.is_empty() {
        return hypothesis.len();
    }
    if hypothesis.is_empty() {
        return reference.len();
    }
    let mut previous: Vec<usize> = (0..=hypothesis.len()).collect();
    let mut current = vec![0usize; hypothesis.len() + 1];
    for (i, expected) in reference.iter().enumerate() {
        current[0] = i + 1;
        for (j, actual) in hypothesis.iter().enumerate() {
            let substitute = previous[j] + usize::from(expected != actual);
            current[j + 1] = substitute.min(previous[j + 1] + 1).min(current[j] + 1);
        }
        std::mem::swap(&mut previous, &mut current);
    }
    previous[hypothesis.len()]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn case_and_punctuation_do_not_count() {
        // 参考是全大写、无标点，模型补了大小写与标点：那是另一把尺子，不该算进 CER
        let (distance, _) = cer("AFTER EARLY NIGHTFALL", "After early nightfall.");
        assert_eq!(distance, 0);
    }

    #[test]
    fn a_chinese_substitution_costs_one() {
        assert_eq!(cer("天气不错", "天气不好"), (1, 4));
    }

    #[test]
    fn an_empty_hypothesis_costs_the_whole_reference() {
        assert_eq!(cer("天气不错", ""), (4, 4));
    }
}
