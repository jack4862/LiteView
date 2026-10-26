use liteview::config::{AppSettings, Language, SettingsStore, UiLanguage};
use liteview::i18n::texts;
use std::fs;

#[test]
fn language_persists_and_invalid_values_keep_system() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let store = SettingsStore::new(directory.path().join("settings.ini"));
    let settings = AppSettings {
        language: Language::Chinese,
        ..AppSettings::default()
    };

    store.save(&settings).expect("save language setting");
    assert_eq!(store.load().language, Language::Chinese);

    fs::write(
        store.path(),
        b"[settings]\nlanguage=klingon\nzoom_mode=fit\n",
    )
    .expect("write invalid language");
    assert_eq!(store.load().language, Language::System);
}

#[test]
fn locale_resolution_prefers_chinese_and_falls_back_to_english() {
    assert_eq!(
        Language::System.resolve(Some("zh-Hans-CN")),
        UiLanguage::Chinese
    );
    assert_eq!(Language::System.resolve(Some("zh_TW")), UiLanguage::Chinese);
    assert_eq!(Language::System.resolve(Some("en-US")), UiLanguage::English);
    assert_eq!(Language::System.resolve(Some("fr-FR")), UiLanguage::English);
    assert_eq!(Language::System.resolve(None), UiLanguage::English);
    assert_eq!(
        Language::English.resolve(Some("zh-CN")),
        UiLanguage::English
    );
    assert_eq!(
        Language::Chinese.resolve(Some("en-US")),
        UiLanguage::Chinese
    );
}

#[test]
fn core_interface_labels_are_translated() {
    let english = texts(UiLanguage::English);
    let chinese = texts(UiLanguage::Chinese);

    assert_eq!(english.open, "Open");
    assert_eq!(chinese.open, "打开");
    assert_eq!(english.settings, "Settings");
    assert_eq!(chinese.settings, "设置");
    assert_eq!(english.image_info, "Image info");
    assert_eq!(chinese.image_info, "图像信息");
    assert_eq!(english.fit_to_window, "Fit to window");
    assert_eq!(chinese.fit_to_window, "适应窗口");
}
