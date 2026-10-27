//! The three-step model picker. Archive IO and preview rendering stay off the UI thread.
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::mpsc::{channel, Receiver, Sender as ChannelSender};

use pet_assets::catalog::Catalog;
use pet_config::{Config, ModelPickerState, Paths, SettingsPatch};
use pet_i18n::{Key, Lang};
use pet_ipc::Message;
use slint::{ComponentHandle, ModelRc, VecModel};

use super::texts;
use super::{CostumeItem, PickerCard, PickerRow, Sender, SettingsWindow};

enum Request {
    Portrait(String, String),
    Preview(String, String),
    Import(PathBuf, Lang),
}
enum ResultItem {
    Portrait(String, Option<PathBuf>),
    Preview(String, Result<PathBuf, String>),
    Import(Result<Catalog, String>),
}

pub(super) struct Picker {
    catalog: Rc<RefCell<Catalog>>,
    paths: Paths,
    state: RefCell<ModelPickerState>,
    pictures: RefCell<HashMap<String, slint::Image>>,
    requested: RefCell<HashSet<String>>,
    requests: ChannelSender<Request>,
    results: Receiver<ResultItem>,
    send: Sender,
    pinned: RefCell<String>,
}

impl Picker {
    pub fn new(
        catalog: Rc<RefCell<Catalog>>,
        paths: &Paths,
        send: Sender,
    ) -> std::io::Result<Rc<Self>> {
        let (requests, receiver) = channel();
        let (results, result_rx) = channel();
        let worker_paths = paths.clone();
        std::thread::Builder::new()
            .name("bandori-picker-assets".into())
            .spawn(move || {
                let cache = pet_assets::cache::ModelCache::new(
                    worker_paths.cache().join("portraits"),
                    worker_paths.models(),
                );
                while let Ok(request) = receiver.recv() {
                    let result = match request {
                        Request::Portrait(character, archive) => {
                            let mut picture = None;
                            for member in [
                                "character.png".to_owned(),
                                format!("{character}/character.png"),
                                "character.jpg".to_owned(),
                                format!("{character}/character.jpg"),
                                "character.webp".to_owned(),
                                format!("{character}/character.webp"),
                            ] {
                                if let Ok(path) =
                                    cache.extract_member(&format!("{archive}::{member}"))
                                {
                                    picture = Some(path);
                                    break;
                                }
                            }
                            ResultItem::Portrait(character, picture)
                        }
                        Request::Preview(key, path) => {
                            ResultItem::Preview(key, super::preview::render(&worker_paths, &path))
                        }
                        Request::Import(path, lang) => {
                            ResultItem::Import(import_archive(&worker_paths, &path, lang))
                        }
                    };
                    if results.send(result).is_err() {
                        break;
                    }
                }
            })?;
        Ok(Rc::new(Self {
            catalog,
            paths: paths.clone(),
            state: RefCell::new(ModelPickerState::default()),
            pictures: RefCell::new(HashMap::new()),
            requested: RefCell::new(HashSet::new()),
            requests,
            results: result_rx,
            send,
            pinned: RefCell::new(String::new()),
        }))
    }

    pub fn wire(self: &Rc<Self>, window: &SettingsWindow) {
        let weak = window.as_weak();
        let picker = self.clone();
        window.on_start_picker(move || {
            if let Some(window) = weak.upgrade() {
                window.set_picking_model(true);
                window.set_picker_stage(0);
                window.set_selected_band("".into());
                window.set_search("".into());
                window.set_character_filter(0);
                picker.close_preview(&window);
                picker.refresh(&window);
            }
        });
        let weak = window.as_weak();
        let picker = self.clone();
        window.on_picker_back(move || {
            if let Some(window) = weak.upgrade() {
                picker.close_preview(&window);
                if window.get_picker_stage() == 2 {
                    window.set_picker_stage(1);
                } else {
                    window.set_picker_stage(0);
                    window.set_selected_band("".into());
                    window.set_search("".into());
                    window.set_character_filter(0);
                }
                picker.refresh(&window);
            }
        });
        let weak = window.as_weak();
        let picker = self.clone();
        window.on_choose_card(move |key| {
            if let Some(window) = weak.upgrade() {
                if window.get_showing_characters() {
                    let preferred = (window.get_selected_character() == key)
                        .then(|| window.get_selected_costume());
                    super::populate_costumes(
                        &window,
                        &picker.catalog.borrow(),
                        &key,
                        preferred.as_deref(),
                    );
                    window.set_costume_search("".into());
                    window.set_costume_filter(0);
                    window.set_picker_stage(2);
                } else {
                    window.set_selected_band(key);
                    window.set_picker_stage(1);
                }
                picker.refresh(&window);
            }
        });
        let weak = window.as_weak();
        let picker = self.clone();
        window.on_refresh_picker(move || {
            if let Some(window) = weak.upgrade() {
                picker.refresh(&window);
            }
        });
        let weak = window.as_weak();
        let picker = self.clone();
        window.on_favorite_character(move |key| {
            if let Some(window) = weak.upgrade() {
                picker.state.borrow_mut().toggle_favorite_character(&key);
                picker.save_state();
                picker.refresh(&window);
            }
        });
        let weak = window.as_weak();
        let picker = self.clone();
        window.on_favorite_costume(move |id| {
            if let Some(window) = weak.upgrade() {
                picker
                    .state
                    .borrow_mut()
                    .toggle_favorite_costume(&window.get_selected_character(), &id);
                picker.save_state();
                picker.refresh(&window);
            }
        });
        let weak = window.as_weak();
        let picker = self.clone();
        window.on_preview(move |id| {
            if let Some(window) = weak.upgrade() {
                if id.is_empty() || *picker.pinned.borrow() == id.as_str() {
                    picker.close_preview(&window);
                } else {
                    *picker.pinned.borrow_mut() = id.to_string();
                    picker.show_preview(&window, &id);
                }
            }
        });
        let weak = window.as_weak();
        let picker = self.clone();
        window.on_hover_preview(move |id, hovered| {
            if let Some(window) = weak.upgrade() {
                if picker.pinned.borrow().is_empty() {
                    if hovered && window.get_shift_held() {
                        picker.show_preview(&window, &id);
                    } else if window.get_preview_costume() == id {
                        picker.close_preview(&window);
                    }
                }
            }
        });
        let weak = window.as_weak();
        let picker = self.clone();
        window.on_import_archive(move |path| {
            if let Some(window) = weak.upgrade() {
                window.set_importing(true);
                window.set_import_error("".into());
                let _ = picker.requests.send(Request::Import(
                    PathBuf::from(path.trim().trim_matches('"')),
                    texts::lang_of(&window),
                ));
            }
        });
    }

    pub fn update_config(&self, window: &SettingsWindow, config: &Config) {
        *self.state.borrow_mut() = config.model_picker_state.clone();
        self.queue_portrait(&config.character);
        if let Some(picture) = self.pictures.borrow().get(&config.character) {
            window.set_portrait(picture.clone());
            window.set_has_portrait(true);
        } else {
            window.set_portrait(slint::Image::default());
            window.set_has_portrait(false);
        }
        self.refresh(window);
    }

    fn save_state(&self) {
        let patch = SettingsPatch::from_pairs([(
            "model_picker_state",
            serde_json::to_value(&*self.state.borrow()).unwrap(),
        )]);
        (self.send)(&Message::SettingsPatch { patch });
    }

    pub fn remember_selection(&self, character: &str, costume: &str) -> ModelPickerState {
        let mut state = self.state.borrow_mut();
        state.remember(character, costume);
        state.clone()
    }

    pub fn close_preview(&self, window: &SettingsWindow) {
        self.pinned.borrow_mut().clear();
        window.set_preview_visible(false);
        window.set_preview_costume("".into());
    }

    pub fn catalog_changed(&self, window: &SettingsWindow) {
        self.pictures.borrow_mut().clear();
        self.requested.borrow_mut().clear();
        self.close_preview(window);
        self.refresh(window);
    }

    pub fn choose_downloaded(&self, window: &SettingsWindow, key: &str) {
        window.set_page(0);
        window.set_adding_model(true);
        window.set_picking_model(true);
        window.set_search("".into());
        window.set_character_filter(0);
        window.set_selected_band(
            self.catalog
                .borrow()
                .character(key)
                .map(|character| character.band_id.as_str())
                .unwrap_or("")
                .into(),
        );
        window.set_costume_search("".into());
        window.set_costume_filter(0);
        self.close_preview(window);
        super::populate_costumes(window, &self.catalog.borrow(), key, None);
        window.set_picker_stage(2);
        self.refresh(window);
    }

    fn show_preview(&self, window: &SettingsWindow, id: &str) {
        if window.get_preview_visible() && window.get_preview_costume() == id {
            return;
        }
        let catalog = self.catalog.borrow();
        let character = window.get_selected_character();
        let Some(costume) = catalog.find(&character, id) else {
            return;
        };
        window.set_preview_costume(id.into());
        window.set_preview_label(costume.display.clone().into());
        window.set_preview_image(slint::Image::default());
        window.set_preview_status(texts::lang_of(window).text(Key::PreviewGenerating).into());
        window.set_preview_visible(true);
        let _ = self.requests.send(Request::Preview(
            format!("{character}:{id}"),
            costume.path.clone(),
        ));
    }

    fn queue_portrait(&self, character: &str) {
        if character.is_empty() || self.requested.borrow().contains(character) {
            return;
        }
        let catalog = self.catalog.borrow();
        if let Some((archive, _)) = catalog
            .costumes_of(character)
            .first()
            .and_then(|costume| costume.path.split_once("::"))
        {
            self.requested.borrow_mut().insert(character.to_owned());
            let _ = self.requests.send(Request::Portrait(
                character.into(),
                self.paths
                    .root()
                    .join(archive)
                    .to_string_lossy()
                    .into_owned(),
            ));
        }
    }

    pub fn poll(&self, window: &SettingsWindow, active_character: &str) {
        let mut refresh = false;
        while let Ok(result) = self.results.try_recv() {
            match result {
                ResultItem::Portrait(character, path) => {
                    if let Some(picture) =
                        path.and_then(|path| slint::Image::load_from_path(&path).ok())
                    {
                        if character == active_character {
                            window.set_portrait(picture.clone());
                            window.set_has_portrait(true);
                        }
                        self.pictures.borrow_mut().insert(character, picture);
                        refresh = true;
                    }
                }
                ResultItem::Preview(key, result) => {
                    if window.get_preview_visible()
                        && key
                            == format!(
                                "{}:{}",
                                window.get_selected_character(),
                                window.get_preview_costume()
                            )
                    {
                        match result.and_then(|path| {
                            slint::Image::load_from_path(&path).map_err(|e| e.to_string())
                        }) {
                            Ok(image) => {
                                window.set_preview_image(image);
                                window.set_preview_status(
                                    texts::lang_of(window).text(Key::PreviewStatic).into(),
                                );
                            }
                            Err(error) => window.set_preview_status(
                                texts::lang_of(window)
                                    .format(Key::PreviewFailed, &[&error])
                                    .into(),
                            ),
                        }
                    }
                }
                ResultItem::Import(result) => {
                    window.set_importing(false);
                    match result {
                        Ok(catalog) => {
                            *self.catalog.borrow_mut() = catalog;
                            window.set_import_visible(false);
                            window.set_status(
                                texts::lang_of(window).text(Key::StatusModelImported).into(),
                            );
                            refresh = true;
                        }
                        Err(error) => window.set_import_error(error.into()),
                    }
                }
            }
        }
        if refresh {
            self.refresh(window);
        }
    }

    pub fn refresh(&self, window: &SettingsWindow) {
        let catalog = self.catalog.borrow();
        let state = self.state.borrow();
        let lang = texts::lang_of(window);
        let stage = window.get_picker_stage();
        let band = window.get_selected_band();
        let query = window.get_search().trim().to_lowercase();
        let filter = window.get_character_filter();
        let show_characters = stage != 0 || !query.is_empty() || filter != 0;
        window.set_showing_characters(show_characters);
        window.set_selected_band_label(
            catalog
                .bands()
                .iter()
                .find(|entry| entry.id == band.as_str())
                .map(|entry| entry.display.as_str())
                .unwrap_or("")
                .into(),
        );
        let character_label = window.get_selected_character_label();
        let band_label = window.get_selected_band_label();
        let (title, subtitle) = heading(
            lang,
            stage,
            show_characters,
            character_label.as_str(),
            band_label.as_str(),
        );
        window.set_picker_title(title.into());
        window.set_picker_subtitle(subtitle.into());
        if stage == 2 {
            let character = window.get_selected_character();
            let query = window.get_costume_search().trim().to_lowercase();
            let filter = window.get_costume_filter();
            let mut costumes: Vec<_> = catalog
                .costumes_of(&character)
                .iter()
                .filter(|costume| {
                    let key = format!("{character}:{}", costume.id);
                    (query.is_empty()
                        || costume.display.to_lowercase().contains(&query)
                        || costume.id.to_lowercase().contains(&query))
                        && matches_filter(
                            filter,
                            &key,
                            &state.recent_costumes,
                            &state.favorite_costumes,
                        )
                })
                .collect();
            if filter != 0 {
                costumes.sort_by_key(|costume| {
                    position(
                        filter,
                        &format!("{character}:{}", costume.id),
                        &state.recent_costumes,
                        &state.favorite_costumes,
                    )
                });
            }
            window.set_picker_costumes(ModelRc::new(VecModel::from(
                costumes
                    .into_iter()
                    .map(|costume| CostumeItem {
                        id: costume.id.clone().into(),
                        display: costume.display.clone().into(),
                        favorite: state
                            .favorite_costumes
                            .contains(&format!("{character}:{}", costume.id)),
                    })
                    .collect::<Vec<_>>(),
            )));
            return;
        }
        let cards: Vec<_> = if show_characters {
            let mut characters: Vec<_> = catalog
                .search(&query)
                .into_iter()
                .filter(|character| {
                    (band.is_empty()
                        || catalog.bands().iter().any(|entry| {
                            entry.id == band.as_str() && entry.characters.contains(&character.key)
                        }))
                        && matches_filter(
                            filter,
                            &character.key,
                            &state.recent_characters,
                            &state.favorite_characters,
                        )
                })
                .collect();
            if filter != 0 {
                characters.sort_by_key(|character| {
                    position(
                        filter,
                        &character.key,
                        &state.recent_characters,
                        &state.favorite_characters,
                    )
                });
            }
            characters
                .into_iter()
                .map(|character| {
                    self.queue_portrait(&character.key);
                    let picture = self.pictures.borrow().get(&character.key).cloned();
                    PickerCard {
                        key: character.key.clone().into(),
                        display: character.display.clone().into(),
                        count_label: lang.character_count(character.costumes.len()).into(),
                        has_picture: picture.is_some(),
                        picture: picture.unwrap_or_default(),
                        favorite: state.favorite_characters.contains(&character.key),
                    }
                })
                .collect()
        } else {
            catalog
                .bands()
                .iter()
                .map(|band| PickerCard {
                    key: band.id.clone().into(),
                    display: band.display.clone().into(),
                    count_label: lang.character_count(band.characters.len()).into(),
                    picture: band_logo(&self.paths, &band.id, &band.logo),
                    has_picture: !band.logo.is_empty(),
                    favorite: false,
                })
                .collect()
        };
        window.set_picker_rows(ModelRc::new(VecModel::from(
            cards
                .chunks(3)
                .map(|cards| PickerRow {
                    cards: ModelRc::new(VecModel::from(cards.to_vec())),
                })
                .collect::<Vec<_>>(),
        )));
    }
}

fn matches_filter(filter: i32, key: &str, recent: &[String], favorites: &[String]) -> bool {
    match filter {
        1 => recent.iter().any(|value| value == key),
        2 => favorites.iter().any(|value| value == key),
        _ => true,
    }
}
fn position(filter: i32, key: &str, recent: &[String], favorites: &[String]) -> usize {
    (if filter == 1 { recent } else { favorites })
        .iter()
        .position(|value| value == key)
        .unwrap_or(usize::MAX)
}

fn band_logo(paths: &Paths, id: &str, logo: &str) -> slint::Image {
    for path in [paths.assets().join(logo), paths.root().join(logo)] {
        if !logo.is_empty() && path.is_file() {
            if let Ok(image) = slint::Image::load_from_path(&path) {
                return image;
            }
        }
    }
    // Bundled images also work when only the two executables are deployed.
    let bytes: &[u8] = match id {
        "poppin_party" => include_bytes!("../../../assets/band_logo/poppinparty.png"),
        "afterglow" => include_bytes!("../../../assets/band_logo/afterglow.png"),
        "pastel_palettes" => include_bytes!("../../../assets/band_logo/pastelpalettes.png"),
        "hello_happy_world" => include_bytes!("../../../assets/band_logo/hellohappyworld.png"),
        "roselia" => include_bytes!("../../../assets/band_logo/roselia.png"),
        "raise_a_suilen" => include_bytes!("../../../assets/band_logo/raiseasuilen.png"),
        "morfonica" => include_bytes!("../../../assets/band_logo/morfonica.png"),
        "mygo" => include_bytes!("../../../assets/band_logo/mygo.png"),
        "ave_mujica" => include_bytes!("../../../assets/band_logo/avemujica.png"),
        "yumemita" => include_bytes!("../../../assets/band_logo/yumemita.png"),
        "millsage" => include_bytes!("../../../assets/band_logo/millsage.png"),
        "ikka_dumb_rock" => include_bytes!("../../../assets/band_logo/ikkadumbrock.png"),
        _ => return slint::Image::default(),
    };
    let Ok(pixels) = image::load_from_memory(bytes).map(|image| image.to_rgba8()) else {
        return slint::Image::default();
    };
    slint::Image::from_rgba8(slint::SharedPixelBuffer::clone_from_slice(
        pixels.as_raw(),
        pixels.width(),
        pixels.height(),
    ))
}

/// Title and subtitle above the picker grid for the current stage.
fn heading(
    lang: Lang,
    stage: i32,
    show_characters: bool,
    character: &str,
    band: &str,
) -> (String, String) {
    if stage == 2 {
        (
            format!("{character} - {}", lang.text(Key::PickerCostumeList)),
            lang.format(Key::PickerChooseCostumeFor, &[character]),
        )
    } else if show_characters {
        let subtitle = if band.is_empty() {
            lang.text(Key::PickerPickCharacter).to_owned()
        } else {
            lang.format(Key::PickerPickCharacterInBand, &[band])
        };
        (lang.text(Key::PickerChooseCharacter).to_owned(), subtitle)
    } else {
        (
            lang.text(Key::PickerChooseBand).to_owned(),
            lang.text(Key::PickerBandFirst).to_owned(),
        )
    }
}

fn import_archive(paths: &Paths, source: &Path, lang: Lang) -> Result<Catalog, String> {
    if !source
        .extension()
        .and_then(|value| value.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("zst"))
    {
        return Err(lang.text(Key::ImportNeedsArchive).to_owned());
    }
    let scan = pet_assets::index::scan_archive(source)
        .map_err(|error| lang.format(Key::ImportUnreadable, &[&error.to_string()]))?;
    if scan.costumes.is_empty() {
        return Err(lang.text(Key::ImportNoModelFile).to_owned());
    }
    std::fs::create_dir_all(paths.models()).map_err(|error| error.to_string())?;
    let destination = paths.models().join(
        source
            .file_name()
            .ok_or_else(|| lang.text(Key::ImportMissingFileName).to_owned())?,
    );
    if destination.exists() {
        return Err(lang.text(Key::ImportDuplicateName).to_owned());
    }
    let temporary = destination.with_extension("zst.importing");
    let mut output = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)
        .map_err(|error| lang.format(Key::ImportCreateFailed, &[&error.to_string()]))?;
    let copy = (|| {
        std::io::copy(&mut std::fs::File::open(source)?, &mut output)?;
        output.sync_all()?;
        drop(output);
        std::fs::rename(&temporary, &destination)
    })();
    if let Err(error) = copy {
        let _ = std::fs::remove_file(&temporary);
        return Err(lang.format(Key::ImportFailed, &[&error.to_string()]));
    }
    Ok(Catalog::build(
        &paths.assets(),
        &pet_assets::index::load_or_scan(&paths.models()),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use pet_assets::index::{ArchiveEntry, ArchiveScan, CostumeEntry, ModelIndex};
    use pet_config::ModelFormat;
    use slint::Model;

    fn fixture_catalog() -> Catalog {
        let mut index = ModelIndex::default();
        // Deliberately unordered archive keys: the picker must follow band.json.
        for key in ["arisa", "saaya", "rimi", "tae", "kasumi", "anon"] {
            index.archives.insert(
                format!("models/{key}.zst"),
                ArchiveEntry {
                    size: 1,
                    mtime_ns: 0,
                    result: Some(ArchiveScan {
                        character: key.into(),
                        costumes: ["live_default", "casual-2023"]
                            .into_iter()
                            .map(|id| CostumeEntry {
                                id: id.into(),
                                path: format!("models/{key}.zst::{id}/model.json"),
                                format: ModelFormat::Moc,
                            })
                            .collect(),
                    }),
                },
            );
        }
        Catalog::build(Path::new("missing-test-assets"), &index)
    }

    #[test]
    fn downloaded_character_opens_costumes_and_returns_to_its_own_band() {
        i_slint_backend_testing::init_no_event_loop();
        let window = super::super::tests::test_window();
        let picker = Picker::new(
            Rc::new(RefCell::new(fixture_catalog())),
            &Paths::with_root("missing-test-root"),
            Rc::new(|_| true),
        )
        .unwrap();
        picker.wire(&window);
        window.set_selected_band("poppin_party".into());
        window.set_character_filter(2);
        window.set_costume_filter(2);
        window.set_costume_search("no-match".into());
        picker.choose_downloaded(&window, "anon");
        assert_eq!(window.get_page(), 0);
        assert!(window.get_adding_model());
        assert_eq!(window.get_picker_stage(), 2);
        assert_eq!(window.get_selected_character(), "anon");
        assert_eq!(window.get_selected_band(), "mygo");
        assert_eq!(window.get_picker_costumes().row_count(), 2);
        window.invoke_picker_back();
        assert_eq!(window.get_picker_stage(), 1);
        assert_eq!(
            window
                .get_picker_rows()
                .row_data(0)
                .unwrap()
                .cards
                .row_data(0)
                .unwrap()
                .key,
            "anon"
        );
    }

    #[test]
    fn picker_navigation_search_favorites_and_responsive_layout() {
        use i_slint_backend_testing::{ElementHandle, TestingBackend, TestingBackendOptions};
        slint::platform::set_platform(Box::new(TestingBackend::new(TestingBackendOptions {
            renderer_name: Some("software".into()),
            mock_time: true,
            ..Default::default()
        })))
        .unwrap();
        let window = SettingsWindow::new().unwrap();
        window.set_theme(1);
        let picker = Picker::new(
            Rc::new(RefCell::new(fixture_catalog())),
            &Paths::with_root("missing-test-root"),
            Rc::new(|_| true),
        )
        .unwrap();
        picker.wire(&window);
        window.invoke_start_picker();
        assert_eq!(window.get_picker_stage(), 0);
        assert_eq!(
            window
                .get_picker_rows()
                .row_data(0)
                .unwrap()
                .cards
                .row_count(),
            2
        );
        window.invoke_choose_card("poppin_party".into());
        assert_eq!(window.get_picker_stage(), 1);
        assert_eq!(window.get_picker_rows().row_count(), 2);
        assert_eq!(
            window
                .get_picker_rows()
                .row_data(0)
                .unwrap()
                .cards
                .row_data(0)
                .unwrap()
                .key,
            "kasumi"
        );
        window.invoke_favorite_character("tae".into());
        window.set_character_filter(2);
        window.invoke_refresh_picker();
        assert_eq!(
            window
                .get_picker_rows()
                .row_data(0)
                .unwrap()
                .cards
                .row_count(),
            1
        );
        assert_eq!(
            window
                .get_picker_rows()
                .row_data(0)
                .unwrap()
                .cards
                .row_data(0)
                .unwrap()
                .key,
            "tae"
        );
        window.set_character_filter(0);
        window.set_search("香澄".into());
        window.invoke_refresh_picker();
        assert_eq!(
            window
                .get_picker_rows()
                .row_data(0)
                .unwrap()
                .cards
                .row_count(),
            1
        );
        window.invoke_choose_card("kasumi".into());
        assert_eq!(window.get_picker_stage(), 2);
        assert_eq!(window.get_picker_costumes().row_count(), 2);
        window.invoke_favorite_costume("casual-2023".into());
        window.set_costume_filter(2);
        window.invoke_refresh_picker();
        assert_eq!(
            window.get_picker_costumes().row_data(0).unwrap().id,
            "casual-2023"
        );
        window.set_costume_search("no-match".into());
        window.invoke_refresh_picker();
        assert_eq!(window.get_picker_costumes().row_count(), 0);
        window.invoke_picker_back();
        assert_eq!(window.get_picker_stage(), 1);
        window.invoke_picker_back();
        assert_eq!(window.get_picker_stage(), 0);
        assert_eq!(window.get_search(), "");

        if let Ok(path) = std::env::var("BANDORI_UI_QA_PORTRAIT") {
            let picture = slint::Image::load_from_path(Path::new(&path)).unwrap();
            for character in ["kasumi", "tae", "rimi", "saaya", "arisa"] {
                picker
                    .pictures
                    .borrow_mut()
                    .insert(character.into(), picture.clone());
            }
        }
        window.show().unwrap();
        for (width, height, scale) in [(1080., 620., 1.), (1180., 680., 1.5), (1500., 900., 1.)] {
            window
                .window()
                .dispatch_event(slint::platform::WindowEvent::ScaleFactorChanged {
                    scale_factor: scale,
                });
            window.window().set_size(slint::PhysicalSize::new(
                (width * scale) as u32,
                (height * scale) as u32,
            ));
            window
                .window()
                .dispatch_event(slint::platform::WindowEvent::Resized {
                    size: slint::LogicalSize::new(width, height),
                });
            for stage in 0..3 {
                window.invoke_start_picker();
                if stage > 0 {
                    window.invoke_choose_card("poppin_party".into());
                }
                if stage == 2 {
                    window.invoke_choose_card("kasumi".into());
                }
                crate::tests::settle_animations(&window);
                let snapshot = window.window().take_snapshot().unwrap();
                let picker = ElementHandle::find_by_element_type_name(&window, "ModelPicker")
                    .next()
                    .unwrap();
                let heading =
                    ElementHandle::find_by_element_id(&window, "ModelPicker::picker-heading")
                        .next()
                        .unwrap();
                assert!(heading.absolute_position().y >= picker.absolute_position().y);
                assert!(heading.size().height >= 36.);
                assert!(picker.size().width >= 500. && picker.size().height >= 400.);
                assert!(picker.absolute_position().x >= 210.);
                assert!(picker.absolute_position().x + picker.size().width <= width - 276.);
                if let Ok(output) = std::env::var("BANDORI_UI_QA_DIR") {
                    std::fs::create_dir_all(&output).unwrap();
                    image::save_buffer(
                        Path::new(&output).join(format!("picker-{stage}-{width}-{scale}.png")),
                        snapshot.as_bytes(),
                        snapshot.width(),
                        snapshot.height(),
                        image::ColorType::Rgba8,
                    )
                    .unwrap();
                }
            }
        }
    }
}
