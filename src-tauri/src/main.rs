//! Media Identifier executable.

// Hide the console window on Windows release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    media_identifier_lib::run();
}
