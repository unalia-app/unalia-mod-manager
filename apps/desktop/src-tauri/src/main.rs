// Never a console on Windows, even in dev: an `nxm://` link starts the app
// again (which hands the link to the instance already open, then exits),
// and a console would flash on screen. Dev logs still reach the terminal of
// `tauri dev`, which reads the process output.
#![windows_subsystem = "windows"]

fn main() {
    modalia_desktop_lib::run()
}
