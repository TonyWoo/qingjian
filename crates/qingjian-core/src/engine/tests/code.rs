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
