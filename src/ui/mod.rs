mod dialogs;
mod panels;

use crate::{
    app::{Session, ViewMode},
    calendar::{self, CalendarProvider, DateInfo, MonthGrid},
    export::{ExportJob, Progress},
    platform::{self, Clock, FixedClock, SystemClock},
    storage::{MemoRepository, Settings, Store, VerifiedBackup},
};
use anyhow::{Context, Result, bail};
use chrono::{DateTime, Datelike, NaiveDate};
use eframe::egui::{self, Color32, TextureHandle, ViewportCommand, WindowLevel};
use std::{path::PathBuf, sync::atomic::Ordering, time::Duration};

pub(super) const BG: Color32 = Color32::from_rgb(0xEE, 0xF2, 0xF2);
pub(super) const PAPER: Color32 = Color32::from_rgb(0xF9, 0xFB, 0xFA);
pub(super) const INK: Color32 = Color32::from_rgb(0x32, 0x5A, 0x6B);
pub(super) const BORDER: Color32 = Color32::from_rgb(0xBD, 0xC9, 0xCA);
pub(super) const SELECT: Color32 = Color32::from_rgb(0xE5, 0xED, 0xF1);
pub(super) const RED: Color32 = Color32::from_rgb(0xB8, 0x33, 0x29);
pub(super) const GREEN: Color32 = Color32::from_rgb(0x35, 0x78, 0x38);
const LAYOUT_SIZE: egui::Vec2 = egui::vec2(840.0, 640.0);
const APP_ICON_PNG: &[u8] = include_bytes!("..\\..\\assets\\icons\\512.png");

#[derive(Default)]
struct WindowSizing {
    zoom: Option<f32>,
    resize_pending: bool,
}

impl WindowSizing {
    fn sync(&mut self, ctx: &egui::Context) {
        let zoom = ctx.zoom_factor();
        if self.zoom != Some(zoom) {
            self.zoom = Some(zoom);
            self.resize_pending = true;
            // Zoom takes effect before viewport coordinates catch up on the next pass.
            ctx.request_repaint();
        } else if std::mem::take(&mut self.resize_pending) {
            let size = ctx.input(|input| {
                input
                    .viewport()
                    .monitor_size
                    .map_or(LAYOUT_SIZE, |monitor| {
                        let available = monitor - egui::vec2(48.0, 96.0) / zoom;
                        LAYOUT_SIZE.min(available.max(egui::Vec2::splat(1.0)))
                    })
            });
            // Viewport commands already include the active zoom and native DPI scale.
            ctx.send_viewport_cmd(ViewportCommand::InnerSize(size));
        }
    }
}

pub(super) enum Action {
    Select(NaiveDate),
    Month(i32),
    SetMonth(i32, u32),
    Today,
    ToggleDisplay,
    Mode(ViewMode),
    Save,
    Top,
    About,
    Help,
    Almanac,
    Tools,
    Exit,
}

pub(super) struct ImportWindow {
    directory: Option<PathBuf>,
    mapping: Option<PathBuf>,
    base: Option<PathBuf>,
    mapping_encoding: String,
    plan: Option<crate::migration::ImportPlan>,
}

pub(super) struct CalendarApp {
    session: Session<Store>,
    provider: CalendarProvider,
    grid: MonthGrid,
    selected_info: DateInfo,
    today_info: DateInfo,
    settings: Settings,
    clock: Box<dyn Clock>,
    resources: PathBuf,
    data: PathBuf,
    action: Option<Action>,
    error: Option<String>,
    about: bool,
    help: bool,
    tools: bool,
    almanac: bool,
    import_open: bool,
    import: ImportWindow,
    restore: Option<VerifiedBackup>,
    report: Option<String>,
    image: Option<TextureHandle>,
    image_error: Option<String>,
    header: Option<TextureHandle>,
    export_year: i32,
    job: Option<ExportJob>,
    export_status: String,
    last_export: Option<PathBuf>,
    close_pending: bool,
    close_wait: bool,
    allow_close: bool,
    first_frame: bool,
    window_sizing: WindowSizing,
    screenshot: Option<PathBuf>,
    frames: u32,
}

pub fn run() -> Result<()> {
    let mut resource_dir = platform::resources()?;
    let mut user_dir = platform::data_directory()?;
    let mut clock: Box<dyn Clock> = Box::new(SystemClock);
    let mut screenshot = None;
    let mut initial_selected = None;
    let mut initial_mode = None;
    let mut args = std::env::args_os().skip(1);
    while let Some(flag) = args.next() {
        let value = args.next().context("啟動參數缺少值")?;
        match flag.to_str() {
            Some("--resources") => resource_dir = PathBuf::from(value),
            Some("--data-dir") => user_dir = PathBuf::from(value),
            Some("--fixed-time") => {
                let value = value.to_str().context("固定時間不是 Unicode")?;
                clock = Box::new(FixedClock(
                    DateTime::parse_from_rfc3339(value).context("固定時間須為含時區 RFC3339")?,
                ));
            }
            Some("--screenshot") => screenshot = Some(PathBuf::from(value)),
            Some("--selected-date") => {
                initial_selected = Some(
                    NaiveDate::parse_from_str(
                        value.to_str().context("選日不是 Unicode")?,
                        "%Y-%m-%d",
                    )
                    .context("選日須為 YYYY-MM-DD")?,
                );
            }
            Some("--view") => {
                initial_mode = Some(match value.to_str() {
                    Some("month") => ViewMode::Month,
                    Some("day") => ViewMode::Day,
                    Some("export") => ViewMode::YearExport,
                    _ => bail!("檢視模式須為 month／day／export"),
                });
            }
            _ => bail!("未知啟動參數"),
        }
    }
    let store = Store::open(&user_dir)?;
    let settings = store.settings()?;
    let data = resource_dir.join("assets").join("calendar-data");
    crate::data::verify_package(&data).context("離線來源不完整；請重新解壓完整發布包")?;
    let mut provider = CalendarProvider::open(&data)?;
    provider.supplemental = settings.supplemental_events;
    let today = clock.now().date_naive();
    let mut session = Session::new(store, today)?;
    if let Some(selected) = initial_selected {
        session.select(selected)?;
    }
    if let Some(mode) = initial_mode {
        session.set_mode(mode)?;
    }
    let grid = provider.month(session.selected.year(), session.selected.month())?;
    let selected_info = provider.info(session.selected);
    let today_info = provider.info(today);
    let icon = eframe::icon_data::from_png_bytes(APP_ICON_PNG).context("無法載入程式圖示")?;
    let mut viewport = egui::ViewportBuilder::default()
        .with_icon(icon)
        .with_inner_size(LAYOUT_SIZE)
        .with_min_inner_size([360.0, 280.0])
        .with_resizable(false)
        .with_maximize_button(false);
    if let Some(position) = settings.window_position
        && let Some(position) = platform::visible_position(position)
    {
        viewport = viewport.with_position(position);
    }
    let options = eframe::NativeOptions {
        viewport,
        persist_window: false,
        ..Default::default()
    };
    eframe::run_native(
        "CalendarRust",
        options,
        Box::new(move |cc| {
            platform::install_fonts(&cc.egui_ctx)?;
            let mut visuals = egui::Visuals::light();
            visuals.panel_fill = BG;
            visuals.window_fill = PAPER;
            visuals.override_text_color = Some(INK);
            visuals.selection.bg_fill = SELECT;
            cc.egui_ctx.set_visuals(visuals);
            cc.egui_ctx.set_zoom_factor(settings.zoom);
            let mut app = CalendarApp {
                export_year: session.selected.year(),
                session,
                provider,
                grid,
                selected_info,
                today_info,
                settings,
                clock,
                resources: resource_dir,
                data,
                action: None,
                error: None,
                about: false,
                help: false,
                tools: false,
                almanac: false,
                import_open: false,
                import: ImportWindow {
                    directory: None,
                    mapping: None,
                    base: None,
                    mapping_encoding: "自動".into(),
                    plan: None,
                },
                restore: None,
                report: None,
                image: None,
                image_error: None,
                header: None,
                job: None,
                export_status: "選擇年份、七項配色與可選抬頭圖，再輸出 XLSX".into(),
                last_export: None,
                close_pending: false,
                close_wait: false,
                allow_close: false,
                first_frame: true,
                window_sizing: WindowSizing::default(),
                screenshot,
                frames: 0,
            };
            app.reload_images(&cc.egui_ctx);
            app.apply_top(&cc.egui_ctx);
            Ok(Box::new(app))
        }),
    )
    .map_err(|error| anyhow::anyhow!("{error}"))
}

impl CalendarApp {
    fn source_version(&self) -> &str {
        &self.provider.lunar_data.manifest.adopted_sha256
    }

    fn apply_top(&self, ctx: &egui::Context) {
        ctx.send_viewport_cmd(ViewportCommand::WindowLevel(
            if self.settings.always_on_top {
                WindowLevel::AlwaysOnTop
            } else {
                WindowLevel::Normal
            },
        ));
    }

    fn persist_settings(&mut self) -> Result<()> {
        self.session.repository.save_settings(&self.settings)
    }

    fn texture(ctx: &egui::Context, path: &std::path::Path, label: &str) -> Result<TextureHandle> {
        let image = image::open(path)?.thumbnail(512, 512).into_rgba8();
        let size = [image.width() as usize, image.height() as usize];
        let image = egui::ColorImage::from_rgba_unmultiplied(size, image.as_raw());
        Ok(ctx.load_texture(label, image, egui::TextureOptions::LINEAR))
    }

    fn reload_images(&mut self, ctx: &egui::Context) {
        self.image = None;
        self.image_error = None;
        let result = (|| -> Result<TextureHandle> {
            let path =
                if let Some(id) = self.session.repository.daily_asset(self.session.selected)? {
                    self.session.repository.asset_path(&id)?
                } else {
                    let lunar = self
                        .selected_info
                        .lunar
                        .as_ref()
                        .map_err(|error| anyhow::anyhow!("{error}"))?;
                    let (_, file) = crate::data::ZODIAC
                        .iter()
                        .find(|(name, _)| *name == lunar.zodiac)
                        .context("未知生肖，沒有預設龍圖")?;
                    let path = self
                        .resources
                        .join("assets")
                        .join("artwork")
                        .join(format!("{file}.png"));
                    let expected = self
                        .provider
                        .lunar_data
                        .manifest
                        .artwork
                        .iter()
                        .find(|item| item["zodiac"].as_str() == Some(&lunar.zodiac))
                        .and_then(|item| item["derived_sha256"].as_str())
                        .context("生肖素材清單缺漏")?;
                    anyhow::ensure!(
                        crate::data::digest(&std::fs::read(&path)?) == expected,
                        "生肖素材雜湊不符"
                    );
                    path
                };
            Self::texture(ctx, &path, "selected-day-image")
        })();
        match result {
            Ok(image) => self.image = Some(image),
            Err(error) => self.image_error = Some(format!("{error:#}")),
        }
        self.header = None;
        if let Some(id) = &self.settings.header_asset {
            match self
                .session
                .repository
                .asset_path(id)
                .and_then(|path| Self::texture(ctx, &path, "header-image"))
            {
                Ok(texture) => self.header = Some(texture),
                Err(error) => self.error = Some(format!("抬頭圖片無法載入：{error:#}")),
            }
        }
    }

    fn refresh(&mut self, ctx: &egui::Context) -> Result<()> {
        self.grid = self
            .provider
            .month(self.session.selected.year(), self.session.selected.month())?;
        self.selected_info = self.provider.info(self.session.selected);
        self.today_info = self.provider.info(self.session.today);
        self.reload_images(ctx);
        Ok(())
    }

    fn execute(&mut self, action: Action, ctx: &egui::Context) -> Result<()> {
        let previous = self.session.selected;
        match action {
            Action::Select(date) => self.session.select(date)?,
            Action::Month(delta) => self.session.navigate(delta)?,
            Action::SetMonth(year, month) => self.session.set_month(year, month)?,
            Action::Today => self.session.go_today()?,
            Action::ToggleDisplay => {
                self.session.flush()?;
                self.settings.display = match self.settings.display {
                    crate::storage::CalendarDisplay::Roc => {
                        crate::storage::CalendarDisplay::Gregorian
                    }
                    crate::storage::CalendarDisplay::Gregorian => {
                        crate::storage::CalendarDisplay::Roc
                    }
                };
                self.persist_settings()?;
            }
            Action::Mode(mode) => self.session.set_mode(mode)?,
            Action::Save => {
                self.session.flush()?;
                self.persist_settings()?;
            }
            Action::Top => {
                self.settings.always_on_top = !self.settings.always_on_top;
                self.apply_top(ctx);
                self.persist_settings()?;
            }
            Action::About => self.about = true,
            Action::Help => self.help = true,
            Action::Tools => {
                self.session.flush()?;
                self.persist_settings()?;
                self.tools = true;
            }
            Action::Almanac => self.almanac = true,
            Action::Exit => self.request_close(ctx)?,
        }
        if previous != self.session.selected {
            self.refresh(ctx)?;
        }
        Ok(())
    }

    fn request_close(&mut self, ctx: &egui::Context) -> Result<()> {
        self.session.flush()?;
        self.persist_settings()?;
        if self.job.is_some() {
            self.close_pending = true;
        } else {
            self.allow_close = true;
            ctx.send_viewport_cmd(ViewportCommand::Close);
        }
        Ok(())
    }

    fn poll_export(&mut self) {
        let mut terminal = false;
        if let Some(job) = &mut self.job {
            loop {
                match job.receiver.try_recv() {
                    Ok(Progress::Month(month)) => job.month = month,
                    Ok(Progress::Complete(path)) => {
                        self.export_status = format!("已完成：{}", path.display());
                        self.last_export = Some(path);
                        terminal = true;
                        break;
                    }
                    Ok(Progress::Cancelled) => {
                        self.export_status = "匯出已取消，原目的檔未變更".into();
                        terminal = true;
                        break;
                    }
                    Ok(Progress::Failed(error)) => {
                        self.error = Some(error.clone());
                        self.export_status = error;
                        terminal = true;
                        break;
                    }
                    Err(std::sync::mpsc::TryRecvError::Empty) => break,
                    Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                        self.error = Some("匯出工作非預期結束；未宣稱成功，請檢查目的檔".into());
                        terminal = true;
                        break;
                    }
                }
            }
        }
        if terminal
            && let Some(mut job) = self.job.take()
            && let Some(handle) = job.handle.take()
            && handle.join().is_err()
        {
            self.error = Some("匯出執行緒異常終止".into());
        }
    }

    fn shortcuts(&mut self, ctx: &egui::Context) {
        let text_editing = ctx
            .memory(|memory| memory.focused())
            .is_some_and(|id| egui::TextEdit::load_state(ctx, id).is_some());
        let ime = ctx.input(|input| {
            input
                .events
                .iter()
                .any(|event| matches!(event, egui::Event::Ime(_)))
        });
        let command = ctx.input_mut(|input| {
            let ctrl = egui::Modifiers::CTRL;
            if input.consume_key(ctrl, egui::Key::S) {
                return Some(Action::Save);
            }
            if input.consume_key(ctrl, egui::Key::Q) {
                return Some(Action::Exit);
            }
            if text_editing || ime {
                return None;
            }
            let bindings = [
                (egui::Key::F1, Action::Month(-12)),
                (egui::Key::F2, Action::Month(12)),
                (egui::Key::F3, Action::Month(-1)),
                (egui::Key::F4, Action::Month(1)),
                (egui::Key::F5, Action::Today),
                (egui::Key::F6, Action::ToggleDisplay),
                (
                    egui::Key::F7,
                    Action::Mode(if self.session.mode == ViewMode::Month {
                        ViewMode::Day
                    } else {
                        ViewMode::Month
                    }),
                ),
                (egui::Key::T, Action::Top),
                (egui::Key::I, Action::Help),
                (egui::Key::A, Action::About),
            ];
            bindings
                .into_iter()
                .find_map(|(key, action)| input.consume_key(ctrl, key).then_some(action))
        });
        if command.is_some() {
            self.action = command;
        }
    }

    fn context_menu(&mut self, ui: &mut egui::Ui) {
        if let Some(action) = command_menu(ui, self.settings.always_on_top, self.session.selected) {
            self.action = Some(action);
        }
    }
}

fn command_menu(ui: &mut egui::Ui, mut always_on_top: bool, selected: NaiveDate) -> Option<Action> {
    let mut result = None;
    for (label, action) in [
        ("上一年　Ctrl+F1", Action::Month(-12)),
        ("下一年　Ctrl+F2", Action::Month(12)),
        ("上一月　Ctrl+F3", Action::Month(-1)),
        ("下一月　Ctrl+F4", Action::Month(1)),
        ("今天　Ctrl+F5", Action::Today),
        ("民國／西元　Ctrl+F6", Action::ToggleDisplay),
        ("月曆　Ctrl+F7", Action::Mode(ViewMode::Month)),
        ("日曆", Action::Mode(ViewMode::Day)),
        ("輸出年曆", Action::Mode(ViewMode::YearExport)),
        ("農民曆", Action::Almanac),
        ("設定／備份／匯入", Action::Tools),
        ("說明　Ctrl+I", Action::Help),
        ("關於　Ctrl+A", Action::About),
        ("離開　Ctrl+Q", Action::Exit),
    ] {
        let enabled = match &action {
            Action::Month(delta) => calendar::navigate_month(selected, *delta).is_ok(),
            _ => true,
        };
        if ui.add_enabled(enabled, egui::Button::new(label)).clicked() {
            result = Some(action);
            ui.close();
        }
    }
    if ui.checkbox(&mut always_on_top, "最上層　Ctrl+T").changed() {
        result = Some(Action::Top);
    }
    result
}

impl eframe::App for CalendarApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.frames += 1;
        self.poll_export();
        if self.close_wait && self.job.is_none() {
            self.close_wait = false;
            if let Err(error) = self.request_close(ctx) {
                self.error = Some(format!("{error:#}"));
            }
        }
        if self.first_frame {
            self.first_frame = false;
            ctx.send_viewport_cmd(ViewportCommand::Title(format!(
                "CalendarRust · 今天 {}",
                self.session.today
            )));
        }
        self.window_sizing.sync(ctx);
        if let Some(rect) = ctx.input(|input| input.viewport().outer_rect) {
            self.settings.window_position = Some([rect.min.x, rect.min.y]);
        }
        if ctx.input(|input| input.viewport().close_requested()) && !self.allow_close {
            ctx.send_viewport_cmd(ViewportCommand::CancelClose);
            if let Err(error) = self.request_close(ctx) {
                self.error = Some(format!("無法離開，草稿仍保留：{error:#}"));
            }
        }
        let previous_today = self.session.today;
        let previous_selected = self.session.selected;
        if let Err(error) = self.session.update_today(self.clock.now().date_naive()) {
            self.error = Some(format!("今日更新失敗，未切換選日：{error:#}"));
        }
        if previous_today != self.session.today {
            ctx.send_viewport_cmd(ViewportCommand::Title(format!(
                "CalendarRust · 今天 {}",
                self.session.today
            )));
        }
        if (previous_today != self.session.today || previous_selected != self.session.selected)
            && let Err(error) = self.refresh(ctx)
        {
            self.error = Some(format!("{error:#}"));
        }
        if self.session.memo.debounce_due()
            && let Err(error) = self.session.flush()
        {
            self.error = Some(format!("備忘未保存：{error:#}"));
        }
        self.shortcuts(ctx);
        egui::CentralPanel::default()
            .frame(egui::Frame::NONE.fill(BG))
            .show(ctx, |ui| {
                egui::ScrollArea::both()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        let (rect, response) =
                            ui.allocate_exact_size(LAYOUT_SIZE, egui::Sense::click());
                        self.draw_layout(ui, rect.min);
                        response.context_menu(|ui| self.context_menu(ui));
                    });
            });
        self.draw_dialogs(ctx);
        if let Some(action) = self.action.take()
            && let Err(error) = self.execute(action, ctx)
        {
            self.error = Some(format!("{error:#}"));
        }
        let minimized = ctx.input(|input| input.viewport().minimized.unwrap_or(false));
        ctx.request_repaint_after(if minimized {
            Duration::from_secs(15)
        } else {
            Duration::from_secs(1)
        });
        if self.job.is_some() {
            ctx.request_repaint_after(Duration::from_millis(100));
        }
        if self.frames == 3 && self.screenshot.is_some() {
            ctx.send_viewport_cmd(ViewportCommand::Screenshot(egui::UserData::default()));
        }
        let screenshot_events = ctx.input(|input| {
            input
                .events
                .iter()
                .filter_map(|event| {
                    if let egui::Event::Screenshot { image, .. } = event {
                        Some(image.clone())
                    } else {
                        None
                    }
                })
                .collect::<Vec<_>>()
        });
        for image in screenshot_events {
            if let Some(path) = self.screenshot.take() {
                let bytes: Vec<u8> = image
                    .pixels
                    .iter()
                    .flat_map(|pixel| pixel.to_array())
                    .collect();
                let result = image::save_buffer(
                    &path,
                    &bytes,
                    image.size[0] as u32,
                    image.size[1] as u32,
                    image::ColorType::Rgba8,
                );
                match result {
                    Ok(()) => {
                        self.action = Some(Action::Exit);
                        ctx.request_repaint();
                    }
                    Err(error) => self.error = Some(format!("截圖保存失敗：{error}")),
                }
            }
        }
    }
}

impl Drop for CalendarApp {
    fn drop(&mut self) {
        if let Some(job) = &mut self.job {
            job.cancel.store(true, Ordering::Relaxed);
            if let Some(handle) = job.handle.take()
                && handle.join().is_err()
            {
                platform::show_fatal("匯出工作異常終止；請確認目的檔");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{APP_ICON_PNG, LAYOUT_SIZE, WindowSizing};
    use eframe::egui::{self, ViewportCommand};

    fn sizing_frame(
        sizing: &mut WindowSizing,
        ctx: &egui::Context,
        monitor_pixels: Option<egui::Vec2>,
        native_scale: f32,
    ) -> Vec<ViewportCommand> {
        let mut input = egui::RawInput::default();
        let viewport = input.viewports.get_mut(&egui::ViewportId::ROOT).unwrap();
        viewport.native_pixels_per_point = Some(native_scale);
        viewport.monitor_size =
            monitor_pixels.map(|size| size / (ctx.zoom_factor() * native_scale));
        ctx.run(input, |ctx| sizing.sync(ctx))
            .viewport_output
            .remove(&egui::ViewportId::ROOT)
            .unwrap()
            .commands
    }

    fn requested_size(commands: &[ViewportCommand]) -> Option<egui::Vec2> {
        commands.iter().find_map(|command| match command {
            ViewportCommand::InnerSize(size) => Some(*size),
            _ => None,
        })
    }

    #[test]
    fn window_tracks_zoom_in_both_directions_at_each_native_dpi() {
        for native_scale in [1.0, 1.25, 1.5, 2.0] {
            let ctx = egui::Context::default();
            let mut sizing = WindowSizing::default();
            for zoom in [1.0, 1.23, 1.5, 2.0, 0.75, 1.0] {
                ctx.set_zoom_factor(zoom);
                assert!(
                    requested_size(&sizing_frame(&mut sizing, &ctx, None, native_scale)).is_none()
                );
                let commands = sizing_frame(&mut sizing, &ctx, None, native_scale);
                let size = requested_size(&commands).unwrap();
                assert_eq!(size, LAYOUT_SIZE);
                assert!(
                    (size * ctx.pixels_per_point() - LAYOUT_SIZE * zoom * native_scale).length()
                        < 0.01
                );
                assert!(
                    requested_size(&sizing_frame(&mut sizing, &ctx, None, native_scale)).is_none()
                );
            }
        }
    }

    #[test]
    fn saved_zoom_is_applied_on_startup_with_monitor_limits() {
        let monitor = egui::vec2(1920.0, 1080.0);
        for native_scale in [1.0, 1.25, 1.5, 2.0] {
            for zoom in [0.75, 1.0, 1.23, 1.5, 2.0] {
                let ctx = egui::Context::default();
                let mut sizing = WindowSizing::default();
                ctx.set_zoom_factor(zoom);
                sizing_frame(&mut sizing, &ctx, Some(monitor), native_scale);
                let commands = sizing_frame(&mut sizing, &ctx, Some(monitor), native_scale);
                let pixels = requested_size(&commands).unwrap() * ctx.pixels_per_point();
                let available = monitor - egui::vec2(48.0, 96.0) * native_scale;
                let expected = (LAYOUT_SIZE * zoom * native_scale).min(available);
                assert!((pixels - expected).length() < 0.01);
            }
        }
    }

    #[test]
    fn embedded_app_icon_is_valid_512px_rgba() {
        let icon = eframe::icon_data::from_png_bytes(APP_ICON_PNG).unwrap();
        assert_eq!((icon.width, icon.height), (512, 512));
        assert_eq!(icon.rgba.len(), 512 * 512 * 4);
        assert!(icon.rgba.chunks_exact(4).any(|pixel| pixel[3] != 0));
    }
}
