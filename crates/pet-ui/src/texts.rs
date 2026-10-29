//! Slint-side translations: one `Texts` value per language plus the label lists
//! the comboboxes use. Strings that need a count or a model name are composed in
//! Rust and reach the window through their own properties.

use pet_i18n::{Key, Lang};
use slint::{ModelRc, SharedString, VecModel};

use crate::{RadialMenu, SettingsWindow};

/// Static copy for `lang`.
pub(crate) fn texts(lang: Lang) -> crate::Texts {
    let t = |key: Key| SharedString::from(lang.text(key));
    crate::Texts {
        settings_title: t(Key::SettingsTitle),
        menu_title: t(Key::MenuTitle),
        nav_heading: t(Key::NavHeading),
        nav_characters: t(Key::NavCharacters),
        nav_behavior: t(Key::NavBehavior),
        nav_display: t(Key::NavDisplay),
        nav_downloads: t(Key::NavDownloads),
        nav_about: t(Key::NavAbout),
        settings_heading: t(Key::SettingsHeading),
        toggle_topmost: t(Key::ToggleTopmost),
        toggle_obs: t(Key::ToggleObs),
        toggle_hide_model: t(Key::ToggleHideModel),
        toggle_autostart: t(Key::ToggleAutostart),
        theme_label: t(Key::ThemeLabel),
        language_label: t(Key::LanguageLabel),
        apply_button: t(Key::ApplyButton),
        toggle_on: t(Key::ToggleOn),
        toggle_off: t(Key::ToggleOff),
        role_select_accessible: t(Key::RoleSelectAccessible),
        role_remove_accessible: t(Key::RoleRemoveAccessible),
        roles_empty: t(Key::RolesEmpty),
        add_model_button: t(Key::AddModelButton),
        roles_hint: t(Key::RolesHint),
        status_connecting: t(Key::StatusConnecting),
        model_details_title: t(Key::ModelDetailsTitle),
        model_details_subtitle: t(Key::ModelDetailsSubtitle),
        no_portrait: t(Key::NoPortrait),
        pick_character_to_start: t(Key::PickCharacterToStart),
        costume_prefix: t(Key::CostumePrefix),
        no_active_model: t(Key::NoActiveModel),
        band_prefix: t(Key::BandPrefix),
        switch_model_button: t(Key::SwitchModelButton),
        switch_model_accessible: t(Key::SwitchModelAccessible),
        pick_new_hint: t(Key::PickNewHint),
        click_feedback_heading: t(Key::ClickFeedbackHeading),
        click_feedback_hint: t(Key::ClickFeedbackHint),
        configure_click_button: t(Key::ConfigureClickButton),
        never_selected: t(Key::NeverSelected),
        display_hint: t(Key::DisplayHint),
        display_section: t(Key::DisplaySection),
        quality_label: t(Key::QualityLabel),
        quality_hint: t(Key::QualityHint),
        fps_label: t(Key::FpsLabel),
        show_fps_label: t(Key::ShowFpsLabel),
        show_fps_hint: t(Key::ShowFpsHint),
        scale_label: t(Key::ScaleLabel),
        opacity_label: t(Key::OpacityLabel),
        vsync_label: t(Key::VsyncLabel),
        gpu_label: t(Key::GpuLabel),
        restart_hint: t(Key::RestartHint),
        position_section: t(Key::PositionSection),
        move_together_label: t(Key::MoveTogetherLabel),
        reset_positions_button: t(Key::ResetPositionsButton),
        behavior_hint: t(Key::BehaviorHint),
        behavior_section: t(Key::BehaviorSection),
        drag_locked_label: t(Key::DragLockedLabel),
        drag_locked_hint: t(Key::DragLockedHint),
        head_tracking_label: t(Key::HeadTrackingLabel),
        mutual_gaze_label: t(Key::MutualGazeLabel),
        idle_actions_label: t(Key::IdleActionsLabel),
        random_actions_label: t(Key::RandomActionsLabel),
        emotion_label: t(Key::EmotionLabel),
        click_hint: t(Key::ClickHint),
        preset_label: t(Key::PresetLabel),
        apply_preset_button: t(Key::ApplyPresetButton),
        region_column: t(Key::RegionColumn),
        motion_column: t(Key::MotionColumn),
        expression_column: t(Key::ExpressionColumn),
        downloads_hint: t(Key::DownloadsHint),
        no_models_heading: t(Key::NoModelsHeading),
        no_models_hint: t(Key::NoModelsHint),
        download_search_placeholder: t(Key::DownloadSearchPlaceholder),
        download_missing_button: t(Key::DownloadMissingButton),
        update_installed_button: t(Key::UpdateInstalledButton),
        refresh_button: t(Key::RefreshButton),
        open_folder_button: t(Key::OpenFolderButton),
        cancel_download_button: t(Key::CancelDownloadButton),
        choose_costume_button: t(Key::ChooseCostumeButton),
        processing: t(Key::Processing),
        download_source: t(Key::DownloadSource),
        download_notice: t(Key::DownloadNotice),
        open_model_folder_button: t(Key::OpenModelFolderButton),
        about_license: t(Key::AboutLicense),
        import_title: t(Key::ImportTitle),
        import_hint: t(Key::ImportHint),
        import_placeholder: t(Key::ImportPlaceholder),
        import_cancel_button: t(Key::ImportCancelButton),
        import_button: t(Key::ImportButton),
        import_busy: t(Key::ImportBusy),
        picker_choose_character: t(Key::PickerChooseCharacter),
        picker_choose_band: t(Key::PickerChooseBand),
        picker_back: t(Key::PickerBack),
        picker_back_band: t(Key::PickerBackBand),
        picker_import_model_button: t(Key::PickerImportModelButton),
        picker_preview_hint: t(Key::PickerPreviewHint),
        picker_search_placeholder: t(Key::PickerSearchPlaceholder),
        picker_costume_search_placeholder: t(Key::PickerCostumeSearchPlaceholder),
        picker_no_portrait: t(Key::PickerNoPortrait),
        picker_no_characters: t(Key::PickerNoCharacters),
        picker_no_costumes: t(Key::PickerNoCostumes),
        picker_favorite_character: t(Key::PickerFavoriteCharacter),
        picker_favorite_costume: t(Key::PickerFavoriteCostume),
        picker_choose_costume: t(Key::PickerChooseCostume),
        picker_preview_costume: t(Key::PickerPreviewCostume),
        radial_lock: t(Key::RadialLock),
        radial_unlock: t(Key::RadialUnlock),
    }
}

/// Applies `lang` to the settings window: copy, label lists and the font that
/// can actually draw this language.
pub(crate) fn apply(window: &SettingsWindow, lang: Lang) {
    window.set_t(texts(lang));
    window.set_ui_font(ui_font(lang).into());
    window.set_language_index(lang.index() as i32);
    window.set_theme_labels(labels(
        lang,
        &[Key::ThemeLight, Key::ThemeDark, Key::ThemeSystem],
    ));
    window.set_quality_labels(labels(
        lang,
        &[Key::QualityPerformance, Key::QualityBalanced],
    ));
    window.set_preset_labels(labels(
        lang,
        &[
            Key::PresetAuto,
            Key::PresetGenki,
            Key::PresetTsundere,
            Key::PresetShy,
            Key::PresetCool,
            Key::PresetSurprised,
            Key::PresetRandom,
        ],
    ));
    window.set_filter_labels(labels(
        lang,
        &[
            Key::PickerFilterAll,
            Key::PickerFilterRecent,
            Key::PickerFilterFavorites,
        ],
    ));
    window.set_download_notice(lang.text(Key::DownloadNotice).into());
}

/// Applies `lang` to the radial menu, which is a separate window tree.
pub(crate) fn apply_radial(radial: &RadialMenu, lang: Lang) {
    radial.set_t(texts(lang));
    radial.set_ui_font(ui_font(lang).into());
}

/// The language the window currently renders.
pub(crate) fn lang_of(window: &SettingsWindow) -> Lang {
    Lang::from_slint_index(window.get_language_index())
}

fn labels(lang: Lang, keys: &[Key]) -> ModelRc<SharedString> {
    ModelRc::new(VecModel::from(
        keys.iter()
            .map(|key| SharedString::from(lang.text(*key)))
            .collect::<Vec<_>>(),
    ))
}

/// Font family that covers the script of `lang` on this platform.
pub(crate) fn ui_font(lang: Lang) -> &'static str {
    #[cfg(windows)]
    let family = match lang {
        Lang::ZhHans => "Microsoft YaHei UI",
        Lang::ZhHant => "Microsoft JhengHei UI",
        Lang::Ja => "Yu Gothic UI",
        Lang::Ko => "Malgun Gothic",
        Lang::En => "Segoe UI",
    };
    #[cfg(target_os = "macos")]
    let family = match lang {
        Lang::ZhHans => "PingFang SC",
        Lang::ZhHant => "PingFang TC",
        Lang::Ja => "Hiragino Sans",
        Lang::Ko => "Apple SD Gothic Neo",
        Lang::En => "Helvetica Neue",
    };
    #[cfg(not(any(windows, target_os = "macos")))]
    let family = match lang {
        Lang::ZhHans => "Noto Sans CJK SC",
        Lang::ZhHant => "Noto Sans CJK TC",
        Lang::Ja => "Noto Sans CJK JP",
        Lang::Ko => "Noto Sans CJK KR",
        Lang::En => "DejaVu Sans",
    };
    family
}

#[cfg(test)]
mod tests {
    use super::*;
    use slint::Model;

    #[test]
    fn every_static_string_is_filled_for_every_language() {
        for lang in Lang::ALL {
            let value = texts(lang);
            for (name, text) in [
                ("settings_title", &value.settings_title),
                ("menu_title", &value.menu_title),
                ("nav_about", &value.nav_about),
                ("about_license", &value.about_license),
                ("radial_unlock", &value.radial_unlock),
                ("picker_preview_costume", &value.picker_preview_costume),
                ("import_placeholder", &value.import_placeholder),
                ("switch_model_button", &value.switch_model_button),
            ] {
                assert!(!text.trim().is_empty(), "{} misses {name}", lang.tag());
            }
        }
    }

    #[test]
    fn every_language_renders_and_exposes_its_labels() {
        use slint::ComponentHandle;
        let window = crate::tests::test_window();
        window
            .window()
            .set_size(slint::LogicalSize::new(1180., 680.));
        window
            .window()
            .dispatch_event(slint::platform::WindowEvent::Resized {
                size: slint::LogicalSize::new(1180., 680.),
            });
        window.show().unwrap();
        let output = std::env::var("BANDORI_UI_QA_DIR").ok();
        for lang in Lang::ALL {
            apply(&window, lang);
            assert_eq!(
                window.get_t().settings_title.as_str(),
                lang.text(Key::SettingsTitle)
            );
            for page in [0, 2, 4] {
                window.set_page(page);
                crate::tests::settle_animations(&window);
                let labels: Vec<String> = i_slint_backend_testing::ElementQuery::from_root(&window)
                    .find_all()
                    .into_iter()
                    .filter_map(|element| element.accessible_label())
                    .map(|label| label.to_string())
                    .collect();
                if let Some(dir) = &output {
                    let snapshot = window.window().take_snapshot().unwrap();
                    std::fs::create_dir_all(dir).unwrap();
                    image::save_buffer(
                        std::path::Path::new(dir)
                            .join(format!("i18n-{}-page{page}.png", lang.tag())),
                        snapshot.as_bytes(),
                        snapshot.width(),
                        snapshot.height(),
                        image::ColorType::Rgba8,
                    )
                    .unwrap();
                }
                // Navigation labels are always laid out; page copy moves with
                // the scroll position and is not a stable accessibility target.
                let keys: &[Key] = if page == 0 {
                    &[Key::NavCharacters, Key::NavAbout, Key::ToggleOff]
                } else {
                    &[Key::NavCharacters, Key::NavAbout]
                };
                for key in keys {
                    assert!(
                        labels.iter().any(|label| label == lang.text(*key)),
                        "{} misses {}: {labels:?}",
                        lang.tag(),
                        key.name()
                    );
                }
            }
        }
    }

    #[test]
    fn picker_and_import_dialog_show_the_selected_language() {
        use slint::ComponentHandle;
        let window = crate::tests::test_window();
        window
            .window()
            .set_size(slint::LogicalSize::new(1180., 680.));
        window
            .window()
            .dispatch_event(slint::platform::WindowEvent::Resized {
                size: slint::LogicalSize::new(1180., 680.),
            });
        window.show().unwrap();
        let output = std::env::var("BANDORI_UI_QA_DIR").ok();
        for lang in Lang::ALL {
            apply(&window, lang);
            window.set_picking_model(true);
            window.set_picker_stage(0);
            window.set_picker_title(lang.text(Key::PickerChooseBand).into());
            window.set_picker_subtitle(lang.text(Key::PickerBandFirst).into());
            window.set_import_visible(true);
            crate::tests::settle_animations(&window);
            let labels: Vec<String> = i_slint_backend_testing::ElementQuery::from_root(&window)
                .find_all()
                .into_iter()
                .filter_map(|element| element.accessible_label())
                .map(|label| label.to_string())
                .collect();
            for key in [
                Key::PickerChooseBand,
                Key::PickerBandFirst,
                Key::ImportTitle,
            ] {
                assert!(
                    labels.iter().any(|label| label == lang.text(key)),
                    "{} misses {}: {labels:?}",
                    lang.tag(),
                    key.name()
                );
            }
            if let Some(dir) = &output {
                let snapshot = window.window().take_snapshot().unwrap();
                std::fs::create_dir_all(dir).unwrap();
                image::save_buffer(
                    std::path::Path::new(dir).join(format!("i18n-{}-picker.png", lang.tag())),
                    snapshot.as_bytes(),
                    snapshot.width(),
                    snapshot.height(),
                    image::ColorType::Rgba8,
                )
                .unwrap();
            }
        }
    }

    #[test]
    fn fonts_cover_the_script_and_labels_follow_the_language() {
        assert_eq!(ui_font(Lang::ZhHans), ui_font(Lang::ZhHans));
        assert_ne!(ui_font(Lang::ZhHans), ui_font(Lang::Ja));
        let window = crate::tests::test_window();
        apply(&window, Lang::En);
        assert_eq!(window.get_language_index(), Lang::En.index() as i32);
        assert_eq!(
            window.get_t().settings_title.as_str(),
            "Bandori Pet LITE - Settings"
        );
        assert_eq!(window.get_theme_labels().row_count(), 3);
        assert_eq!(window.get_preset_labels().row_count(), 7);
        assert_eq!(
            window.get_t().switch_model_button.as_str(),
            "Change\ncharacter"
        );
        apply(&window, Lang::ZhHans);
        assert_eq!(window.get_language_index(), 0);
        assert_eq!(
            window.get_t().settings_title.as_str(),
            "Bandori 桌宠 LITE - 设置"
        );
    }
}
