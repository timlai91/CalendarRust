use crate::{
    calendar,
    storage::{MEMO_LIMIT, MemoRepository},
};
use anyhow::{Result, ensure};
use chrono::{Datelike, Duration, NaiveDate};
use std::time::{Duration as StdDuration, Instant};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ViewMode {
    Month,
    Day,
    YearExport,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SaveState {
    Saved,
    Unsaved,
    Saving,
    Failed(String),
}

pub struct MemoDraft {
    pub date: NaiveDate,
    pub body: String,
    pub saved_body: String,
    pub revision: u64,
    pub saved_revision: u64,
    pub state: SaveState,
    edited: Instant,
}

impl MemoDraft {
    fn new(date: NaiveDate, body: String) -> Self {
        Self {
            date,
            saved_body: body.clone(),
            body,
            revision: 0,
            saved_revision: 0,
            state: SaveState::Saved,
            edited: Instant::now(),
        }
    }

    pub fn edit(&mut self, body: String) -> Result<()> {
        ensure!(
            body.len() <= MEMO_LIMIT,
            "備忘超過 256 KiB；本次新增內容未接受，原草稿仍保留"
        );
        if self.body != body {
            self.body = body;
            self.revision += 1;
            self.state = SaveState::Unsaved;
            self.edited = Instant::now();
        }
        Ok(())
    }

    pub fn dirty(&self) -> bool {
        self.revision != self.saved_revision
    }
    pub fn debounce_due(&self) -> bool {
        self.dirty()
            && self.state == SaveState::Unsaved
            && self.edited.elapsed() >= StdDuration::from_secs(1)
    }

    pub fn discard(&mut self) {
        self.body = self.saved_body.clone();
        self.revision += 1;
        self.saved_revision = self.revision;
        self.state = SaveState::Saved;
    }
}

pub struct Session<R: MemoRepository> {
    pub repository: R,
    pub today: NaiveDate,
    pub selected: NaiveDate,
    pub follow_today: bool,
    pub mode: ViewMode,
    pub memo: MemoDraft,
    pub notice: Option<String>,
}

impl<R: MemoRepository> Session<R> {
    pub fn new(repository: R, today: NaiveDate) -> Result<Self> {
        let selected = calendar::clamp(today);
        let body = repository.load_memo(selected)?;
        Ok(Self {
            repository,
            today,
            selected,
            follow_today: calendar::selectable(today),
            mode: ViewMode::Month,
            memo: MemoDraft::new(selected, body),
            notice: (!calendar::selectable(today))
                .then(|| "系統今天超出 1912–2050，選日已定位支援邊界，今日仍顯示真實日期".into()),
        })
    }

    pub fn flush(&mut self) -> Result<()> {
        if !self.memo.dirty() {
            return Ok(());
        }
        self.memo.state = SaveState::Saving;
        match self.repository.save_memo(self.memo.date, &self.memo.body) {
            Ok(()) => {
                self.memo.saved_body = self.memo.body.clone();
                self.memo.saved_revision = self.memo.revision;
                self.memo.state = SaveState::Saved;
                Ok(())
            }
            Err(error) => {
                self.memo.state = SaveState::Failed(format!("{error:#}"));
                Err(error)
            }
        }
    }

    pub fn select(&mut self, target: NaiveDate) -> Result<()> {
        ensure!(calendar::selectable(target), "此日期超出支援範圍 1912–2050");
        self.flush()?;
        if target != self.selected {
            let body = self.repository.load_memo(target)?;
            self.selected = target;
            self.memo = MemoDraft::new(target, body);
        }
        self.follow_today = target == self.today;
        Ok(())
    }

    pub fn navigate(&mut self, months: i32) -> Result<()> {
        self.select(calendar::navigate_month(self.selected, months)?)?;
        self.follow_today = false;
        Ok(())
    }

    pub fn set_month(&mut self, year: i32, month: u32) -> Result<()> {
        let delta =
            (year - self.selected.year()) * 12 + month as i32 - self.selected.month() as i32;
        self.navigate(delta)
    }

    pub fn step_day(&mut self, delta: i64) -> Result<()> {
        self.select(self.selected + Duration::days(delta))
    }

    pub fn go_today(&mut self) -> Result<()> {
        self.select(calendar::clamp(self.today))?;
        self.follow_today = calendar::selectable(self.today);
        if !self.follow_today {
            self.notice = Some("今天超出支援範圍，定位至邊界而非冒充今天".into());
        }
        Ok(())
    }

    pub fn update_today(&mut self, today: NaiveDate) -> Result<()> {
        if today != self.today {
            self.today = today;
            if self.follow_today {
                if calendar::selectable(today) {
                    self.select(today)?;
                } else {
                    self.follow_today = false;
                    self.notice = Some("今天已超出支援範圍，已停止跟隨今日".into());
                }
            }
        }
        Ok(())
    }

    pub fn set_mode(&mut self, mode: ViewMode) -> Result<()> {
        self.flush()?;
        self.mode = mode;
        Ok(())
    }

    pub fn reload_memo(&mut self) -> Result<()> {
        ensure!(!self.memo.dirty(), "尚有未保存草稿，拒絕載入覆蓋");
        let body = self.repository.load_memo(self.selected)?;
        self.memo = MemoDraft::new(self.selected, body);
        Ok(())
    }
}
