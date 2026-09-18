//! System tray icon for desktop runs (Windows and macOS).
//!
//! `tray-icon` needs a GUI event loop on the thread that owns the icon, and on
//! macOS that loop must run on the main thread before the icon is created.
//! [`run`] therefore hijacks the main thread with a `tao` event loop while the
//! HTTP server keeps running on a worker thread; picking Quit triggers a
//! graceful server shutdown.
//!
//! The module is only compiled on Windows and macOS. Linux serves headless.

use std::sync::Arc;
use std::sync::mpsc::Receiver;

use tao::event::{Event, StartCause};
use tao::event_loop::{ControlFlow, EventLoopBuilder};
use tao::platform::run_return::EventLoopExtRunReturn;
use tokio::sync::Notify;
use tray_icon::menu::{Menu, MenuEvent, MenuId, MenuItem, PredefinedMenuItem};
#[cfg(windows)]
use tray_icon::{MouseButton, MouseButtonState};
use tray_icon::{TrayIcon, TrayIconBuilder, TrayIconEvent};

use crate::error::{Error, Result};

mod icon;

/// Menu id of the "Open dashboard" item.
const MENU_OPEN: &str = "open-dashboard";
/// Menu id of the "Quit" item.
const MENU_QUIT: &str = "quit";

/// Everything the tray needs from the server side.
pub struct Options {
    /// `host:port` used to build the dashboard URL.
    pub address: String,
    /// Signalled when the user quits; the server shuts down gracefully.
    pub shutdown: Arc<Notify>,
    /// Fires when the server thread stops (Ctrl+C, fatal error) so the tray
    /// exits too.
    pub server_done: Receiver<()>,
}

/// Tray events forwarded to the event loop from the global handlers.
enum UserEvent {
    Menu(MenuId),
    Tray(TrayIconEvent),
    ServerStopped,
}

/// Runs the tray until the user quits or the server stops.
pub fn run(options: Options) -> Result<()> {
    let mut event_loop = EventLoopBuilder::<UserEvent>::with_user_event().build();
    let proxy = event_loop.create_proxy();

    let menu_proxy = proxy.clone();
    MenuEvent::set_event_handler(Some(move |event: MenuEvent| {
        let _ = menu_proxy.send_event(UserEvent::Menu(event.id));
    }));
    let tray_proxy = proxy.clone();
    TrayIconEvent::set_event_handler(Some(move |event: TrayIconEvent| {
        let _ = tray_proxy.send_event(UserEvent::Tray(event));
    }));

    let done_proxy = proxy.clone();
    std::thread::spawn(move || {
        let _ = options.server_done.recv();
        let _ = done_proxy.send_event(UserEvent::ServerStopped);
    });

    let address = options.address;
    let shutdown = options.shutdown;
    let mut tray: Option<TrayIcon> = None;

    event_loop.run_return(|event, _target, control_flow| match event {
        Event::NewEvents(StartCause::Init) => match build(&address) {
            Ok(icon) => tray = Some(icon),
            Err(error) => {
                tracing::warn!(%error, "cannot create the tray icon; serving without it");
            }
        },
        Event::UserEvent(UserEvent::Menu(id)) => {
            if id.0 == MENU_OPEN {
                open_dashboard(&address);
            } else if id.0 == MENU_QUIT {
                // `notify_one` rather than `notify_waiters`: a Quit pressed
                // while the server is still binding must not be dropped.
                shutdown.notify_one();
                *control_flow = ControlFlow::Exit;
            }
        }
        Event::UserEvent(UserEvent::Tray(event)) => {
            if is_left_click(&event) {
                open_dashboard(&address);
            }
        }
        Event::UserEvent(UserEvent::ServerStopped) => *control_flow = ControlFlow::Exit,
        _ => {}
    });

    Ok(())
}

/// Builds the tray icon with its context menu.
fn build(address: &str) -> Result<TrayIcon> {
    let menu = Menu::new();
    menu.append_items(&[
        &MenuItem::with_id(MENU_OPEN, "Open dashboard", true, None),
        &PredefinedMenuItem::separator(),
        &MenuItem::with_id(MENU_QUIT, "Quit alnair-router", true, None),
    ])
    .map_err(|error| Error::Internal(format!("cannot build the tray menu: {error}")))?;

    let builder = TrayIconBuilder::new()
        .with_menu(Box::new(menu))
        .with_tooltip(format!("alnair-router - {}", dashboard_url(address)))
        .with_icon(icon::router()?);

    // Windows: left click opens the dashboard, menu stays on right click.
    #[cfg(windows)]
    let builder = builder.with_menu_on_left_click(false);
    // macOS: the menu bar tints template images for light and dark themes.
    #[cfg(target_os = "macos")]
    let builder = builder.with_icon_as_template(true);

    builder
        .build()
        .map_err(|error| Error::Internal(format!("cannot create the tray icon: {error}")))
}

/// URL of the embedded dashboard.
fn dashboard_url(address: &str) -> String {
    format!("http://{address}")
}

/// Opens the dashboard in the default browser.
fn open_dashboard(address: &str) {
    let url = dashboard_url(address);

    #[cfg(windows)]
    let spawned = {
        use std::os::windows::process::CommandExt;

        // CREATE_NO_WINDOW: the GUI-subsystem binary has no console, and this
        // keeps `cmd` from allocating one just to launch the browser.
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;

        std::process::Command::new("cmd")
            .args(["/C", "start", "", &url])
            .creation_flags(CREATE_NO_WINDOW)
            .spawn()
    };
    #[cfg(target_os = "macos")]
    let spawned = std::process::Command::new("open").arg(&url).spawn();

    if let Err(error) = spawned {
        tracing::warn!(%error, "cannot open the dashboard in a browser");
    }
}

/// True for the left-button release that should open the dashboard.
///
/// Windows has no menu-on-click convention, so left-click is a shortcut for
/// "Open dashboard". macOS opens the menu on any click, so it stays menu-only.
#[cfg(windows)]
fn is_left_click(event: &TrayIconEvent) -> bool {
    matches!(
        event,
        TrayIconEvent::Click {
            button: MouseButton::Left,
            button_state: MouseButtonState::Up,
            ..
        }
    )
}

#[cfg(target_os = "macos")]
fn is_left_click(_event: &TrayIconEvent) -> bool {
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_dashboard_urls() {
        assert_eq!(dashboard_url("127.0.0.1:7878"), "http://127.0.0.1:7878");
    }

    #[test]
    fn menu_ids_are_stable() {
        assert_eq!(MENU_OPEN, "open-dashboard");
        assert_eq!(MENU_QUIT, "quit");
    }
}
