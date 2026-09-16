//! 「通用」页：学习语言、每页候选数、输入方案、英文模式候选。

use objc2::MainThreadMarker;
use objc2::rc::Retained;
use objc2_app_kit::{NSButton, NSPopUpButton};
use qingjian_core::Language;
use qingjian_platform::{Config, MAX_PAGE_SIZE, Scheme};

use crate::preferences::controls::{
    checkbox, language_label, note, row_checkbox, row_popup, select, set_checked,
};
use crate::preferences::layout::Layout;
use crate::preferences::setting::Setting;
use crate::preferences::target::PreferencesTarget;

pub struct GeneralPage {
    /// 学习语言。
    learning_language: Retained<NSPopUpButton>,

    /// 每页候选数。
    page_size: Retained<NSPopUpButton>,

    /// 拼音方案（按 `Scheme::ALL` 的顺序）。
    scheme: Retained<NSPopUpButton>,

    /// 五笔（86 版形码）；与拼音方案同时开着就是混输。
    wubi: Retained<NSButton>,

    /// 英文模式也给候选。
    english: Retained<NSButton>,

    /// 终端 / 编辑器里不给英文候选。
    english_off_in_apps: Retained<NSButton>,

    /// 学习语言弹出菜单里各项对应的语言。
    languages: Vec<Language>,

    /// 默认中文标点模式。
    punctuation: Retained<NSPopUpButton>,
}

impl GeneralPage {
    /// `languages` 是打进包里的释义表语言。
    pub fn build(
        layout: &mut Layout,
        mtm: MainThreadMarker,
        target: &PreferencesTarget,
        languages: &[Language],
    ) -> Self {
        let language_titles: Vec<String> = languages
            .iter()
            .map(|l| language_label(*l).to_owned())
            .collect();
        let learning_language = row_popup(
            layout,
            mtm,
            "学习语言",
            &language_titles,
            Setting::LearningLanguage,
            target,
        );
        note(
            layout,
            mtm,
            "候选词右侧显示哪种语言的译词，只列出安装了释义表的语言。",
        );
        let page_size_titles: Vec<String> = (1..=MAX_PAGE_SIZE).map(|n| n.to_string()).collect();
        let page_size = row_popup(
            layout,
            mtm,
            "每页候选数",
            &page_size_titles,
            Setting::PageSize,
            target,
        );
        let scheme_titles: Vec<String> = Scheme::ALL.iter().map(|s| s.label().to_owned()).collect();
        let scheme = row_popup(
            layout,
            mtm,
            "拼音方案",
            &scheme_titles,
            Setting::Scheme,
            target,
        );
        note(
            layout,
            mtm,
            "全拼、四套双拼、大千注音，或关（只用下面的五笔）。双拼与注音下 v、u、i 是按键，表达式与问字模式只能用 ? 开头进。",
        );
        let wubi = checkbox(mtm, "五笔（86 版）", Setting::Wubi, target);
        row_checkbox(layout, &wubi);
        note(
            layout,
            mtm,
            "与拼音方案同时开着就是混输：五笔候选在前，打不出的字直接打拼音。单用五笔请把拼音方案关掉；第 5 个字母起五笔查不到东西，自动只剩拼音。",
        );
        let punctuation = row_popup(
            layout,
            mtm,
            "默认中文标点",
            &["全角（，；：）".to_owned(), "半角（,;:）".to_owned()],
            Setting::FullWidthPunctuation,
            target,
        );
        note(
            layout,
            mtm,
            "仅影响标点，字母和数字保持半角；自定义短语原样输出。设置会保存。 ",
        );
        let english = checkbox(
            mtm,
            "英文模式（Caps Lock）也给候选",
            Setting::EnglishCandidates,
            target,
        );
        row_checkbox(layout, &english);
        note(
            layout,
            mtm,
            "Tab 或方向键选词；空格、回车、标点仍原样上屏敲的字母，不选词时与直接打字一样。",
        );
        let english_off_in_apps = checkbox(
            mtm,
            "但在终端和代码编辑器里不给",
            Setting::EnglishCandidatesOffInApps,
            target,
        );
        row_checkbox(layout, &english_off_in_apps);
        note(
            layout,
            mtm,
            "终端、iTerm、Warp、Ghostty、VS Code、Cursor、Zed、JetBrains、Xcode 等，那里的候选窗口会挡住应用自己的补全；名单可在配置文件里改。",
        );
        Self {
            learning_language,
            page_size,
            scheme,
            wubi,
            english,
            english_off_in_apps,
            languages: languages.to_vec(),
            punctuation,
        }
    }

    pub fn sync(&self, config: &Config) {
        let general = &config.general;
        select(
            &self.punctuation,
            Some(usize::from(!general.full_width_punctuation)),
        );
        select(
            &self.learning_language,
            self.languages
                .iter()
                .position(|l| l.code() == general.learning_language),
        );
        select(&self.page_size, Some(general.page_size() - 1));
        select(
            &self.scheme,
            Some(
                Scheme::ALL
                    .iter()
                    .position(|s| *s == general.scheme())
                    .unwrap_or(0),
            ),
        );
        set_checked(&self.wubi, general.wubi());
        set_checked(&self.english, general.english_candidates);
        set_checked(
            &self.english_off_in_apps,
            config.apps.has_english_candidates_off(),
        );
        self.english_off_in_apps
            .setEnabled(general.english_candidates);
    }
}
