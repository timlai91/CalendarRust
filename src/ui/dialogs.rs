use super::*;
use crate::{
    calendar::AlmanacAvailability,
    export::{COLOR_NAMES, ExportOptions},
    media,
    migration::{self, Conflict, ImportContent},
    storage,
};
use anyhow::Result;
use egui::{Align2, FontId, RichText, Stroke, StrokeKind, UiBuilder, vec2};
use std::{io::Write, path::Path, sync::atomic::Ordering};

impl CalendarApp {
    pub(super) fn draw_dialogs(&mut self, ctx: &egui::Context) {
        self.error_dialog(ctx);
        self.tools_dialog(ctx);
        self.import_dialog(ctx);
        self.restore_dialog(ctx);
        if self.about {
            egui::Window::new("關於 CalendarRust").open(&mut self.about).default_width(520.0).show(ctx, |ui| {
                ui.heading(format!("CalendarRust {}",env!("CARGO_PKG_VERSION")));
                ui.label("獨立 Rust 重建 · Windows 11 x64 · 離線使用\n不是原作者／原工房發行版，也不是擇日知錄網站。");
                ui.separator();
                ui.label("農民曆與生肖：LunarData／擇日知錄，MIT");
                ui.label("Copyright (c) 2026 當麻實驗室");
                ui.label("https://github.com/donma/LunarData");
                ui.label("歷史曆法候選：lunar_rust 1.0.1（MIT，移植自 6tail/lunar-python）");
                ui.label("系統中文字型與 emoji 字型僅本機讀取，未隨程式散布。");
                ui.label("農曆及節氣採核准 HKO UTC+08:00 公開曆表校正，附原始值、來源及校正清單；與 HKO 一致不代表第三個獨立來源再驗證。");
                ui.colored_label(RED,"目前為候選版本：完整 IME／多螢幕 DPI 與乾淨電腦驗收仍待人工確認，不是 M5 正式版。");
                ui.label(format!("LunarData 快照：{}",self.provider.lunar_data.manifest.snapshot_sha256));
                ui.label(format!("節氣：{}",self.provider.lunar_data.manifest.correction_version));
                ui.label("授權與來源清單隨完整發布包附於 licenses 與 assets。");
                ui.label("原版避凶／托盤精確還原與進階流派互動不屬於本版已完成能力。");
            });
        }
        if self.help {
            egui::Window::new("CalendarRust 離線說明").open(&mut self.help).default_width(560.0).vscroll(true).show(ctx, |ui| {
                ui.heading("操作");
                ui.label("上方選年月，右緣按鈕切年／月或回今天。日期格及底部日期帶共用選日命令。\n今天卡片與時鐘不隨選日改變。左下事件、右下備忘、日曆圖片及農民曆均屬於選日。");
                ui.label("備忘停止輸入 1 秒後保存；失焦、切日／月／模式、Ctrl+S、離開時立即保存。清空會刪除，純空白仍保存。\n失敗時保留草稿並阻擋切換，可重試、另存草稿，或明確放棄修改。");
                ui.label("Tab 移動焦點，日期格方向鍵移動焦點，Enter／空白選日；備忘內保留文字游標及 Ctrl+A。\nCtrl+F1/F2 上／下一年，Ctrl+F3/F4 上／下一月，Ctrl+F5 今天，Ctrl+F6 曆別，Ctrl+F7 月／日曆，Ctrl+T 置頂，Ctrl+I 說明，Ctrl+A 關於，Ctrl+Q 離開。");
                ui.label("視窗內容不足時可水平／垂直捲動；設定可調縮放。\n圖片支援 JPEG/BMP/ICO/PNG，Windows WMF/EMF 靜態轉換；不會刪外部原圖。\n輸出年曆可選七色及獨立抬頭圖，不需 Excel。匯出可取消且只在成功後提交目的檔。");
                ui.heading("保存／遷移");
                ui.label("資料位於 %LOCALAPPDATA%\\CalendarRust，不寫 Windows 或程式目錄。\n設定／備份／匯入提供含圖片的完整 ZIP 備份；還原須預覽確認，會先備份現況。\n舊資料來源只讀，先掃描及確認日期、編碼與衝突。歧義不硬猜；重複來源略過。");
                ui.heading("資料限制");
                ui.label("1912–1969 為歷史候選加 HKO 校正，無 LunarData 宜忌。\n全部節氣區間以核准 HKO 逐年資料重建，不是把錯誤年份減一。raw 與 corrections.json 保留差異及依據。\n列週數是該列星期四的 ISO 週年／週，不是星期日起算的每日 ISO 週數。");
                ui.colored_label(RED,"節日／紀念日不等於現行法定休假；不推算補班／補假。農民曆為民俗參考，不作安全、醫療或重大決策依據。\n事件為使用者核准的獨立整理清單，不保證原版全部標籤等價；LunarData 民俗為可選補充。完整第一版仍待人工驗收。");
            });
        }
        if self.almanac {
            self.almanac_viewport(ctx);
        }
        if let Some(report) = &mut self.report {
            let mut open = true;
            egui::Window::new("操作結果／匯入報告")
                .open(&mut open)
                .default_width(600.0)
                .vscroll(true)
                .show(ctx, |ui| {
                    ui.add(
                        egui::TextEdit::multiline(report)
                            .desired_width(f32::INFINITY)
                            .desired_rows(18),
                    );
                    if ui.button("另存報告").clicked()
                        && let Some(path) = rfd::FileDialog::new()
                            .set_file_name("CalendarRust-report.txt")
                            .save_file()
                        && let Err(error) = atomic_text(&path, report)
                    {
                        self.error = Some(format!("報告另存失敗：{error:#}"));
                    }
                });
            if !open {
                self.report = None;
            }
        }
        if self.close_pending {
            egui::Window::new("年曆仍在匯出")
                .collapsible(false)
                .resizable(false)
                .show(ctx, |ui| {
                    ui.label("完成或取消清理後才能離開，不會留下半成品目的檔。");
                    if ui.button("等待完成後離開").clicked() {
                        self.close_wait = true;
                        self.close_pending = false;
                    }
                    if ui.button("取消匯出後離開").clicked() {
                        if let Some(job) = &self.job {
                            job.cancel.store(true, Ordering::Relaxed);
                        }
                        self.close_wait = true;
                        self.close_pending = false;
                    }
                    if ui.button("返回程式").clicked() {
                        self.close_pending = false;
                    }
                });
        }
        if let Some(notice) = self.session.notice.clone() {
            egui::Window::new("日期提示")
                .collapsible(false)
                .show(ctx, |ui| {
                    ui.label(notice);
                    if ui.button("確定").clicked() {
                        self.session.notice = None;
                    }
                });
        }
    }

    fn error_dialog(&mut self, ctx: &egui::Context) {
        if let Some(error) = self.error.clone() {
            let mut open = true;
            egui::Window::new("操作未完成")
                .open(&mut open)
                .collapsible(false)
                .default_width(540.0)
                .show(ctx, |ui| {
                    ui.colored_label(RED, error);
                    if self.session.memo.dirty() {
                        ui.label("草稿仍屬於原日期，切換／離開必須先成功保存或明確放棄。");
                        if ui.button("重試保存").clicked() {
                            match self.session.flush().and_then(|_| self.persist_settings()) {
                                Ok(()) => self.error = None,
                                Err(error) => self.error = Some(format!("{error:#}")),
                            }
                        }
                        if ui.button("另存草稿（不改變原保存狀態）").clicked()
                            && let Some(path) = rfd::FileDialog::new()
                                .set_file_name(format!(
                                    "CalendarRust-draft-{}.txt",
                                    self.session.memo.date
                                ))
                                .save_file()
                        {
                            match atomic_text(&path, &self.session.memo.body) {
                                Ok(()) => {
                                    self.report = Some(
                                        "草稿已另存；尚未放棄原草稿，也未將備忘標為已儲存。".into(),
                                    )
                                }
                                Err(error) => self.error = Some(format!("另存失敗：{error:#}")),
                            }
                        }
                        if ui.button("明確放棄本次修改，恢復上次保存內容").clicked()
                        {
                            self.session.memo.discard();
                            self.error = None;
                        }
                    }
                    if ui.button("返回（保留草稿）").clicked() {
                        self.error = None;
                    }
                });
            if !open {
                self.error = None;
            }
        }
    }

    fn almanac_viewport(&mut self, ctx: &egui::Context) {
        let mut open = true;
        let info = &self.selected_info;
        ctx.show_viewport_immediate(egui::ViewportId::from_hash_of("almanac-detail"),
            egui::ViewportBuilder::default().with_title(format!("CalendarRust 農民曆 · {}",info.date))
                .with_inner_size([620.0,700.0]).with_min_inner_size([360.0,280.0]),
            |ctx,_class| {
                if ctx.input(|input| input.viewport().close_requested()) { open=false; }
                egui::CentralPanel::default().show(ctx, |ui| {
                    egui::ScrollArea::vertical().show(ui, |ui| {
                        ui.heading(format!("選日 {}",info.date));
                        ui.label("LunarData／擇日知錄，民俗參考；流派與地區規則可能不同。");
                        ui.colored_label(RED,"不作安全、醫療或重大決策依據；非原版避凶算法還原。按民用日期查表，不在 23:00 自動換日。");
                        if let Ok(lunar)=&info.lunar { ui.label(lunar.description()); }
                        match &info.almanac {
                            AlmanacAvailability::OutOfRange => { ui.label("此日期無 LunarData 資料（覆蓋自 1970-01-01 起）；不補造宜忌。"); }
                            AlmanacAvailability::Invalid(error) => { ui.colored_label(RED,format!("來源讀取失敗：{error}")); }
                            AlmanacAvailability::Available(raw) => {
                                if let Some(image)=&self.image {
                                    ui.add(egui::Image::new((image.id(),image.size_vec2())).fit_to_exact_size(vec2(150.0,150.0)));
                                    ui.label("選日圖片（自選圖優先；否則農曆年生肖）");
                                }
                                ui.heading("宜／忌（保持來源順序）");
                                list(ui,"宜",&raw.auspicious);
                                list(ui,"忌",&raw.inauspicious);
                                ui.separator();
                                ui.label(format!("日干支：{}／納音：{}",raw.day_ganzhi.full,raw.day_ganzhi.nayin.as_deref().unwrap_or("來源未列")));
                                ui.label(format!("月干支：{}",raw.month_ganzhi.full));
                                ui.label(format!("沖生肖：{}　煞：{}　{}　被沖年干支：{}",raw.zodiac_clash,raw.clash_direction,raw.clash_detail,raw.clash_year_ganzhi));
                                ui.label(format!("彭祖百忌：{}",raw.peng_taboo));
                                ui.label(format!("十二建除：{}",raw.twelve_officer));
                                ui.label(format!("胎神：{}",raw.fetal_god));
                                for (name,value) in &raw.lucky_direction { ui.label(format!("{name}：{value}")); }
                                list(ui,"吉神",&raw.good_gods);
                                list(ui,"凶神",&raw.bad_gods);
                                ui.heading("十二時辰");
                                match raw.validate_hours() {
                                    Ok(()) => {
                                        list(ui,"吉時",&raw.auspicious_hours);
                                        list(ui,"凶時",&raw.inauspicious_hours);
                                        ui.label(raw.hour_ganzhi.iter().map(|(hour,ganzhi)| format!("{hour}：{ganzhi}")).collect::<Vec<_>>().join("　"));
                                    }
                                    Err(error) => { ui.colored_label(RED,error.to_string()); }
                                }
                                ui.heading("當前節氣期間（不等於當日交節）");
                                match &info.solar_term {
                                    Ok(Some(term)) => {
                                        ui.label(format!("{}：{} ≤ 本日 < {}；下個節氣 {} {}",term.name,term.start_date,term.end_date,term.next_name,term.next_date));
                                        ui.label(&term.desc);
                                    }
                                    Ok(None) => { ui.label("來源未列"); }
                                    Err(error) => { ui.colored_label(RED,format!("此欄不可用：{error}")); }
                                }
                                ui.heading("民俗事件／介紹");
                                list(ui,"來源事件",&raw.deity_birthday);
                                for event in &raw.deity_info {
                                    ui.label(event["name"].as_str().unwrap_or("來源事件名稱無效"));
                                    if event["info"].is_null() { ui.label("此項無介紹"); }
                                    else if let Some(description)=event["info"]["description"].as_str() { ui.label(description); }
                                    else { ui.label("此項未列文字介紹"); }
                                }
                                ui.label("三派、飛星、日宿／月宿與個人化互動為 P2；原始資料保留在離線來源庫，不混合成唯一吉凶結論。");
                            }
                        }
                    });
                });
            });
        self.almanac = open;
    }

    fn tools_dialog(&mut self, ctx: &egui::Context) {
        if !self.tools {
            return;
        }
        let mut open = true;
        egui::Window::new("設定／資料管理")
            .open(&mut open)
            .default_width(540.0)
            .show(ctx, |ui| {
                let mut changed = false;
                changed |= ui
                    .add(egui::Slider::new(&mut self.settings.zoom, 0.75..=2.0).text("介面縮放"))
                    .changed();
                if changed {
                    ctx.set_zoom_factor(self.settings.zoom);
                }
                if ui
                    .checkbox(&mut self.settings.always_on_top, "最上層")
                    .changed()
                {
                    changed = true;
                    self.apply_top(ctx);
                }
                if ui
                    .checkbox(
                        &mut self.settings.supplemental_events,
                        "額外顯示 LunarData 節日／民俗（不替代獨立清單）",
                    )
                    .changed()
                {
                    changed = true;
                    self.provider.supplemental = self.settings.supplemental_events;
                    if let Err(error) = self.refresh(ctx) {
                        self.error = Some(format!("{error:#}"));
                    }
                }
                if changed && let Err(error) = self.persist_settings() {
                    self.error = Some(format!("設定未保存：{error:#}"));
                }
                ui.separator();
                ui.label("資料位於使用者 LOCALAPPDATA，備份含一致 SQLite 快照與被引用的受管理圖。");
                if ui.button("建立完整備份 ZIP").clicked() {
                    let result = (|| -> Result<()> {
                        self.session.flush()?;
                        self.persist_settings()?;
                        if let Some(path) = rfd::FileDialog::new()
                            .set_file_name(format!(
                                "CalendarRust-backup-{}.zip",
                                chrono::Local::now().format("%Y%m%d-%H%M%S")
                            ))
                            .save_file()
                        {
                            self.session
                                .repository
                                .backup(&path, self.source_version())?;
                            self.report = Some(format!("完整備份已建立：{}", path.display()));
                        }
                        Ok(())
                    })();
                    if let Err(error) = result {
                        self.error = Some(format!("備份失敗：{error:#}"));
                    }
                }
                if ui
                    .add_enabled(self.job.is_none(), egui::Button::new("驗證／預覽完整還原"))
                    .clicked()
                    && let Some(path) = rfd::FileDialog::new()
                        .add_filter("CalendarRust 備份", &["zip"])
                        .pick_file()
                {
                    match storage::verify_backup(&path) {
                        Ok(backup) => self.restore = Some(backup),
                        Err(error) => {
                            self.error = Some(format!("拒絕還原，既有資料未變更：{error:#}"))
                        }
                    }
                }
                if ui
                    .add_enabled(self.job.is_none(), egui::Button::new("只讀舊版資料匯入"))
                    .clicked()
                {
                    self.import_open = true;
                }
                if ui.button("清理無引用受管理圖片（不刪外部原圖）").clicked() {
                    match self
                        .session
                        .repository
                        .cleanup_orphans(self.settings.header_asset.as_deref())
                    {
                        Ok(count) => self.report = Some(format!("已清理 {count} 個無引用圖片項目")),
                        Err(error) => self.error = Some(format!("清理未完成：{error:#}")),
                    }
                }
                if ui.button("核對離線來源庫 SHA-256").clicked() {
                    match crate::data::verify_package(&self.data) {
                        Ok(()) => {
                            self.report = Some(
                                "來源庫 SHA-256 符合隨附清單；不代表獨立曆法正確性驗證已完成。"
                                    .into(),
                            )
                        }
                        Err(error) => self.error = Some(format!("{error:#}")),
                    }
                }
                ui.label("備份還原為完整取代，不是合併；還原與匯入前自動備份現況。");
            });
        self.tools = open;
    }

    pub(super) fn choose_image(&mut self, ctx: &egui::Context, header: bool) {
        if let Some(path) = rfd::FileDialog::new()
            .add_filter("圖片", &["jpg", "jpeg", "bmp", "ico", "png", "wmf", "emf"])
            .pick_file()
        {
            let result = (|| -> Result<()> {
                let image = media::load(&path)?;
                let id = self.session.repository.register_image(
                    &image.png,
                    image.width,
                    image.height,
                )?;
                if header {
                    let previous = self.settings.header_asset.replace(id);
                    if let Err(error) = self.persist_settings() {
                        self.settings.header_asset = previous;
                        return Err(error);
                    }
                } else {
                    self.session
                        .repository
                        .set_daily_asset(self.session.selected, Some(&id))?;
                }
                Ok(())
            })();
            match result {
                Ok(()) => self.reload_images(ctx),
                Err(error) => self.error = Some(format!("圖片未取代舊映射：{error:#}")),
            }
        }
    }

    pub(super) fn export_panels(&mut self, ui: &mut egui::Ui, origin: egui::Pos2) {
        let left = panels::region(origin, 16.0, 44.0, 192.0, 526.0);
        ui.painter().rect_filled(left, 0.0, PAPER);
        ui.painter()
            .rect_stroke(left, 0.0, Stroke::new(1.0_f32, BORDER), StrokeKind::Inside);
        ui.scope_builder(UiBuilder::new().max_rect(left.shrink(8.0)), |ui| {
            egui::ScrollArea::vertical()
                .id_salt("export-options")
                .show(ui, |ui| {
                    ui.heading("年曆設定");
                    ui.label("匯出年份（不改選日）");
                    egui::ComboBox::from_id_salt("export-year")
                        .selected_text(format!(
                            "{}／民國{}",
                            self.export_year,
                            self.export_year - 1911
                        ))
                        .show_ui(ui, |ui| {
                            for year in 1912..=2050 {
                                ui.selectable_value(
                                    &mut self.export_year,
                                    year,
                                    format!("{}／民國{}", year, year - 1911),
                                );
                            }
                        });
                    ui.label(
                        RichText::new("歲次／生肖於農曆正月初一換年，不將整年標為單一生肖")
                            .size(11.0),
                    );
                    let mut changed = false;
                    for (i, name) in COLOR_NAMES.iter().enumerate() {
                        ui.label(RichText::new(*name).size(11.0));
                        let color = self.settings.colors[i];
                        let mut rgb = [(color >> 16) as u8, (color >> 8) as u8, color as u8];
                        if ui.color_edit_button_srgb(&mut rgb).changed() {
                            self.settings.colors[i] = u32::from(rgb[0]) << 16
                                | u32::from(rgb[1]) << 8
                                | u32::from(rgb[2]);
                            changed = true;
                        }
                    }
                    if ui.button("還原七色預設").clicked() {
                        self.settings.colors = storage::DEFAULT_COLORS;
                        changed = true;
                    }
                    if changed && let Err(error) = self.persist_settings() {
                        self.error = Some(format!("七色設定未保存：{error:#}"));
                    }
                    if ui.button("選擇抬頭圖片").clicked() {
                        self.choose_image(ui.ctx(), true);
                    }
                    if ui.button("移除抬頭圖片").clicked() {
                        let previous = self.settings.header_asset.take();
                        if let Err(error) = self.persist_settings() {
                            self.settings.header_asset = previous;
                            self.error = Some(format!("{error:#}"));
                        }
                        self.reload_images(ui.ctx());
                    }
                    if ui
                        .add_enabled(self.job.is_none(), egui::Button::new("選擇位置並匯出 XLSX"))
                        .clicked()
                    {
                        self.start_export();
                    }
                });
        });
        let preview = panels::region(origin, 224.0, 44.0, 600.0, 370.0);
        ui.painter().rect_filled(preview, 0.0, PAPER);
        ui.painter().rect_stroke(
            preview,
            0.0,
            Stroke::new(1.0_f32, BORDER),
            StrokeKind::Inside,
        );
        let month_preview = self.provider.month(self.export_year, 1);
        ui.scope_builder(UiBuilder::new().max_rect(preview.shrink(12.0)), |ui| {
            ui.heading(format!(
                "西元 {} 年／民國 {} 年",
                self.export_year,
                self.export_year - 1911
            ));
            ui.label("XLSX：12 月，3 欄 × 4 列，A4 橫向，另附資料來源表");
            ui.allocate_ui(vec2(325.0, 70.0), |ui| {
                if let Some(header) = &self.header {
                    ui.add(
                        egui::Image::new((header.id(), header.size_vec2()))
                            .fit_to_exact_size(vec2(325.0, 70.0)),
                    );
                } else {
                    ui.label("325 × 70 抬頭圖片預覽（可選）");
                }
            });
            ui.separator();
            ui.label("一月實際日期／事件預覽（與 XLSX 共用資料及色彩規則）");
            match &month_preview {
                Ok(grid) => {
                    let (bounds, _) =
                        ui.allocate_exact_size(vec2(568.0, 150.0), egui::Sense::hover());
                    let width = bounds.width() / 7.0;
                    for (index, day) in grid.days.iter().enumerate() {
                        let rect = egui::Rect::from_min_size(
                            bounds.min
                                + vec2((index % 7) as f32 * width, (index / 7) as f32 * 25.0),
                            vec2(width, 25.0),
                        );
                        let color = self.settings.colors
                            [crate::export::day_color_index(day, self.export_year, 1)];
                        ui.painter().rect_filled(rect, 0.0, rgb_color(color));
                        ui.painter().rect_stroke(
                            rect,
                            0.0,
                            Stroke::new(1.0_f32, rgb_color(self.settings.colors[4])),
                            StrokeKind::Inside,
                        );
                        let text_color = if day.date.month() == 1 {
                            INK
                        } else {
                            Color32::GRAY
                        };
                        ui.painter().text(
                            rect.center() - vec2(0.0, 6.0),
                            Align2::CENTER_CENTER,
                            day.date.day(),
                            FontId::proportional(13.0),
                            text_color,
                        );
                        let label = day.short_label().chars().take(6).collect::<String>();
                        ui.painter().text(
                            rect.center() + vec2(0.0, 7.0),
                            Align2::CENTER_CENTER,
                            label,
                            FontId::proportional(10.0),
                            text_color,
                        );
                        ui.interact(
                            rect,
                            egui::Id::new(("export-preview", day.date)),
                            egui::Sense::hover(),
                        )
                        .on_hover_text(day.tooltip());
                    }
                    ui.painter().rect_stroke(
                        bounds,
                        0.0,
                        Stroke::new(2.0_f32, rgb_color(self.settings.colors[5])),
                        StrokeKind::Inside,
                    );
                }
                Err(error) => {
                    ui.colored_label(RED, format!("預覽不可用：{error:#}"));
                }
            }
            ui.horizontal(|ui| {
                let (sample, _) = ui.allocate_exact_size(vec2(16.0, 12.0), egui::Sense::hover());
                ui.painter()
                    .rect_filled(sample, 0.0, rgb_color(self.settings.colors[3]));
                ui.label(
                    RichText::new("補假只展示圖例，不推算日期；不匯出私人備忘／每日圖。")
                        .size(10.0),
                );
            });
        });
        let status = panels::region(origin, 224.0, 426.0, 600.0, 144.0);
        ui.painter().rect_filled(status, 0.0, PAPER);
        ui.scope_builder(UiBuilder::new().max_rect(status.shrink(10.0)), |ui| {
            ui.label(&self.export_status);
            if let Some(job) = &self.job {
                ui.add(
                    egui::ProgressBar::new(job.month as f32 / 12.0)
                        .text(format!("{}/12 月", job.month)),
                );
                if ui.button("取消匯出").clicked() {
                    job.cancel.store(true, Ordering::Relaxed);
                }
                if let Some(path) = &self.last_export
                    && ui.button("以系統預設程式開啟已完成的 XLSX").clicked()
                    && let Err(error) = platform::open_document(path)
                {
                    self.error = Some(format!("{error:#}"));
                }
            }
            if ui.button("返回月曆（保留選日）").clicked() {
                self.action = Some(Action::Mode(ViewMode::Month));
            }
        });
    }

    fn start_export(&mut self) {
        let result = (|| -> Result<()> {
            self.session.flush()?;
            self.persist_settings()?;
            let Some(target) = rfd::FileDialog::new()
                .add_filter("Excel 工作簿", &["xlsx"])
                .set_file_name(format!("CalendarRust-{}.xlsx", self.export_year))
                .save_file()
            else {
                return Ok(());
            };
            let overwrite = target.exists();
            if overwrite
                && rfd::MessageDialog::new()
                    .set_title("確認取代")
                    .set_description("目的檔已存在。只有成功完成匯出後才原子取代；確定覆寫？")
                    .set_buttons(rfd::MessageButtons::YesNo)
                    .show()
                    != rfd::MessageDialogResult::Yes
            {
                return Ok(());
            }
            let header_png = match &self.settings.header_asset {
                Some(id) => Some(std::fs::read(self.session.repository.asset_path(id)?)?),
                None => None,
            };
            let options = ExportOptions {
                year: self.export_year,
                colors: self.settings.colors,
                header_png,
                target,
                overwrite,
                supplemental: self.settings.supplemental_events,
                source_snapshot: self.source_version().into(),
            };
            self.job = Some(ExportJob::start(self.data.clone(), options)?);
            self.export_status = "背景產生年曆中；已凍結年份、來源與七色快照".into();
            Ok(())
        })();
        if let Err(error) = result {
            self.error = Some(format!("{error:#}"));
        }
    }

    fn restore_dialog(&mut self, ctx: &egui::Context) {
        if self.restore.is_none() {
            return;
        }
        let mut confirm = false;
        let mut cancel = false;
        let mut open = true;
        egui::Window::new("確認完整取代還原")
            .open(&mut open)
            .default_width(520.0)
            .show(ctx, |ui| {
                if let Some(backup) = &self.restore {
                    ui.label(format!(
                        "已驗證清單、SHA-256、schema 與安全路徑。\n{} 則備忘，{} 張引用圖片。",
                        backup.memos, backup.images
                    ));
                    if backup.manifest.source_version != self.source_version() {
                        ui.colored_label(
                            RED,
                            "備份的曆表來源版本與目前不同。備忘仍按西元日期還原，不重新解讀農曆。",
                        );
                    }
                    ui.colored_label(
                        RED,
                        "將完整取代目前備忘、圖片映射及設定；不是合併。確認前會先備份現況。",
                    );
                    if ui.button("確認：備份現況並完整還原").clicked() {
                        confirm = true;
                    }
                    if ui.button("取消").clicked() {
                        cancel = true;
                    }
                }
            });
        if confirm {
            if let Some(backup) = self.restore.take() {
                let result = (|| -> Result<()> {
                    self.session.flush()?;
                    self.persist_settings()?;
                    let version = self.source_version().to_string();
                    let safety = self.session.repository.restore(backup, &version)?;
                    self.settings = self.session.repository.settings()?;
                    self.session.reload_memo()?;
                    self.provider.supplemental = self.settings.supplemental_events;
                    ctx.set_zoom_factor(self.settings.zoom);
                    self.apply_top(ctx);
                    self.refresh(ctx)?;
                    self.report = Some(format!("完整還原已提交；還原前備份：{}", safety.display()));
                    Ok(())
                })();
                if let Err(error) = result {
                    self.error = Some(format!("還原未完成：{error:#}；請保留還原前備份"));
                }
            }
        } else if !open || cancel {
            self.restore = None;
        }
    }

    fn import_dialog(&mut self, ctx: &egui::Context) {
        if !self.import_open {
            return;
        }
        let mut open = true;
        let mut commit = false;
        egui::Window::new("舊版資料只讀掃描／預覽")
            .open(&mut open)
            .default_width(740.0)
            .default_height(560.0)
            .vscroll(true)
            .show(ctx, |ui| {
                ui.label("不修改來源、不執行舊 EXE。日期／編碼歧義須確認；衝突預設略過。");
                if ui.button("選擇舊備忘 .txt 目錄").clicked() {
                    self.import.directory = rfd::FileDialog::new().pick_folder();
                    self.import.plan = None;
                }
                ui.label(
                    self.import
                        .directory
                        .as_ref()
                        .map(|p| p.display().to_string())
                        .unwrap_or_else(|| "尚未選來源目錄".into()),
                );
                if ui.button("選擇 pCalendar.dat（可選）").clicked() {
                    self.import.mapping = rfd::FileDialog::new()
                        .add_filter("圖片映射", &["dat"])
                        .pick_file();
                    self.import.plan = None;
                }
                if let Some(path) = &self.import.mapping {
                    ui.label(path.display().to_string());
                }
                if ui
                    .button("相對圖片路徑基準／舊 EXE 目錄（需明確選擇）")
                    .clicked()
                {
                    self.import.base = rfd::FileDialog::new().pick_folder();
                    self.import.plan = None;
                }
                ui.horizontal(|ui| {
                    ui.label("映射檔編碼");
                    egui::ComboBox::from_id_salt("mapping-encoding")
                        .selected_text(&self.import.mapping_encoding)
                        .show_ui(ui, |ui| {
                            for encoding in ["自動", "UTF-8", "Big5"] {
                                ui.selectable_value(
                                    &mut self.import.mapping_encoding,
                                    encoding.to_string(),
                                    encoding,
                                );
                            }
                        });
                    if ui.button("掃描／重新掃描").clicked() {
                        match self
                            .import
                            .directory
                            .as_deref()
                            .context("請先選來源目錄")
                            .and_then(|directory| {
                                migration::scan(
                                    directory,
                                    self.import.mapping.as_deref(),
                                    self.import.base.as_deref(),
                                    (self.import.mapping_encoding != "自動")
                                        .then_some(self.import.mapping_encoding.as_str()),
                                )
                            }) {
                            Ok(plan) => self.import.plan = Some(plan),
                            Err(error) => self.error = Some(format!("掃描未完成：{error:#}")),
                        }
                    }
                });
                if let Some(plan) = &mut self.import.plan {
                    ui.checkbox(
                        &mut plan.allow_partial,
                        "明確同意只匯入已確認有效項目，其他問題列於報告（否則整批阻擋）",
                    );
                    for (index, item) in plan.items.iter_mut().enumerate() {
                        ui.push_id(index, |ui| {
                            ui.separator();
                            ui.checkbox(&mut item.enabled, &item.name);
                            if let Some(problem) = &item.problem {
                                ui.colored_label(RED, problem);
                            }
                            let current = item
                                .date()
                                .map(|d| d.to_string())
                                .unwrap_or_else(|| "未確認／無效".into());
                            egui::ComboBox::from_id_salt("import-date")
                                .selected_text(current)
                                .show_ui(ui, |ui| {
                                    for (index, date) in item.dates.iter().enumerate() {
                                        ui.selectable_value(
                                            &mut item.chosen_date,
                                            Some(index),
                                            date.to_string(),
                                        );
                                    }
                                });
                            if let ImportContent::Memo(candidates) = &item.content {
                                let label = item
                                    .encoding
                                    .and_then(|index| candidates.get(index))
                                    .map(|text| text.encoding.clone())
                                    .unwrap_or_else(|| "請確認編碼".into());
                                egui::ComboBox::from_id_salt("import-encoding")
                                    .selected_text(label)
                                    .show_ui(ui, |ui| {
                                        for (index, text) in candidates.iter().enumerate() {
                                            ui.selectable_value(
                                                &mut item.encoding,
                                                Some(index),
                                                &text.encoding,
                                            );
                                        }
                                    });
                            }
                            ui.horizontal(|ui| {
                                ui.label("同日期衝突／重複候選：");
                                ui.selectable_value(&mut item.conflict, Conflict::Skip, "略過");
                                ui.selectable_value(&mut item.conflict, Conflict::Replace, "取代");
                                if matches!(item.content, ImportContent::Memo(_)) {
                                    ui.selectable_value(
                                        &mut item.conflict,
                                        Conflict::Merge,
                                        "合併備忘",
                                    );
                                }
                            });
                            if let Some(date) = item.date() {
                                if !calendar::selectable(date) {
                                    ui.label("目前 UI 不可瀏覽，仍可保存有效備忘");
                                }
                                match self.session.repository.load_memo(date) {
                                    Ok(existing) if !existing.is_empty() => {
                                        ui.collapsing("現有備忘預覽", |ui| {
                                            ui.label(existing);
                                        });
                                    }
                                    Ok(_) => {}
                                    Err(error) => {
                                        ui.colored_label(
                                            RED,
                                            format!("現有資料讀取失敗：{error:#}"),
                                        );
                                    }
                                }
                            }
                            if let Some(body) = item.body() {
                                ui.collapsing("來源正文預覽", |ui| {
                                    ui.label(body);
                                });
                            }
                            if let ImportContent::Image(path) = &item.content {
                                ui.label(path.display().to_string());
                            }
                        });
                    }
                    if ui.button("確認以上規則，先備份現況再交易匯入").clicked() {
                        commit = true;
                    }
                }
            });
        self.import_open = open;
        if commit {
            let result = (|| -> Result<()> {
                self.session.flush()?;
                self.persist_settings()?;
                let plan = self.import.plan.as_ref().context("沒有匯入預覽")?;
                let version = self.source_version().to_string();
                let report = migration::apply(plan, &mut self.session.repository, &version)?;
                self.session.reload_memo()?;
                self.refresh(ctx)?;
                self.report = Some(report.join("\n"));
                Ok(())
            })();
            if let Err(error) = result {
                self.error = Some(format!("匯入未完成：{error:#}"));
            }
        }
    }
}

fn list(ui: &mut egui::Ui, label: &str, values: &[String]) {
    ui.label(format!(
        "{label}：{}",
        if values.is_empty() {
            "此來源未列項目".into()
        } else {
            values.join("、")
        }
    ));
}

fn rgb_color(value: u32) -> Color32 {
    Color32::from_rgb((value >> 16) as u8, (value >> 8) as u8, value as u8)
}

fn atomic_text(path: &Path, text: &str) -> Result<()> {
    let mut temporary = tempfile::NamedTempFile::new_in(path.parent().context("儲存位置無效")?)?;
    temporary.write_all(text.as_bytes())?;
    temporary.as_file().sync_all()?;
    temporary.persist(path)?;
    Ok(())
}
