use super::*;
use crate::{
    app::SaveState,
    calendar::{WEEKDAY_EN, WEEKDAYS},
    storage::CalendarDisplay,
};
use chrono::Timelike;
use egui::{
    Align2, FontId, Pos2, Rect, RichText, Sense, Stroke, StrokeKind, Ui, UiBuilder, pos2, vec2,
};

pub(super) fn region(origin: Pos2, x: f32, y: f32, w: f32, h: f32) -> Rect {
    Rect::from_min_size(origin + vec2(x, y), vec2(w, h))
}

fn text(ui: &Ui, position: Pos2, anchor: Align2, value: impl ToString, size: f32, color: Color32) {
    ui.painter().text(
        position,
        anchor,
        value.to_string(),
        FontId::proportional(size),
        color,
    );
}

fn panel(ui: &Ui, rect: Rect, fill: Color32) {
    ui.painter().rect_filled(rect, 0.0, fill);
    ui.painter()
        .rect_stroke(rect, 0.0, Stroke::new(1.0_f32, BORDER), StrokeKind::Inside);
}

impl CalendarApp {
    pub(super) fn draw_layout(&mut self, ui: &mut Ui, origin: Pos2) {
        self.banner(ui, region(origin, 0.0, 0.0, 840.0, 32.0));
        if self.session.mode == ViewMode::YearExport {
            self.export_panels(ui, origin);
        } else {
            self.clock_panel(ui, region(origin, 16.0, 44.0, 192.0, 172.0));
            self.today_panel(ui, region(origin, 16.0, 228.0, 192.0, 208.0));
            self.event_panel(ui, region(origin, 16.0, 448.0, 192.0, 122.0));
            let main = region(origin, 224.0, 44.0, 600.0, 370.0);
            if self.session.mode == ViewMode::Month {
                self.month_panel(ui, main);
            } else {
                self.day_panel(ui, main);
            }
            self.memo_panel(ui, region(origin, 224.0, 426.0, 600.0, 144.0));
        }
        self.date_strip(ui, region(origin, 0.0, 582.0, 840.0, 28.0));
        self.footer(ui, region(origin, 0.0, 610.0, 840.0, 30.0));
    }

    fn banner(&mut self, ui: &mut Ui, rect: Rect) {
        ui.painter().rect_filled(rect, 0.0, PAPER);
        let controls = Rect::from_min_max(rect.min + vec2(224.0, 4.0), rect.max - vec2(10.0, 4.0));
        ui.scope_builder(
            UiBuilder::new()
                .max_rect(controls)
                .layout(egui::Layout::left_to_right(egui::Align::Center)),
            |ui| {
                ui.spacing_mut().item_spacing.x = 6.0;
                ui.spacing_mut().interact_size.y = controls.height();
                if ui
                    .button(match self.settings.display {
                        CalendarDisplay::Roc => "民國",
                        CalendarDisplay::Gregorian => "西元",
                    })
                    .clicked()
                {
                    self.action = Some(Action::ToggleDisplay);
                }
                let mut year = self.session.selected.year();
                let mut month = self.session.selected.month();
                let old = (year, month);
                egui::ComboBox::from_id_salt("year")
                    .selected_text(format!(
                        "{} 年",
                        if self.settings.display == CalendarDisplay::Roc {
                            year - 1911
                        } else {
                            year
                        }
                    ))
                    .width(75.0)
                    .height(300.0)
                    .show_ui(ui, |ui| {
                        for y in 1912..=2050 {
                            ui.selectable_value(
                                &mut year,
                                y,
                                format!(
                                    "{} 年",
                                    if self.settings.display == CalendarDisplay::Roc {
                                        y - 1911
                                    } else {
                                        y
                                    }
                                ),
                            );
                        }
                    });
                egui::ComboBox::from_id_salt("month")
                    .selected_text(format!("{month} 月"))
                    .width(50.0)
                    .show_ui(ui, |ui| {
                        for m in 1..=12 {
                            ui.selectable_value(&mut month, m, format!("{m} 月"));
                        }
                    });
                if old != (year, month) {
                    self.action = Some(Action::SetMonth(year, month));
                }
                let selected = match &self.selected_info.lunar {
                    Ok(lunar) => format!("選日 {}／{}", lunar.year_ganzhi, lunar.zodiac),
                    Err(_) => "選日資料不可用".into(),
                };
                ui.label(
                    RichText::new(selected)
                        .size(13.0)
                        .color(Color32::from_rgb(0x8D, 0x60, 0x42)),
                );
                for (mode, label) in [(ViewMode::Month, "月曆"), (ViewMode::Day, "日曆")] {
                    if ui
                        .selectable_label(self.session.mode == mode, label)
                        .clicked()
                    {
                        self.action = Some(Action::Mode(mode));
                    }
                }
                if ui.button("設定").clicked() {
                    self.action = Some(Action::Tools);
                }
            },
        );
    }

    fn clock_panel(&self, ui: &mut Ui, rect: Rect) {
        panel(ui, rect, Color32::from_rgb(0xE5, 0xEC, 0xEE));
        let center = rect.min + vec2(96.0, 76.0);
        let p = ui.painter();
        p.circle_filled(center, 72.0, Color32::from_rgb(0xF7, 0xFA, 0xF9));
        p.circle_stroke(
            center,
            72.0,
            Stroke::new(2.0_f32, Color32::from_rgb(0xC2, 0xCE, 0xD0)),
        );
        let roman = [
            "XII", "I", "II", "III", "IV", "V", "VI", "VII", "VIII", "IX", "X", "XI",
        ];
        for (i, label) in roman.iter().enumerate() {
            let angle = i as f32 * std::f32::consts::TAU / 12.0 - std::f32::consts::FRAC_PI_2;
            text(
                ui,
                center + vec2(angle.cos(), angle.sin()) * 58.0,
                Align2::CENTER_CENTER,
                label,
                12.0,
                INK,
            );
        }
        let now = self.clock.now();
        let minute = now.minute() as f32 + now.second() as f32 / 60.0;
        for (angle, length, width) in [
            (
                (now.hour() % 12) as f32 / 12.0 + minute / 720.0,
                37.0,
                4.0_f32,
            ),
            (minute / 60.0, 51.0, 2.0_f32),
        ] {
            let angle = angle * std::f32::consts::TAU - std::f32::consts::FRAC_PI_2;
            p.line_segment(
                [center, center + vec2(angle.cos(), angle.sin()) * length],
                Stroke::new(width, INK),
            );
        }
        p.circle_filled(center, 4.0, INK);
        text(
            ui,
            rect.min + vec2(96.0, 159.0),
            Align2::CENTER_CENTER,
            now.format("%H:%M:%S"),
            14.0,
            Color32::from_rgb(0x52, 0x71, 0x7B),
        );
    }

    fn today_panel(&self, ui: &mut Ui, rect: Rect) {
        panel(ui, rect, Color32::from_rgb(0xF8, 0xFA, 0xF6));
        ui.painter().rect_filled(
            Rect::from_min_size(rect.min, vec2(rect.width(), 24.0)),
            0.0,
            Color32::from_rgb(0xDF, 0xE8, 0xDD),
        );
        text(
            ui,
            rect.min + vec2(96.0, 12.0),
            Align2::CENTER_CENTER,
            "今天",
            13.0,
            Color32::from_rgb(0x4E, 0x6B, 0x4F),
        );
        let today = self.session.today;
        text(
            ui,
            rect.min + vec2(96.0, 79.0),
            Align2::CENTER_CENTER,
            today.day(),
            104.0,
            Color32::from_rgb(0x3A, 0x57, 0x4E),
        );
        let index = today.weekday().num_days_from_sunday() as usize;
        text(
            ui,
            rect.min + vec2(96.0, 137.0),
            Align2::CENTER_CENTER,
            format!("{}　{}", WEEKDAYS[index], WEEKDAY_EN[index]),
            14.0,
            INK,
        );
        text(
            ui,
            rect.min + vec2(96.0, 157.0),
            Align2::CENTER_CENTER,
            today.format("%Y 年 %m 月"),
            13.0,
            INK,
        );
        match &self.today_info.lunar {
            Ok(lunar) => {
                text(
                    ui,
                    rect.min + vec2(96.0, 177.0),
                    Align2::CENTER_CENTER,
                    format!(
                        "農曆 {}{}{}",
                        if lunar.is_leap_month { "閏" } else { "" },
                        lunar.month_name,
                        lunar.day_name
                    ),
                    13.0,
                    INK,
                );
                text(
                    ui,
                    rect.min + vec2(96.0, 196.0),
                    Align2::CENTER_CENTER,
                    format!("{}年　生肖{}", lunar.year_ganzhi, lunar.zodiac),
                    12.0,
                    INK,
                );
            }
            Err(_) => text(
                ui,
                rect.min + vec2(96.0, 185.0),
                Align2::CENTER_CENTER,
                "超出曆法資料範圍／不可用",
                11.0,
                RED,
            ),
        }
    }

    fn event_panel(&self, ui: &mut Ui, rect: Rect) {
        panel(ui, rect, PAPER);
        ui.painter().rect_filled(
            Rect::from_min_size(rect.min, vec2(192.0, 24.0)),
            0.0,
            Color32::from_rgb(0xE0, 0xE9, 0xEC),
        );
        text(
            ui,
            rect.min + vec2(8.0, 12.0),
            Align2::LEFT_CENTER,
            "選日節日／紀念日",
            13.0,
            INK,
        );
        ui.scope_builder(
            UiBuilder::new().max_rect(rect.shrink2(vec2(7.0, 4.0)).with_min_y(rect.min.y + 27.0)),
            |ui| {
                egui::ScrollArea::vertical()
                    .id_salt("events")
                    .show(ui, |ui| {
                        ui.spacing_mut().item_spacing.y = 2.0;
                        ui.label(RichText::new(self.session.selected.to_string()).size(12.0));
                        if let Err(error) = &self.selected_info.lunar {
                            ui.colored_label(RED, format!("讀取失敗：{error}"));
                        } else if let Some(error) = &self.selected_info.event_error {
                            ui.colored_label(RED, format!("事件來源不完整：{error}"));
                        } else if self.selected_info.events.is_empty() {
                            ui.label(RichText::new("此日無列示事件").size(12.0));
                        }
                        for event in &self.selected_info.events {
                            ui.label(RichText::new(&event.name).size(13.0))
                                .on_hover_text(&event.source);
                        }
                        ui.label(RichText::new(crate::events::WARNING).size(10.0));
                        if let Err(error) = &self.selected_info.solar_term {
                            ui.colored_label(
                                RED,
                                RichText::new(format!("節氣不可用：{error}")).size(10.0),
                            );
                        }
                    });
            },
        );
    }

    fn month_panel(&mut self, ui: &mut Ui, rect: Rect) {
        panel(ui, rect, PAPER);
        let week_width = 34.0;
        let column_width = 530.0 / 7.0;
        let row_height = 328.0 / 6.0;
        let start = rect.min + vec2(35.0, 41.0);
        text(
            ui,
            rect.min + vec2(300.0, 190.0),
            Align2::CENTER_CENTER,
            format!("{}\n{}", self.grid.year, month_english(self.grid.month)),
            60.0,
            Color32::from_rgba_unmultiplied(0xDC, 0xE6, 0xE8, 125),
        );
        text(
            ui,
            rect.min + vec2(17.0, 20.0),
            Align2::CENTER_CENTER,
            "週",
            12.0,
            INK,
        );
        for column in 0..7 {
            let x = start.x + (column as f32 + 0.5) * column_width;
            let color = weekday_color(column, true);
            text(
                ui,
                pos2(x, rect.min.y + 14.0),
                Align2::CENTER_CENTER,
                WEEKDAYS[column].trim_start_matches("星期"),
                15.0,
                color,
            );
            text(
                ui,
                pos2(x, rect.min.y + 30.0),
                Align2::CENTER_CENTER,
                WEEKDAY_EN[column],
                11.0,
                color,
            );
        }
        for (index, info) in self.grid.days.iter().enumerate() {
            let row = index / 7;
            let column = index % 7;
            let cell = Rect::from_min_size(
                start + vec2(column as f32 * column_width, row as f32 * row_height),
                vec2(column_width, row_height),
            );
            let in_month = info.date.month() == self.grid.month;
            let selected = info.date == self.session.selected;
            let today = info.date == self.session.today;
            let response = ui.interact(
                cell,
                egui::Id::new(("date", info.date)),
                if calendar::selectable(info.date) {
                    Sense::click()
                } else {
                    Sense::hover()
                },
            );
            response.widget_info(|| {
                egui::WidgetInfo::labeled(
                    egui::WidgetType::Button,
                    calendar::selectable(info.date),
                    info.tooltip(),
                )
            });
            if response.gained_focus() {
                response.scroll_to_me(Some(egui::Align::Center));
            }
            if selected {
                ui.painter().rect_filled(cell.shrink(1.0), 0.0, SELECT);
                ui.painter().rect_stroke(
                    cell.shrink(1.0),
                    0.0,
                    Stroke::new(2.0_f32, Color32::from_rgb(0x69, 0x8E, 0x9E)),
                    StrokeKind::Inside,
                );
            } else if response.hovered() {
                ui.painter().rect_filled(
                    cell.shrink(1.0),
                    0.0,
                    Color32::from_rgb(0xED, 0xF2, 0xF2),
                );
            }
            if response.has_focus() {
                ui.painter().rect_stroke(
                    cell.shrink(3.0),
                    0.0,
                    Stroke::new(1.5_f32, INK),
                    StrokeKind::Inside,
                );
                self.date_keys(ui, info.date);
                response.show_tooltip_text(info.tooltip());
            }
            let p = ui.painter().with_clip_rect(cell.shrink(2.0));
            p.text(
                pos2(cell.center().x, cell.min.y + 18.0),
                Align2::CENTER_CENTER,
                info.date.day(),
                FontId::proportional(27.0),
                weekday_color(column, in_month),
            );
            let label = info.short_label();
            let label = if label.chars().count() > 6 {
                format!("{}…", label.chars().take(5).collect::<String>())
            } else {
                label
            };
            let event_color = if !in_month {
                Color32::from_rgb(0x73, 0x87, 0x8C)
            } else if info
                .events
                .first()
                .is_some_and(|event| event.kind == crate::events::EventKind::SolarTerm)
            {
                GREEN
            } else {
                Color32::from_rgb(0x18, 0x23, 0x18)
            };
            p.text(
                pos2(cell.center().x, cell.min.y + 43.0),
                Align2::CENTER_CENTER,
                label,
                FontId::proportional(12.0),
                event_color,
            );
            if today {
                p.circle_filled(
                    cell.min + vec2(8.0, 8.0),
                    3.0,
                    Color32::from_rgb(0x95, 0x61, 0x3B),
                );
            }
            if response.clicked() {
                response.request_focus();
                self.action = Some(Action::Select(info.date));
            }
            response.context_menu(|ui| {
                if let Some(action) =
                    command_menu(ui, self.settings.always_on_top, self.session.selected)
                {
                    self.action = Some(action);
                }
            });
            response.on_hover_text(info.tooltip());
            if column == 0 {
                let (year, week) = calendar::row_week(info.date);
                text(
                    ui,
                    pos2(rect.min.x + week_width / 2.0, cell.center().y),
                    Align2::CENTER_CENTER,
                    week,
                    12.0,
                    INK,
                );
                ui.interact(
                    Rect::from_min_size(pos2(rect.min.x, cell.min.y), vec2(34.0, row_height)),
                    egui::Id::new(("week", row)),
                    Sense::hover(),
                )
                .on_hover_text(format!("列星期四的 ISO 週：{year} 年第 {week} 週"));
            }
        }
        let ops = Rect::from_min_max(rect.min + vec2(566.0, 1.0), rect.max - vec2(1.0, 1.0));
        ui.painter()
            .rect_filled(ops, 0.0, Color32::from_rgb(0xED, 0xF2, 0xF2));
        for (index, (label, action, enabled)) in [
            (
                "<<",
                Action::Month(-12),
                calendar::navigate_month(self.session.selected, -12).is_ok(),
            ),
            (
                "<",
                Action::Month(-1),
                calendar::navigate_month(self.session.selected, -1).is_ok(),
            ),
            ("今", Action::Today, true),
            (
                ">",
                Action::Month(1),
                calendar::navigate_month(self.session.selected, 1).is_ok(),
            ),
            (
                ">>",
                Action::Month(12),
                calendar::navigate_month(self.session.selected, 12).is_ok(),
            ),
            ("曆", Action::ToggleDisplay, true),
            ("?", Action::Help, true),
        ]
        .into_iter()
        .enumerate()
        {
            let button = Rect::from_min_size(
                ops.min + vec2(2.0, 12.0 + index as f32 * 49.0),
                vec2(29.0, 35.0),
            );
            ui.scope_builder(UiBuilder::new().max_rect(button), |ui| {
                if ui
                    .add_enabled(enabled, egui::Button::new(label).min_size(button.size()))
                    .clicked()
                {
                    self.action = Some(action);
                }
            });
        }
    }

    fn date_keys(&self, ui: &Ui, date: NaiveDate) {
        let delta = ui.input_mut(|input| {
            for (key, delta) in [
                (egui::Key::ArrowLeft, -1),
                (egui::Key::ArrowRight, 1),
                (egui::Key::ArrowUp, -7),
                (egui::Key::ArrowDown, 7),
            ] {
                if input.consume_key(egui::Modifiers::NONE, key) {
                    return Some(delta);
                }
            }
            None
        });
        if let Some(delta) = delta {
            let target = date + chrono::Duration::days(delta);
            if self.grid.days.iter().any(|day| day.date == target) && calendar::selectable(target) {
                ui.memory_mut(|memory| memory.request_focus(egui::Id::new(("date", target))));
            }
        }
    }

    fn memo_panel(&mut self, ui: &mut Ui, rect: Rect) {
        panel(ui, rect, Color32::from_rgb(0xFB, 0xFC, 0xFA));
        let heading = Rect::from_min_size(rect.min, vec2(rect.width(), 27.0));
        ui.painter()
            .rect_filled(heading, 0.0, Color32::from_rgb(0xE0, 0xE9, 0xEC));
        let date = self.session.selected;
        let year = if self.settings.display == CalendarDisplay::Roc {
            format!("民國 {} 年", date.year() - 1911)
        } else {
            format!("西元 {} 年", date.year())
        };
        let title = format!(
            "選日備忘　{} {} 月 {} 日　{}　資訊 i",
            year,
            date.month(),
            date.day(),
            WEEKDAYS[date.weekday().num_days_from_sunday() as usize]
        );
        let response = ui.put(
            heading.shrink2(vec2(8.0, 2.0)),
            egui::Label::new(RichText::new(title).size(13.0)).sense(Sense::click()),
        );
        if response.has_focus() {
            response.show_tooltip_text(self.selected_info.tooltip());
        }
        response.on_hover_text(self.selected_info.tooltip());
        let edit_rect = Rect::from_min_max(rect.min + vec2(7.0, 30.0), rect.max - vec2(5.0, 23.0));
        ui.scope_builder(UiBuilder::new().max_rect(edit_rect), |ui| {
            egui::ScrollArea::vertical()
                .id_salt("memo-scroll")
                .show(ui, |ui| {
                    let mut body = self.session.memo.body.clone();
                    let response = ui.add(
                        egui::TextEdit::multiline(&mut body)
                            .id(egui::Id::new("memo-editor"))
                            .desired_width(577.0)
                            .desired_rows(3)
                            .font(FontId::proportional(14.0))
                            .frame(false),
                    );
                    if response.changed()
                        && let Err(error) = self.session.memo.edit(body)
                    {
                        self.error = Some(format!("{error:#}"));
                    }
                    if response.lost_focus() && self.session.memo.dirty() && self.action.is_none() {
                        self.action = Some(Action::Save);
                    }
                });
        });
        let status = Rect::from_min_max(pos2(rect.min.x, rect.max.y - 21.0), rect.max);
        ui.painter()
            .rect_filled(status, 0.0, Color32::from_rgb(0xE8, 0xEE, 0xF0));
        let (label, color) = match &self.session.memo.state {
            SaveState::Saved => ("已儲存 · Ctrl+S", INK),
            SaveState::Unsaved => ("未儲存 · 輸入停止 1 秒後自動保存", INK),
            SaveState::Saving => ("儲存中", INK),
            SaveState::Failed(_) => ("儲存失敗 · 草稿保留，請重試／另存／明確放棄", RED),
        };
        text(
            ui,
            status.min + vec2(8.0, 10.5),
            Align2::LEFT_CENTER,
            label,
            11.0,
            color,
        );
    }

    fn day_panel(&mut self, ui: &mut Ui, rect: Rect) {
        panel(ui, rect, PAPER);
        ui.scope_builder(UiBuilder::new().max_rect(rect.shrink(10.0)), |ui| {
            ui.horizontal(|ui| {
                if ui.add_enabled(self.session.selected>calendar::MIN,egui::Button::new("前一日")).clicked() {
                    self.action=Some(Action::Select(self.session.selected-chrono::Duration::days(1)));
                }
                ui.heading(self.session.selected.to_string());
                if ui.add_enabled(self.session.selected<calendar::MAX,egui::Button::new("後一日")).clicked() {
                    self.action=Some(Action::Select(self.session.selected+chrono::Duration::days(1)));
                }
            });
            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    ui.set_min_size(vec2(240.0,240.0));
                    if let Some(texture)=&self.image {
                        ui.add(egui::Image::new((texture.id(),texture.size_vec2())).fit_to_exact_size(vec2(240.0,240.0)));
                    } else { ui.colored_label(RED,self.image_error.as_deref().unwrap_or("圖片不可用")); }
                    ui.label(RichText::new("自選圖優先；無自選圖時顯示農曆年生肖").size(10.0));
                });
                ui.vertical(|ui| {
                    ui.set_width(300.0);
                    match &self.selected_info.lunar {
                        Ok(lunar) => { ui.label(lunar.description()); }
                        Err(error) => { ui.colored_label(RED,error); }
                    }
                    ui.label(format!("月干支 {}　日干支 {}",self.selected_info.month_ganzhi,self.selected_info.day_ganzhi));
                    ui.separator();
                    if ui.button("選擇／替換每日圖片").clicked() { self.choose_image(ui.ctx(),false); }
                    if ui.button("移除自選圖（不刪外部原圖）").clicked() {
                        match self.session.repository.set_daily_asset(self.session.selected,None) {
                            Ok(()) => self.reload_images(ui.ctx()),
                            Err(error) => self.error=Some(format!("{error:#}")),
                        }
                    }
                    if ui.button("農民曆詳情").clicked() { self.action=Some(Action::Almanac); }
                    ui.label(RichText::new("JPEG · BMP · ICO · PNG\nWMF／EMF Windows 靜態轉點陣\n20 MiB／8192 px／3200 萬像素").size(11.0));
                });
            });
        });
    }

    fn date_strip(&mut self, ui: &mut Ui, rect: Rect) {
        panel(ui, rect, Color32::from_rgb(0xF8, 0xFB, 0xFA));
        let length = calendar::month_length(self.grid.year, self.grid.month).unwrap_or(31);
        let width = rect.width() / length as f32;
        for day in 1..=length {
            let date = NaiveDate::from_ymd_opt(self.grid.year, self.grid.month, day)
                .expect("validated month");
            let cell = Rect::from_min_size(
                rect.min + vec2((day - 1) as f32 * width, 0.0),
                vec2(width, 28.0),
            );
            let response = ui.interact(cell, egui::Id::new(("strip", date)), Sense::click());
            if date == self.session.selected {
                ui.painter().rect_filled(cell.shrink(1.0), 0.0, SELECT);
                ui.painter().rect_stroke(
                    cell.shrink(1.0),
                    0.0,
                    Stroke::new(1.5_f32, INK),
                    StrokeKind::Inside,
                );
            }
            text(
                ui,
                cell.center() - vec2(0.0, 5.0),
                Align2::CENTER_CENTER,
                day,
                10.0,
                weekday_color(date.weekday().num_days_from_sunday() as usize, true),
            );
            text(
                ui,
                cell.center() + vec2(0.0, 7.0),
                Align2::CENTER_CENTER,
                WEEKDAYS[date.weekday().num_days_from_sunday() as usize].trim_start_matches("星期"),
                9.0,
                INK,
            );
            if response.clicked() {
                self.action = Some(Action::Select(date));
            }
            if response.has_focus() {
                response
                    .show_tooltip_text(format!("{date} · 列週數 {:?}", calendar::row_week(date)));
            }
            if response.gained_focus() {
                response.scroll_to_me(Some(egui::Align::Center));
            }
            response.context_menu(|ui| {
                if let Some(action) =
                    command_menu(ui, self.settings.always_on_top, self.session.selected)
                {
                    self.action = Some(action);
                }
            });
            response.on_hover_text(format!(
                "{date} · 列星期四 ISO 週 {:?}",
                calendar::row_week(date)
            ));
        }
    }

    fn footer(&mut self, ui: &mut Ui, rect: Rect) {
        ui.painter()
            .rect_filled(rect, 0.0, Color32::from_rgb(0xE1, 0xE9, 0xEB));
        ui.scope_builder(
            UiBuilder::new().max_rect(rect.shrink2(vec2(12.0, 3.0))),
            |ui| {
                ui.horizontal(|ui| {
                    // ui.label(RichText::new("CalendarRust · 獨立 Rust 重建").size(11.0));
                    // ui.separator();
                    if ui.button("農民曆").clicked() {
                        self.action = Some(Action::Almanac);
                    }
                    if ui.button("輸出年曆").clicked() {
                        self.action = Some(Action::Mode(ViewMode::YearExport));
                    }
                    if ui.button("說明").clicked() {
                        self.action = Some(Action::Help);
                    }
                    // ui.label(RichText::new("曆法全範圍獨立核對未完成").size(10.0));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button("離開").clicked() {
                            self.action = Some(Action::Exit);
                        }
                    });
                });
            },
        );
    }
}

fn weekday_color(index: usize, in_month: bool) -> Color32 {
    if !in_month {
        Color32::from_rgb(0x73, 0x87, 0x8C)
    } else if index == 0 {
        RED
    } else if index == 6 {
        GREEN
    } else {
        Color32::from_rgb(0x18, 0x23, 0x18)
    }
}

fn month_english(month: u32) -> &'static str {
    [
        "JANUARY",
        "FEBRUARY",
        "MARCH",
        "APRIL",
        "MAY",
        "JUNE",
        "JULY",
        "AUGUST",
        "SEPTEMBER",
        "OCTOBER",
        "NOVEMBER",
        "DECEMBER",
    ][month as usize - 1]
}
