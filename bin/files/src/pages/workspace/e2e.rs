//! Existing Files scenarios driven through the composed native UI and rendered to pixels.
use super::{
    model::{Operation, Prompt, Row},
    ui::{Browser, LaunchOptions},
};
use crate::entities::{
    file::ViewMode,
    location::{Location, places::Storage, volumes},
    preferences,
};
use anti_webui::{
    core::{
        Ui,
        access::{self, AccessNode},
        event::{ImeUpdate, KeyEvent, WindowEvent},
        geometry::{PointI, RectI, SizeI},
        platform::{PlatformRequest, place_popup},
    },
    prelude::*,
    wgpu::{Gpu, Renderer, wgpu::TextureFormat},
};
use anyhow::{Context, ensure};
use gds::{app::Gds, appearance::theme, service::task_manager::Supervisor};
use std::{
    cell::RefCell,
    fs,
    path::{Path, PathBuf},
    process::ExitCode,
    rc::Rc,
    sync::Arc,
    time::{Duration, Instant},
};

const WIDTH: u32 = 1100;
const HEIGHT: u32 = 760;
const DEADLINE: Duration = Duration::from_secs(20);

pub(crate) fn run() -> ExitCode {
    match Session::run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("Files driven UI scenario failed: {error:#}");
            ExitCode::FAILURE
        }
    }
}
struct Session {
    ui: Ui,
    browser: Browser,
    gpu: Gpu,
    renderer: Renderer,
    artifacts: PathBuf,
    viewport: SizeI,
    time: u32,
}
impl Session {
    fn run() -> anyhow::Result<()> {
        let artifacts =
            PathBuf::from(std::env::var_os("GDS_E2E_ARTIFACTS").context("E2E artifact directory")?);
        let home = PathBuf::from(std::env::var_os("HOME").context("isolated HOME")?);
        let supervisor = Supervisor::new(true)?;
        let scope = supervisor.scope();
        let storage = Storage::start(artifacts.join("places.sqlite"), &scope);
        let places = supervisor.block_on(storage.load(&scope))??;
        let browser_slot = Rc::new(RefCell::new(None));
        let mounted = browser_slot.clone();
        let gpu = Gpu::headless().map_err(|error| anyhow::anyhow!(error.to_string()))?;
        let renderer = Renderer::new(&gpu.device, TextureFormat::Rgba8Unorm);
        let mut ui = Ui::new(|| {
            let app = Gds::install(scope.clone(), None);
            app.i18n().install_catalog(
                crate::shared::i18n::prepare(gds::i18n::Language::English)
                    .expect("application messages"),
            );
            let preferences = preferences::Preferences {
                language: Some(gds::i18n::Language::English),
                animations_enabled: Some(false),
                appearance: theme::Appearance {
                    source: theme::Source::Builtin,
                    mode: theme::ModePreference::Dark,
                    ..Default::default()
                },
                ..Default::default()
            };
            app.appearance()
                .theme()
                .apply(&preferences.appearance)
                .expect("built-in theme");
            app.appearance().motion().set_enabled(false);
            app.ui().time().apply(preferences.time_format);
            preferences::Service::install(preferences, &scope).expect("isolated settings");
            let browser = Browser::new(
                LaunchOptions {
                    places: Ok(places),
                    start: home.join("work"),
                    home,
                    picker: None,
                },
                volumes::Service::new(scope.clone()),
                Rc::new(|_| panic!("scenario unexpectedly opened another window")),
            );
            *mounted.borrow_mut() = Some(browser.clone());
            browser.view().into_view()
        });
        ui.handle_event(WindowEvent::Resized {
            size: SizeI::new(WIDTH as i32, HEIGHT as i32),
            scale: Scale::ONE,
        });
        ui.handle_event(WindowEvent::Decorations(Decorations::Client));
        ui.handle_event(WindowEvent::Focused(true));
        let thread = std::thread::current();
        anti_webui::core::channel::set_thread_waker(Arc::new(move || thread.unpark()));
        let mut session = Self {
            ui,
            browser: browser_slot.borrow_mut().take().expect("mounted browser"),
            gpu,
            renderer,
            artifacts,
            viewport: SizeI::new(WIDTH as i32, HEIGHT as i32),
            time: 0,
        };
        let result = session.workflow();
        if result.is_err() {
            let _ = session.capture("failure");
        }
        session.ui.close();
        drop(session.browser);
        supervisor.shutdown();
        result
    }
    fn pump(&mut self) {
        self.ui.poll_images();
        let now = Instant::now();
        self.ui.tick(now);
        self.ui.animate(now);
        for (window, command) in anti_webui::core::window::take_commands() {
            if window == self.ui.window() {
                self.ui.apply(command);
            }
        }
        self.ui.update();
        loop {
            let mut configured = false;
            for request in self.ui.take_requests() {
                if let PlatformRequest::ShowPopup(popup) | PlatformRequest::UpdatePopup(popup) =
                    request
                {
                    let bounds = place_popup(
                        popup.placement,
                        popup.anchor,
                        popup.gap,
                        popup.wanted,
                        RectI::new(0, 0, self.viewport.width, self.viewport.height),
                    );
                    self.ui.handle_event(WindowEvent::PopupConfigured {
                        id: popup.id,
                        origin: popup.position.unwrap_or_else(|| {
                            PointI::new(bounds.x - popup.geometry.x, bounds.y - popup.geometry.y)
                        }),
                        size: SizeI::new(bounds.width, bounds.height),
                    });
                    configured = true;
                }
            }
            if !configured {
                break;
            }
            self.ui.update();
        }
    }
    fn wait_for(
        &mut self,
        description: &str,
        predicate: impl Fn(&Browser) -> bool,
    ) -> anyhow::Result<()> {
        let deadline = Instant::now() + DEADLINE;
        loop {
            self.pump();
            if predicate(&self.browser) {
                return Ok(());
            }
            ensure!(
                Instant::now() < deadline,
                "timed out waiting for {description}; notice: {:?}",
                self.browser.0.notice.get_untracked()
            );
            let now = Instant::now();
            let mut wake = self.ui.next_deadline(now).unwrap_or(deadline).min(deadline);
            if self.ui.animating() {
                wake = wake.min(now + Duration::from_millis(16));
            }
            std::thread::park_timeout(wake.saturating_duration_since(now));
        }
    }
    fn event(&mut self, event: WindowEvent) {
        if let WindowEvent::Resized { size, .. } = &event {
            self.viewport = *size;
        }
        self.ui.handle_event(event);
        self.pump();
    }
    fn key(&mut self, key: Key, modifiers: Modifiers) {
        self.event(WindowEvent::Key(KeyEvent {
            key,
            text: None,
            modifiers,
            pressed: true,
            repeat: false,
        }));
    }
    fn shortcut(&mut self, key: char, shift: bool) {
        self.key(
            Key::Character(key.to_string()),
            Modifiers {
                ctrl: true,
                shift,
                ..Modifiers::default()
            },
        );
    }
    fn named(&mut self, key: NamedKey) {
        self.key(Key::Named(key), Modifiers::default());
    }
    fn input(&mut self, value: &str) {
        self.event(WindowEvent::Ime(ImeUpdate {
            commit: Some(value.to_owned()),
            ..Default::default()
        }));
    }
    fn replace_input(&mut self, value: &str) {
        self.shortcut('a', false);
        self.named(NamedKey::Backspace);
        self.input(value);
    }
    fn accessible(&mut self) -> access::AccessTree {
        self.pump();
        access::snapshot()
    }
    fn node(&mut self, label: &str) -> anyhow::Result<AccessNode> {
        self.accessible()
            .nodes
            .into_iter()
            .find(|node| {
                node.label.as_deref() == Some(label)
                    && node.bounds.width > 0.0
                    && node.bounds.height > 0.0
            })
            .with_context(|| format!("visible accessible item {label:?}"))
    }
    fn click_at(&mut self, point: PointF, button: PointerButton) {
        self.event(WindowEvent::PointerMoved {
            position: point,
            modifiers: Modifiers::default(),
        });
        for pressed in [true, false] {
            self.time += 20;
            self.event(WindowEvent::PointerButton {
                position: point,
                button,
                pressed,
                modifiers: Modifiers::default(),
                time: self.time,
            });
        }
    }
    fn click(&mut self, label: &str) -> anyhow::Result<()> {
        let bounds = self.node(label)?.bounds;
        self.click_at(
            PointF::new(
                bounds.x + bounds.width / 2.0,
                bounds.y + bounds.height / 2.0,
            ),
            PointerButton::Primary,
        );
        Ok(())
    }
    fn click_tab(&mut self, label: &str) -> anyhow::Result<()> {
        self.pump();
        let region = self
            .browser
            .0
            .tabs_node
            .logical_rect()
            .context("tab region bounds")?;
        let tree = self.accessible();
        let target = tree
            .nodes
            .iter()
            .find(|node| {
                node.role == Role::Button
                    && node.label.as_deref() == Some(label)
                    && node.bounds.x >= region.x
                    && node.bounds.right() <= region.right()
                    && node.bounds.y >= region.y
                    && node.bounds.bottom() <= region.bottom()
            })
            .context("tab control")?
            .bounds;
        self.click_at(
            PointF::new(
                target.x + target.width / 2.0,
                target.y + target.height / 2.0,
            ),
            PointerButton::Primary,
        );
        Ok(())
    }
    fn has_file(browser: &Browser, name: &str) -> bool {
        browser.read(|state| {
            matches!(state.operation, Operation::Idle)
                && !state.tab().navigation.is_loading()
                && state
                    .tab()
                    .rows
                    .iter()
                    .any(|row| matches!(row,Row::File(entry) if entry.name==name))
        })
    }
    fn wait_file(&mut self, name: &str) -> anyhow::Result<()> {
        self.wait_for(name, |browser| Self::has_file(browser, name))
    }
    fn wait_prompt_closed(&mut self) -> anyhow::Result<()> {
        self.wait_for("modal focus restored", |_| {
            !access::snapshot()
                .nodes
                .iter()
                .any(|node| node.role == Role::Dialog)
        })
    }
    fn capture(&mut self, name: &str) -> anyhow::Result<()> {
        eprintln!("Driven UI checkpoint: {name}");
        self.pump();
        let directory = self.artifacts.join("driven").join(name);
        fs::create_dir_all(&directory)?;
        fs::write(
            directory.join("accessibility.txt"),
            format!("{:#?}", access::snapshot()),
        )?;
        let frame = self.ui.paint_frame(Instant::now());
        self.capture_scene(&directory.join("frame.png"), &frame.window)?;
        for (index, (_, scene)) in frame.popups.iter().enumerate() {
            self.capture_scene(&directory.join(format!("popup-{index}.png")), scene)?;
        }
        Ok(())
    }
    fn capture_scene(
        &mut self,
        path: &Path,
        scene: &anti_webui::core::scene::Scene,
    ) -> anyhow::Result<()> {
        let pixels = self.gpu.render_to_rgba(&mut self.renderer, scene);
        anti_webui::app::headless::write_png(path, scene.width, scene.height, &pixels)
            .map_err(|error| anyhow::anyhow!(error.to_string()))
    }
    fn workflow(&mut self) -> anyhow::Result<()> {
        self.wait_file("original.txt")?;
        self.capture("01-open")?;
        self.click("original.txt")?;
        self.named(NamedKey::F(2));
        self.wait_for("rename dialog", |browser| {
            matches!(browser.0.prompt.get_untracked(), Prompt::Name { .. })
        })?;
        self.replace_input("renamed.txt");
        self.named(NamedKey::Enter);
        self.wait_file("renamed.txt")?;
        self.wait_prompt_closed()?;
        ensure!(
            fs::read(self.artifacts.join("home/work/renamed.txt"))? == b"original payload\n",
            "rename preserves payload"
        );
        ensure!(
            !self.artifacts.join("home/work/original.txt").exists(),
            "rename removed old name"
        );
        self.capture("02-renamed")?;
        self.shortcut('z', false);
        self.wait_file("original.txt")?;
        self.shortcut('z', true);
        self.wait_file("renamed.txt")?;
        self.shortcut('n', true);
        self.wait_for("new folder dialog", |browser| {
            matches!(browser.0.prompt.get_untracked(), Prompt::Name { .. })
        })?;
        self.replace_input("created");
        self.named(NamedKey::Enter);
        self.wait_file("created")?;
        self.wait_prompt_closed()?;
        ensure!(
            self.artifacts.join("home/work/created").is_dir(),
            "new directory exists"
        );
        self.click("renamed.txt")?;
        self.named(NamedKey::Delete);
        self.wait_for("trash operation", |browser| {
            browser.read(|state| matches!(state.operation, Operation::Idle))
                && !Self::has_file(browser, "renamed.txt")
        })?;
        ensure!(
            !self.artifacts.join("home/work/renamed.txt").exists(),
            "trashed file leaves directory"
        );
        self.shortcut('z', false);
        self.wait_file("renamed.txt")?;
        ensure!(
            fs::read(self.artifacts.join("home/work/renamed.txt"))? == b"original payload\n",
            "restore preserves payload"
        );
        self.capture("03-restored")?;
        fs::write(
            self.artifacts.join("home/work/download.txt"),
            b"external download\n",
        )?;
        self.wait_file("download.txt")?;
        self.capture("04-watched-create")?;
        self.shortcut('g', false);
        self.wait_for("grid layout", |browser| {
            browser.read(|state| state.tab().mode == ViewMode::Grid)
        })?;
        self.capture("05-grid")?;
        self.reorder_tabs()?;
        self.search_on_typing()?;
        self.search_result_navigation()?;
        self.location_history()?;
        fs::write(
            self.artifacts.join("driven-passed.txt"),
            "PASS (native Ui event dispatch + GPU renderer): rename, undo, redo, create folder, trash, restore, filesystem watch, grid, tab reorder, search on typing, IME protocol, search result navigation, mouse location history\n",
        )?;
        Ok(())
    }
    fn reorder_tabs(&mut self) -> anyhow::Result<()> {
        self.shortcut('n', false);
        self.wait_for("second tab", |browser| {
            browser.read(|state| state.tabs.len() == 2)
        })?;
        let ids = self
            .browser
            .read(|state| state.tabs.iter().map(|tab| tab.id).collect::<Vec<_>>());
        self.pump();
        let tab_bounds = self
            .browser
            .0
            .tabs_node
            .logical_rect()
            .context("tab region bounds")?;
        let tree = self.accessible();
        let tabs = tree
            .nodes
            .into_iter()
            .filter(|node| {
                node.role == Role::Button
                    && node.label.as_deref() == Some("work")
                    && node.bounds.x >= tab_bounds.x
                    && node.bounds.right() <= tab_bounds.right()
                    && node.bounds.y >= tab_bounds.y
                    && node.bounds.bottom() <= tab_bounds.bottom()
            })
            .collect::<Vec<_>>();
        ensure!(tabs.len() == 2, "both tab controls expose their names");
        let source = tabs[0].bounds;
        let target = tabs[1].bounds;
        let start = PointF::new(
            source.x + source.width / 2.0,
            source.y + source.height / 2.0,
        );
        let end = PointF::new(target.x + target.width / 2.0, target.bottom() - 2.0);
        self.time += 20;
        self.event(WindowEvent::PointerButton {
            position: start,
            button: PointerButton::Primary,
            pressed: true,
            modifiers: Modifiers::default(),
            time: self.time,
        });
        for step in 1..=10 {
            let ratio = step as f32 / 10.0;
            self.event(WindowEvent::PointerMoved {
                position: PointF::new(
                    start.x + (end.x - start.x) * ratio,
                    start.y + (end.y - start.y) * ratio,
                ),
                modifiers: Modifiers::default(),
            });
        }
        self.time += 20;
        self.event(WindowEvent::PointerButton {
            position: end,
            button: PointerButton::Primary,
            pressed: false,
            modifiers: Modifiers::default(),
            time: self.time,
        });
        self.wait_for("tab reorder", |browser| {
            browser.read(|state| state.tabs[0].id == ids[1])
        })?;
        self.click_tab("work")?;
        self.shortcut('f', false);
        self.input("download");
        ensure!(
            self.browser.0.filter.get_untracked() == "download",
            "tab activation preserves immediate keyboard shortcuts"
        );
        self.replace_input("");
        self.capture("06-reordered-tabs")
    }
    fn search_on_typing(&mut self) -> anyhow::Result<()> {
        self.click_tab("work")?;
        self.input("x");
        ensure!(
            self.browser.0.filter.get_untracked().is_empty(),
            "typing search starts disabled"
        );
        self.browser
            .0
            .preferences
            .edit(|preferences| preferences.files.auto_focus_search = true)?;
        self.click_tab("work")?;
        self.input("ab");
        ensure!(
            self.browser.0.filter.get_untracked() == "ab",
            "committed text transfers from list to search"
        );
        self.capture("07-type-to-search")?;
        self.replace_input("");
        self.click_tab("work")?;
        self.event(WindowEvent::Ime(ImeUpdate {
            preedit: Some(("zhong".into(), Some((5, 5)))),
            ..Default::default()
        }));
        ensure!(
            self.browser.0.filter_node.is_composing(),
            "list preedit delegates to search"
        );
        self.input("中");
        ensure!(
            self.browser.0.filter.get_untracked() == "中",
            "IME commits native text once"
        );
        ensure!(
            !self.browser.0.filter_node.is_composing(),
            "IME commit clears preedit"
        );
        self.capture("08-search-ime")
    }
    fn search_result_navigation(&mut self) -> anyhow::Result<()> {
        self.replace_input(".txt");
        self.wait_for("filtered file results", |browser| {
            browser.read(|state| state.tab().rows.len() == 2)
        })?;
        self.named(NamedKey::ArrowDown);
        self.assert_cursor(0)?;
        self.named(NamedKey::ArrowDown);
        self.assert_cursor(1)?;
        self.shortcut('f', false);
        self.named(NamedKey::ArrowUp);
        self.assert_cursor(1)?;
        self.shortcut('f', false);
        self.replace_input("missing-result");
        self.wait_for("empty results", |browser| {
            browser.read(|state| state.tab().rows.is_empty())
        })?;
        self.named(NamedKey::ArrowDown);
        self.replace_input("");
        self.wait_file("download.txt")?;
        self.event(WindowEvent::Ime(ImeUpdate {
            preedit: Some(("down".into(), Some((4, 4)))),
            ..Default::default()
        }));
        self.named(NamedKey::ArrowDown);
        self.named(NamedKey::ArrowUp);
        ensure!(
            self.browser.0.filter_node.is_composing(),
            "candidate arrows preserve composition"
        );
        self.input("download");
        self.wait_for("committed search", |browser| {
            browser.read(|state| state.tab().rows.len() == 1)
        })?;
        self.named(NamedKey::ArrowDown);
        self.assert_cursor(0)?;
        self.shortcut('f', false);
        self.replace_input("created");
        self.wait_file("created")?;
        self.named(NamedKey::ArrowDown);
        self.named(NamedKey::Enter);
        self.wait_for("open search folder", |browser| {
            browser.read(|state| {
                state
                    .tab()
                    .navigation
                    .path()
                    .is_some_and(|path| path.ends_with("work/created"))
            })
        })?;
        self.capture("09-search-open-folder")
    }
    fn assert_cursor(&self, index: usize) -> anyhow::Result<()> {
        ensure!(
            self.browser
                .read(|state| state.tab().rows.get(index).is_some_and(|row| state
                    .tab()
                    .navigation
                    .view()
                    .selection
                    .contains(&row.key()))),
            "search arrow selected result {index}"
        );
        Ok(())
    }
    fn open_address(&mut self, address: &str) -> anyhow::Result<()> {
        self.shortcut('l', false);
        self.replace_input(address);
        self.named(NamedKey::Enter);
        self.wait_address(address)
    }
    fn wait_address(&mut self, address: &str) -> anyhow::Result<()> {
        let expected = Location::from_uri(address)?;
        self.wait_for(address, |browser| {
            browser.read(|state| {
                state.tab().navigation.location() == &expected
                    && !state.tab().navigation.is_loading()
            })
        })
    }
    fn mouse_history(&mut self, button: u32, address: &str) -> anyhow::Result<()> {
        self.click_at(PointF::new(700.0, 75.0), PointerButton::Other(button));
        self.wait_address(address)
    }
    fn location_history(&mut self) -> anyhow::Result<()> {
        self.open_address("tags:/")?;
        self.open_address("tags:/工作")?;
        self.open_address("tags:/工作/设计")?;
        self.mouse_history(8, "tags:/工作")?;
        self.mouse_history(8, "tags:/")?;
        self.mouse_history(9, "tags:/工作")?;
        self.mouse_history(9, "tags:/工作/设计")?;
        self.open_address("location:/recent")?;
        self.mouse_history(8, "tags:/工作/设计")?;
        self.mouse_history(9, "location:/recent")?;
        self.capture("10-location-mouse-history")
    }
}
