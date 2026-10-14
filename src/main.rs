#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

fn main() {
    if let Err(error) = calendar_rust::ui::run() {
        calendar_rust::platform::show_fatal(&format!("CalendarRust 無法啟動：\n{error:#}"));
        std::process::exit(1);
    }
}
