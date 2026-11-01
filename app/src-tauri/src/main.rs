// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    aon::run_speaker_guard_if_asked();
    aonin_lib::run()
}
