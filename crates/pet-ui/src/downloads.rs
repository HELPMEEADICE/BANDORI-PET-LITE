//! Download page state; all HTTP, validation and index scans run on a worker.
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};

use pet_assets::catalog::{model_packages, Catalog, ModelPackage};
use pet_assets::download::{DownloadError, DownloadProgress, PackageDownloader};
use pet_assets::{index, ModelIndex};
use pet_config::Paths;
use pet_ipc::Message;
use slint::{ComponentHandle, ModelRc, VecModel};

use super::{picker::Picker, DownloadRow, Sender, SettingsWindow};

enum Request {
    Scan,
    Download(Vec<String>),
}

enum Event {
    Catalog(ModelIndex, Catalog),
    Progress(String, Option<DownloadProgress>),
    Installed(String, ModelIndex, Catalog),
    Failed(String, String),
    Finished {
        done: usize,
        failed: usize,
        cancelled: bool,
    },
}

pub(super) struct Downloads {
    packages: Vec<ModelPackage>,
    index: RefCell<ModelIndex>,
    catalog: Rc<RefCell<Catalog>>,
    picker: Rc<Picker>,
    status: RefCell<HashMap<String, (String, f32)>>,
    active: RefCell<String>,
    busy: Cell<bool>,
    cancel: Arc<AtomicBool>,
    requests: mpsc::Sender<Request>,
    events: mpsc::Receiver<Event>,
    send: Sender,
}

impl Downloads {
    pub fn new(
        paths: &Paths,
        index: &ModelIndex,
        catalog: Rc<RefCell<Catalog>>,
        picker: Rc<Picker>,
        send: Sender,
    ) -> std::io::Result<Rc<Self>> {
        let (requests, receiver) = mpsc::channel();
        let (events, event_rx) = mpsc::channel();
        let cancel = Arc::new(AtomicBool::new(false));
        let worker_cancel = Arc::clone(&cancel);
        let paths = paths.clone();
        let packages = model_packages(&paths.assets());
        std::thread::Builder::new()
            .name("bandori-model-downloads".into())
            .spawn(move || {
                while let Ok(request) = receiver.recv() {
                    match request {
                        Request::Scan => {
                            let index = index::load_or_scan(&paths.models());
                            let catalog = Catalog::build(&paths.assets(), &index);
                            if events.send(Event::Catalog(index, catalog)).is_err() {
                                break;
                            }
                        }
                        Request::Download(keys) => {
                            let downloader = PackageDownloader::new();
                            let mut done = 0;
                            let mut failed = 0;
                            for key in keys {
                                if worker_cancel.load(Ordering::Relaxed) {
                                    break;
                                }
                                if events.send(Event::Progress(key.clone(), None)).is_err() {
                                    return;
                                }
                                let result = match &downloader {
                                    Ok(downloader) => downloader.download(
                                        &key,
                                        &paths.models(),
                                        &worker_cancel,
                                        |progress| {
                                            let _ = events
                                                .send(Event::Progress(key.clone(), Some(progress)));
                                        },
                                    ),
                                    Err(error) => {
                                        failed += 1;
                                        let _ = events.send(Event::Failed(key, error.to_string()));
                                        continue;
                                    }
                                };
                                match result {
                                    Ok(_) => {
                                        done += 1;
                                        let index = index::load_or_scan(&paths.models());
                                        let catalog = Catalog::build(&paths.assets(), &index);
                                        if events
                                            .send(Event::Installed(key, index, catalog))
                                            .is_err()
                                        {
                                            return;
                                        }
                                    }
                                    Err(DownloadError::Cancelled) => {
                                        let _ = events.send(Event::Failed(
                                            key,
                                            "已取消，原有模型已保留".into(),
                                        ));
                                        break;
                                    }
                                    Err(error) => {
                                        failed += 1;
                                        let _ = events.send(Event::Failed(
                                            key,
                                            format!("{error}；可点击重试"),
                                        ));
                                    }
                                }
                            }
                            if events
                                .send(Event::Finished {
                                    done,
                                    failed,
                                    cancelled: worker_cancel.load(Ordering::Relaxed),
                                })
                                .is_err()
                            {
                                break;
                            }
                        }
                    }
                }
            })?;
        Ok(Rc::new(Self {
            packages,
            index: RefCell::new(index.clone()),
            catalog,
            picker,
            status: RefCell::new(HashMap::new()),
            active: RefCell::new(String::new()),
            busy: Cell::new(false),
            cancel,
            requests,
            events: event_rx,
            send,
        }))
    }

    pub fn wire(self: &Rc<Self>, window: &SettingsWindow) {
        let weak = window.as_weak();
        let this = self.clone();
        window.on_refresh_downloads(move || {
            if let Some(window) = weak.upgrade() {
                if !this.busy.replace(true) {
                    window.set_download_busy(true);
                    window.set_download_scanning(true);
                    window.set_download_notice("正在扫描本地模型…".into());
                    let _ = this.requests.send(Request::Scan);
                }
            }
        });
        let weak = window.as_weak();
        let this = self.clone();
        window.on_filter_downloads(move || {
            if let Some(window) = weak.upgrade() {
                this.refresh(&window);
            }
        });
        let weak = window.as_weak();
        let this = self.clone();
        window.on_download_package(move |key| {
            if let Some(window) = weak.upgrade() {
                if this
                    .packages
                    .iter()
                    .any(|package| package.key == key.as_str())
                {
                    this.start(&window, vec![key.to_string()]);
                }
            }
        });
        let weak = window.as_weak();
        let this = self.clone();
        window.on_download_batch(move |installed| {
            if let Some(window) = weak.upgrade() {
                let index = this.index.borrow();
                let keys = this
                    .packages
                    .iter()
                    .filter(|package| {
                        let is_installed = !index.costumes_of(&package.key).is_empty();
                        is_installed == installed
                    })
                    .map(|package| package.key.clone())
                    .collect();
                drop(index);
                this.start(&window, keys);
            }
        });
        let weak = window.as_weak();
        let this = self.clone();
        window.on_cancel_downloads(move || {
            this.cancel.store(true, Ordering::Relaxed);
            if let Some(window) = weak.upgrade() {
                window.set_download_notice("正在取消下载…".into());
            }
        });
        let weak = window.as_weak();
        let this = self.clone();
        window.on_use_package(move |key| {
            if let Some(window) = weak.upgrade() {
                if !this.index.borrow().costumes_of(&key).is_empty() {
                    this.picker.choose_downloaded(&window, &key);
                }
            }
        });
    }

    fn start(&self, window: &SettingsWindow, keys: Vec<String>) {
        if self.busy.get() {
            return;
        }
        if keys.is_empty() {
            window.set_download_notice("没有需要处理的模型包".into());
            return;
        }
        self.busy.set(true);
        self.cancel.store(false, Ordering::Relaxed);
        window.set_download_busy(true);
        window.set_download_notice(format!("准备下载 {} 个模型包…", keys.len()).into());
        let _ = self.requests.send(Request::Download(keys));
    }

    pub fn poll(&self, window: &SettingsWindow) {
        let mut refresh = false;
        while let Ok(event) = self.events.try_recv() {
            refresh = true;
            match event {
                Event::Catalog(index, catalog) => {
                    *self.index.borrow_mut() = index;
                    *self.catalog.borrow_mut() = catalog;
                    self.picker.catalog_changed(window);
                    self.busy.set(false);
                    window.set_download_busy(false);
                    window.set_download_scanning(false);
                    window.set_download_notice("本地模型已刷新".into());
                    (self.send)(&Message::ModelsChanged {
                        characters: Vec::new(),
                    });
                }
                Event::Progress(key, progress) => {
                    *self.active.borrow_mut() = key.clone();
                    let (text, percent) = match progress {
                        Some(p) => (
                            if p.bytes_per_second == 0.0 {
                                "下载完成，正在校验模型包…".into()
                            } else {
                                format!(
                                    "{} / {} · {}/s",
                                    size(p.downloaded),
                                    p.total.map(size).unwrap_or_else(|| "未知大小".into()),
                                    size(p.bytes_per_second as u64)
                                )
                            },
                            p.total
                                .map(|total| {
                                    (p.downloaded as f64 * 100. / total as f64).min(99.) as f32
                                })
                                .unwrap_or(-1.),
                        ),
                        None => ("正在连接下载源…".into(), -1.),
                    };
                    window.set_download_notice(format!("{key}：{text}").into());
                    self.status.borrow_mut().insert(key, (text, percent));
                }
                Event::Installed(key, index, catalog) => {
                    *self.index.borrow_mut() = index;
                    *self.catalog.borrow_mut() = catalog;
                    self.status
                        .borrow_mut()
                        .insert(key.clone(), ("已完成，可选择服装或再次更新".into(), 100.));
                    self.active.borrow_mut().clear();
                    self.picker.catalog_changed(window);
                    (self.send)(&Message::ModelsChanged {
                        characters: vec![key],
                    });
                }
                Event::Failed(key, error) => {
                    self.status.borrow_mut().insert(key, (error, 0.));
                    self.active.borrow_mut().clear();
                }
                Event::Finished {
                    done,
                    failed,
                    cancelled,
                } => {
                    self.busy.set(false);
                    window.set_download_busy(false);
                    window.set_download_notice(
                        format!(
                            "{}：成功 {done} 个，失败 {failed} 个",
                            if cancelled {
                                "下载已取消"
                            } else {
                                "下载结束"
                            }
                        )
                        .into(),
                    );
                }
            }
        }
        if refresh {
            self.refresh(window);
        }
    }

    pub fn refresh(&self, window: &SettingsWindow) {
        let index = self.index.borrow();
        let installed = index.characters();
        window.set_no_installed_models(installed.is_empty());
        window.set_download_summary(
            format!(
                "已安装 {} 个角色 · 可下载 {} 个角色",
                installed.len(),
                self.packages.len()
            )
            .into(),
        );
        let mut packages = self.packages.clone();
        for key in &installed {
            if !packages.iter().any(|package| &package.key == key) {
                packages.push(ModelPackage {
                    key: key.clone(),
                    display: key.clone(),
                    band_display: "自定义角色".into(),
                });
            }
        }
        let query = window.get_download_search().trim().to_lowercase();
        let mut previous_band = String::new();
        let mut rows = Vec::new();
        for package in packages {
            if !format!(
                "{} {} {}",
                package.key, package.display, package.band_display
            )
            .to_lowercase()
            .contains(&query)
            {
                continue;
            }
            let entry = index.archives.get(&format!("models/{}.zst", package.key));
            let costumes = index.costumes_of(&package.key).len();
            let available = self.packages.iter().any(|remote| remote.key == package.key);
            let detail = self
                .status
                .borrow()
                .get(&package.key)
                .cloned()
                .unwrap_or_else(|| {
                    (
                        if costumes > 0 {
                            format!(
                                "已安装 · {costumes} 套服装 · {}",
                                size(entry.map(|entry| entry.size).unwrap_or(0))
                            )
                        } else if entry.is_some() {
                            "模型包无法读取，请重新下载".into()
                        } else {
                            "尚未下载".into()
                        },
                        0.,
                    )
                });
            let group_start = previous_band != package.band_display;
            previous_band = package.band_display.clone();
            rows.push(DownloadRow {
                key: package.key.clone().into(),
                display: package.display.into(),
                band: package.band_display.into(),
                group_start,
                installed: costumes > 0,
                available,
                action: if entry.is_some() {
                    "更新 / 重试"
                } else {
                    "下载"
                }
                .into(),
                detail: detail.0.into(),
                progress: detail.1,
                active: *self.active.borrow() == package.key,
            });
        }
        window.set_download_rows(ModelRc::new(VecModel::from(rows)));
    }
}

impl Drop for Downloads {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}

fn size(bytes: u64) -> String {
    let mut amount = bytes as f64;
    for unit in ["B", "KB", "MB", "GB", "TB"] {
        if amount < 1024. || unit == "TB" {
            return format!("{amount:.1} {unit}");
        }
        amount /= 1024.;
    }
    unreachable!()
}

#[cfg(test)]
mod tests {
    use super::*;
    use i_slint_backend_testing::{ElementHandle, TestingBackend, TestingBackendOptions};
    use slint::Model;

    #[test]
    fn fresh_install_download_page_lists_remote_models_filters_and_exposes_download_actions() {
        slint::platform::set_platform(Box::new(TestingBackend::new(TestingBackendOptions {
            renderer_name: Some("software".into()),
            mock_time: true,
            ..Default::default()
        })))
        .unwrap();
        let root = tempfile::tempdir().unwrap();
        let paths = Paths::with_root(root.path());
        let window = SettingsWindow::new().unwrap();
        window.set_page(4);
        window.set_first_run(true);
        let catalog = Rc::new(RefCell::new(Catalog::default()));
        let send: Sender = Rc::new(|_| true);
        let picker = Picker::new(Rc::clone(&catalog), &paths, send.clone()).unwrap();
        let downloads =
            Downloads::new(&paths, &ModelIndex::default(), catalog, picker, send).unwrap();
        downloads.wire(&window);
        downloads.refresh(&window);
        assert!(window.get_no_installed_models());
        assert!(window.get_download_rows().row_count() >= 40);
        let selected = Rc::new(RefCell::new(String::new()));
        let result = Rc::clone(&selected);
        window.on_download_package(move |key| *result.borrow_mut() = key.to_string());
        window.show().unwrap();
        // Slint delivers the page-changed callback on the next event-loop tick.
        // Let its real background scan complete before exercising row actions.
        for _ in 0..100 {
            super::super::tests::settle_animations(&window);
            downloads.poll(&window);
            if !downloads.busy.get() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert!(!downloads.busy.get());
        for (width, height, scale, theme) in [
            (1080., 620., 1., 1),
            (1180., 680., 1., 0),
            (1180., 680., 1.5, 1),
        ] {
            window.set_theme(theme);
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
            super::super::tests::settle_animations(&window);
            let snapshot = window.window().take_snapshot().unwrap();
            for label in ["下载缺失", "更新已安装", "刷新", "打开目录", "下载"] {
                let button = ElementHandle::find_by_accessible_label(&window, label)
                    .next()
                    .unwrap_or_else(|| panic!("missing {label}"));
                let position = button.absolute_position();
                assert!(
                    position.x >= 210. && position.x + button.size().width <= width,
                    "{label} outside page"
                );
                assert!(
                    position.y >= 0. && position.y + button.size().height <= height,
                    "{label} outside page vertically"
                );
            }
            if let Ok(output) = std::env::var("BANDORI_UI_QA_DIR") {
                std::fs::create_dir_all(&output).unwrap();
                image::save_buffer(
                    std::path::Path::new(&output)
                        .join(format!("downloads-{width}-{scale}-{theme}.png")),
                    snapshot.as_bytes(),
                    snapshot.width(),
                    snapshot.height(),
                    image::ColorType::Rgba8,
                )
                .unwrap();
            }
        }
        ElementHandle::find_by_accessible_label(&window, "下载")
            .next()
            .unwrap()
            .invoke_accessible_default_action();
        assert_eq!(*selected.borrow(), "kasumi");
        window.set_download_search("MyGO".into());
        super::super::tests::settle_animations(&window);
        assert_eq!(window.get_download_rows().row_count(), 5);
        window.set_download_search("千早爱音".into());
        super::super::tests::settle_animations(&window);
        assert_eq!(window.get_download_rows().row_count(), 1);
        assert_eq!(window.get_download_rows().row_data(0).unwrap().key, "anon");
        window.set_download_search("不存在的角色".into());
        super::super::tests::settle_animations(&window);
        assert_eq!(window.get_download_rows().row_count(), 0);
    }
}
