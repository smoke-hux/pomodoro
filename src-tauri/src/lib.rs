//! Application composition. Runtime, IPC commands, and desktop UI live in their own modules.

mod alerts;
mod commands;
mod data_transfer;
mod domain;
mod notifications;
mod quiet;
mod runtime;
mod schema;
mod sound;
mod storage;
mod system;
mod tray;

use runtime::shut_down;
use tray::show_main_window;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _, _| {
            show_main_window(app);
        }))
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .manage(data_transfer::TransferState::default())
        .setup(runtime::setup)
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.hide();
            }
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_snapshot,
            commands::toggle_timer,
            commands::reset_timer,
            commands::skip_phase,
            commands::set_phase,
            commands::select_task,
            commands::add_task,
            commands::update_task,
            commands::toggle_task,
            commands::delete_task,
            commands::capture_interruption,
            commands::set_interruption_handled,
            commands::delete_interruption,
            commands::convert_interruption_to_task,
            commands::update_settings,
            commands::clear_history,
            commands::triage_notification,
            commands::convert_notification,
            commands::delete_notification,
            commands::clear_notifications,
            commands::preview_sound,
            data_transfer::export_data,
            data_transfer::preview_import,
            data_transfer::confirm_import,
            data_transfer::cancel_import,
            data_transfer::export_sessions_csv,
            data_transfer::open_data_location,
        ])
        .build(tauri::generate_context!())
        .expect("error while running Pomodoro")
        // Cleanup used to hang off the tray's Quit item alone, so any other
        // orderly way out left the banners off and the last captured
        // notifications unwritten. Both events, because which of them a given
        // exit produces is the runtime's business; `shut_down` runs once
        // whichever comes first. The exit is never prevented here.
        //
        // Not covered: SIGTERM and the like end the process without either
        // event. That needs a signal handler and is dealt with separately;
        // until then the persisted restore marker repairs the desktop at the
        // next launch.
        .run(|app, event| {
            if matches!(
                event,
                tauri::RunEvent::ExitRequested { .. } | tauri::RunEvent::Exit
            ) {
                shut_down(app);
            }
        });
}
