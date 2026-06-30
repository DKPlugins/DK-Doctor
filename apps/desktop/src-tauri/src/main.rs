//! dk-doctor-desktop binary: thin entry point, delegates to the library.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    dk_doctor_desktop_lib::run();
}
