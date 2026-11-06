//! Read-only desktop appearance, independent of the timer and its data lock.
//! The Settings portal supplies one snapshot and then change signals; an idle
//! app does not poll GSettings or launch child processes to discover its theme.

use std::{
    collections::HashMap,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Condvar, Mutex, PoisonError,
    },
    thread,
    time::Duration,
};

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};
use zbus::{
    blocking::{connection::Builder, Connection, Proxy},
    zvariant::OwnedValue,
};

const APPEARANCE: &str = "org.freedesktop.appearance";
const INTERFACE: &str = "org.gnome.desktop.interface";
const ACCESSIBILITY: &str = "org.gnome.desktop.a11y.interface";
type Settings = HashMap<String, HashMap<String, OwnedValue>>;

#[derive(Clone, Debug, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DesktopAppearance {
    color_scheme: Option<&'static str>,
    accent_color: Option<String>,
    high_contrast: bool,
    reduced_motion: bool,
}

fn accent_color(value: &OwnedValue) -> Option<String> {
    let rgb = <&zbus::zvariant::Structure>::try_from(value).ok()?;
    let fields = rgb.fields();
    if fields.len() != 3 {
        return None;
    }
    let channels: Option<Vec<u8>> = fields
        .iter()
        .map(|field| {
            let value = f64::try_from(field).ok()?;
            (value.is_finite() && (0.0..=1.0).contains(&value))
                .then(|| (value * 255.0).round() as u8)
        })
        .collect();
    let channels = channels?;
    Some(format!(
        "#{:02x}{:02x}{:02x}",
        channels[0], channels[1], channels[2]
    ))
}

fn resolve(settings: &Settings) -> DesktopAppearance {
    let get = |namespace: &str, key: &str| settings.get(namespace)?.get(key);
    let integer = |namespace, key| get(namespace, key).and_then(|v| u32::try_from(v).ok());
    let boolean = |namespace, key| get(namespace, key).and_then(|v| bool::try_from(v).ok());
    let color_scheme = match integer(APPEARANCE, "color-scheme") {
        Some(1) => Some("dark"),
        Some(2) => Some("light"),
        // GNOME's Default style is "no preference" in the standard namespace.
        // Its own key still tells us to use light, even when WebKit's media
        // query has not caught up after leaving Dark style.
        Some(0) | None => {
            match get(INTERFACE, "color-scheme").and_then(|v| <&str>::try_from(v).ok()) {
                Some("prefer-dark") => Some("dark"),
                Some("prefer-light" | "default") => Some("light"),
                _ => None,
            }
        }
        Some(_) => None,
    };
    DesktopAppearance {
        color_scheme,
        accent_color: get(APPEARANCE, "accent-color").and_then(accent_color),
        high_contrast: integer(APPEARANCE, "contrast")
            .map(|value| value == 1)
            .unwrap_or_else(|| boolean(ACCESSIBILITY, "high-contrast").unwrap_or(false)),
        reduced_motion: integer(APPEARANCE, "reduced-motion")
            .map(|value| value == 1)
            .unwrap_or_else(|| boolean(INTERFACE, "enable-animations") == Some(false)),
    }
}

fn relevant(namespace: &str, key: &str) -> bool {
    match namespace {
        APPEARANCE => matches!(
            key,
            "color-scheme" | "accent-color" | "contrast" | "reduced-motion"
        ),
        INTERFACE => matches!(key, "color-scheme" | "enable-animations"),
        ACCESSIBILITY => key == "high-contrast",
        _ => false,
    }
}

#[derive(Default)]
pub(crate) struct AppearanceState {
    current: Mutex<DesktopAppearance>,
    connection: Mutex<Option<Connection>>,
    stopped: AtomicBool,
    retry_lock: Mutex<()>,
    retry_wake: Condvar,
}

impl AppearanceState {
    fn publish(&self, app: &AppHandle, next: DesktopAppearance) {
        let mut current = self.current.lock().unwrap_or_else(PoisonError::into_inner);
        if *current != next && !self.stopped.load(Ordering::Acquire) {
            *current = next.clone();
            let _ = app.emit("desktop-appearance-changed", next);
        }
    }

    fn listen(&self, app: &AppHandle) -> zbus::Result<()> {
        let connection = Builder::session()?
            .method_timeout(Duration::from_secs(2))
            .build()?;
        {
            let mut held = self
                .connection
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            if self.stopped.load(Ordering::Acquire) {
                return Ok(());
            }
            *held = Some(connection.clone());
        }
        let proxy = Proxy::new(
            &connection,
            "org.freedesktop.portal.Desktop",
            "/org/freedesktop/portal/desktop",
            "org.freedesktop.portal.Settings",
        )?;
        // Activate a lazily started portal before subscribing. Subscribe before
        // the definitive read so a change during startup cannot be lost.
        let _: Settings = proxy.call("ReadAll", &(vec![APPEARANCE],))?;
        let changes = proxy.receive_signal("SettingChanged")?;
        let mut settings: Settings =
            proxy.call("ReadAll", &(vec![APPEARANCE, INTERFACE, ACCESSIBILITY],))?;
        settings.retain(|namespace, values| {
            values.retain(|key, _| relevant(namespace, key));
            !values.is_empty()
        });
        self.publish(app, resolve(&settings));
        for message in changes {
            if self.stopped.load(Ordering::Acquire) {
                break;
            }
            let Ok((namespace, key, value)) =
                message.body().deserialize::<(String, String, OwnedValue)>()
            else {
                continue;
            };
            if relevant(&namespace, &key) {
                settings.entry(namespace).or_default().insert(key, value);
                self.publish(app, resolve(&settings));
            }
        }
        Ok(())
    }

    fn stop(&self) {
        // The retry condition and notification share a lock, avoiding a missed
        // shutdown between the worker's condition check and its wait.
        let _retry = self
            .retry_lock
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        self.stopped.store(true, Ordering::Release);
        self.retry_wake.notify_all();
        let held = self
            .connection
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take();
        if let Some(connection) = held {
            let _ = connection.close();
        }
    }
}

#[tauri::command]
pub(crate) fn get_desktop_appearance(
    state: tauri::State<'_, Arc<AppearanceState>>,
) -> DesktopAppearance {
    state
        .current
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .clone()
}

pub(crate) fn start(app: &AppHandle) {
    let state = Arc::new(AppearanceState::default());
    app.manage(Arc::clone(&state));
    let app = app.clone();
    thread::spawn(move || {
        while !state.stopped.load(Ordering::Acquire) {
            let _ = state.listen(&app);
            // Drop a failed connection before retrying. No desktop support is
            // normal on some systems; browser preferences remain the fallback.
            let held = state
                .connection
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .take();
            if let Some(connection) = held {
                let _ = connection.close();
            }
            let lock = state
                .retry_lock
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            let _ = state
                .retry_wake
                .wait_timeout_while(lock, Duration::from_secs(30), |_| {
                    !state.stopped.load(Ordering::Acquire)
                });
        }
    });
}

pub(crate) fn stop(app: &AppHandle) {
    if let Some(state) = app.try_state::<Arc<AppearanceState>>() {
        state.stop();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setting(settings: &mut Settings, namespace: &str, key: &str, value: impl Into<OwnedValue>) {
        settings
            .entry(namespace.into())
            .or_default()
            .insert(key.into(), value.into());
    }

    fn rgb(red: f64, green: f64, blue: f64) -> OwnedValue {
        zbus::zvariant::Value::from((red, green, blue))
            .try_into()
            .unwrap()
    }

    #[test]
    fn absent_settings_leave_browser_fallback_available() {
        assert_eq!(resolve(&Settings::new()), DesktopAppearance::default());
    }

    #[test]
    fn portal_preferences_are_decoded_and_serialized_for_the_frontend() {
        let mut settings = Settings::new();
        setting(&mut settings, APPEARANCE, "color-scheme", 1_u32);
        setting(
            &mut settings,
            APPEARANCE,
            "accent-color",
            rgb(0.9, 0.2, 0.1),
        );
        setting(&mut settings, APPEARANCE, "contrast", 1_u32);
        setting(&mut settings, APPEARANCE, "reduced-motion", 1_u32);
        assert_eq!(
            serde_json::to_value(resolve(&settings)).unwrap(),
            serde_json::json!({
                "colorScheme": "dark", "accentColor": "#e6331a", "highContrast": true, "reducedMotion": true
            })
        );
        setting(&mut settings, APPEARANCE, "color-scheme", 2_u32);
        assert_eq!(resolve(&settings).color_scheme, Some("light"));
        setting(&mut settings, APPEARANCE, "color-scheme", 99_u32);
        assert_eq!(resolve(&settings).color_scheme, None);
    }

    #[test]
    fn invalid_accent_values_are_unset_instead_of_clamped() {
        for color in [
            rgb(-0.01, 0.0, 0.0),
            rgb(0.0, 1.01, 0.0),
            rgb(f64::NAN, 0.0, 0.0),
            rgb(0.0, 0.0, f64::INFINITY),
            OwnedValue::from(42_u32),
        ] {
            assert_eq!(accent_color(&color), None);
        }
        assert_eq!(
            accent_color(&rgb(0.0, 0.5, 1.0)).as_deref(),
            Some("#0080ff")
        );
    }

    #[test]
    fn gnome_default_style_resolves_light_when_the_portal_reports_no_preference() {
        let mut settings = Settings::new();
        setting(&mut settings, APPEARANCE, "color-scheme", 0_u32);
        setting(
            &mut settings,
            INTERFACE,
            "color-scheme",
            zbus::zvariant::Str::from("default"),
        );
        assert_eq!(resolve(&settings).color_scheme, Some("light"));
        setting(&mut settings, APPEARANCE, "color-scheme", 1_u32);
        assert_eq!(resolve(&settings).color_scheme, Some("dark"));
    }

    #[test]
    fn older_gnome_accessibility_keys_work_but_standard_keys_take_priority() {
        let mut settings = Settings::new();
        setting(&mut settings, INTERFACE, "enable-animations", false);
        setting(&mut settings, ACCESSIBILITY, "high-contrast", true);
        assert!(resolve(&settings).reduced_motion);
        assert!(resolve(&settings).high_contrast);
        setting(&mut settings, APPEARANCE, "reduced-motion", 0_u32);
        setting(&mut settings, APPEARANCE, "contrast", 0_u32);
        assert!(!resolve(&settings).reduced_motion);
        assert!(!resolve(&settings).high_contrast);
    }

    #[test]
    fn unrelated_desktop_preferences_are_not_retained() {
        assert!(!relevant(INTERFACE, "font-name"));
        assert!(!relevant("org.gnome.desktop.background", "picture-uri"));
        assert!(relevant(APPEARANCE, "accent-color"));
    }
}
