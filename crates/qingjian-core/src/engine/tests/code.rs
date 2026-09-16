//! 形码（五笔）：编码直接查码表，不走拼音的切分与整句。

use super::*;

/// 五笔 86 的一小段：一级简码 V 是 发，`ggll` 是 一。
const CODES: &str =
    "一\tggll\t90000\n发\tv\t30000\n到\tgc\t20000\n来\tgo\t15000\n开\tga\t5000\n开发\tgant\t900\n";

fn wubi() -> Engine {
    let mut engine = engine();
    engine.set_code_table(Some(CodeTable::parse(CODES).unwrap()));
    engine
}

fn code_texts(engine: &mut Engine, input: &str) -> Vec<String> {
    engine.set_input(input);
    texts_of(engine)
}

#[test]
fn looks_up_by_code_prefix_and_prefers_the_finished_code() {
    let mut engine = wubi();
    // 编码打全的 开 排在同前缀的 开发 前面
    assert_eq!(code_texts(&mut engine, "ga"), ["开", "开发"]);
    // 没打全时只按词频
    assert_eq!(
        code_texts(&mut engine, "g"),
        ["一", "到", "来", "开", "开发"]
    );
    // preedit 原样显示敲的编码，没有拼音切分
    engine.set_input("ga");
    let query = engine.query().unwrap();
    assert_eq!(query.marked_text(), "ga");
    assert_eq!(query.tail, "ga");
    assert!(query.segmentations.is_empty());
    assert!(!query.decoded_keys);
    assert!(
        query
            .candidates
            .items
            .iter()
            .all(|c| c.syllables.is_empty())
    );
}

#[test]
fn commits_the_whole_code_and_empties_the_buffer() {
    let mut engine = wubi();
    engine.set_input("ga");
    let kai = engine.query().unwrap().candidates.items[0].clone();
    assert_eq!(kai.kind, CandidateKind::Code);
    assert_eq!(engine.commit(&kai), "开");
    assert!(engine.composition().is_empty());
}

#[test]
fn takes_no_part_in_the_pinyin_machinery() {
    let mut engine = wubi();
    // `v` 是字根键，不当表达式模式的前缀
    assert_eq!(code_texts(&mut engine, "v"), ["发"]);
    // 快捷候选按拼音键认，形码下不出
    assert!(code_texts(&mut engine, "rq").is_empty());
    // 模糊音不参与：`ga` 不会命中并不存在的 `ka`
    engine.set_fuzzy(FuzzyRules::ALL);
    assert_eq!(code_texts(&mut engine, "ga"), ["开", "开发"]);
}

#[test]
fn a_word_chosen_under_a_code_rises_next_time() {
    let mut engine = wubi().with_learner(Box::new(WordLearner::default()));
    engine.set_input("g");
    let kaifa = engine
        .query()
        .unwrap()
        .candidates
        .items
        .last()
        .cloned()
        .unwrap();
    assert_eq!(kaifa.text, "开发");
    engine.commit(&kaifa);
    engine.set_input("g");
    assert_eq!(engine.query().unwrap().candidates.items[0].text, "开发");
}

#[test]
fn code_candidates_get_translations_like_pinyin_ones() {
    let mut engine = wubi().with_translator(Box::new(FixedTranslator));
    engine.set_input("gant");
    let mut query = engine.query().unwrap();
    engine.annotate(&mut query.candidates);
    let kaifa = query
        .candidates
        .items
        .iter()
        .find(|c| c.text == "开发")
        .unwrap();
    let senses = kaifa.translation.as_ref().unwrap().senses();
    assert_eq!(senses[0].text, "develop");
}

#[test]
fn code_commits_still_feed_translations_vocabulary_and_usage() {
    // 形码换的是「怎么按键出候选」，不是「上屏之后算什么」：释义标注、生词判定、词汇记录与输入统计
    // 都按上屏的词工作，与方案无关。这条用例把这句话钉住。
    let usage = Arc::new(Mutex::new(Vec::new()));
    let vocabulary = Arc::new(Mutex::new(VocabularyCounts::default()));
    let mut engine = wubi()
        .with_translator(Box::new(FixedTranslator))
        .with_usage_meter(Box::new(MemoryMeter(usage.clone())))
        .with_vocabulary_tracker(Box::new(MemoryVocabulary(vocabulary.clone())));

    engine.set_input("gant");
    let mut query = engine.query().unwrap();
    engine.annotate(&mut query.candidates);
    let kaifa = query.candidates.items[0].clone();
    assert_eq!(kaifa.text, "开发");
    // 释义照常标注；这条译词一个轮次都没见过，标成生词
    let sense = &kaifa.translation.as_ref().unwrap().senses()[0];
    assert_eq!(sense.text, "develop");
    assert!(sense.fresh);

    engine.commit(&kaifa);
    // 输入统计：一个中文词、两个汉字（形码一次上屏就是一个词）
    let usage = usage.lock().unwrap();
    assert_eq!(usage.len(), 1);
    assert_eq!((usage[0].words, usage[0].hanzi), (1, 2));
    // 词汇记录：上屏带译词的中文候选，那条译词记「上屏过」；没按修饰键打出来过，不是「用过」
    let counts = vocabulary.lock().unwrap();
    let entry = counts
        .get(&(Language::English, "develop".to_owned()))
        .expect("译词应当进了词汇记录");
    assert_eq!((entry.1, entry.2), (1, 0));
}

#[test]
fn keys_outside_the_code_alphabet_fall_back_to_raw() {
    let mut engine = wubi();
    engine.set_input("no-way");
    let query = engine.query().unwrap();
    assert_eq!(query.candidates.items[0].text, "no-way");
    assert_eq!(query.candidates.items[0].kind, CandidateKind::English);
}

#[test]
fn switching_back_to_pinyin_drops_the_code_table() {
    let mut engine = wubi();
    engine.set_code_table(None);
    assert!(!engine.is_code_mode());
    engine.set_input("kaifa");
    assert_eq!(engine.query().unwrap().candidates.items[0].text, "开发");
}
