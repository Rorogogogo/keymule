//! `keymule menubar`: keymule in the macOS menu bar. A key icon whose menu
//! opens keymule's page in its own window (no browser), or in the browser,
//! and can make it start at login.
//!
//! The page server runs inside this process (see [`crate::ui::start`]), so
//! there's no separate background server to go stale. Only one copy runs:
//! it listens on a Unix socket in keymule's config folder, and a second
//! `keymule menubar` (or `keymule ui`) asks it to show its window instead.

use std::io::{Read, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;

use anyhow::{Context, Result};
use tao::event::{Event, StartCause, WindowEvent};
use tao::event_loop::{ControlFlow, EventLoopBuilder};
use tao::platform::macos::{ActivationPolicy, EventLoopExtMacOS, EventLoopWindowTargetExtMacOS};
use tao::window::WindowBuilder;
use tray_icon::menu::accelerator::{Accelerator, Code, Modifiers};
use tray_icon::menu::{CheckMenuItem, Menu, MenuEvent, MenuItem, PredefinedMenuItem, Submenu};
use tray_icon::{Icon, TrayIconBuilder};
use wry::WebViewBuilder;

/// The launchd job that starts it at login.
const LABEL: &str = "dev.keymule.menubar";

enum Msg {
    Open,
    Menu(MenuEvent),
}

fn socket() -> Result<PathBuf> {
    Ok(crate::store::config_dir()?.join("menubar.sock"))
}

/// If the menu-bar app is running, ask it to show its window.
pub fn open_running() -> bool {
    let Ok(path) = socket() else { return false };
    let Ok(mut s) = UnixStream::connect(path) else {
        return false;
    };
    s.write_all(b"open\n").is_ok()
}

fn plist_path() -> Result<PathBuf> {
    let home = std::env::var_os("HOME").context("HOME isn't set")?;
    Ok(PathBuf::from(home).join(format!("Library/LaunchAgents/{LABEL}.plist")))
}

pub fn starts_at_login() -> bool {
    plist_path().is_ok_and(|p| p.exists())
}

/// Make it start at login (or stop that). Takes effect at the next login.
pub fn set_login(on: bool) -> Result<()> {
    let path = plist_path()?;
    if !on {
        if path.exists() {
            std::fs::remove_file(&path)?;
        }
        return Ok(());
    }
    let exe = std::env::current_exe()?.canonicalize()?;
    let esc = |s: &str| {
        s.replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;")
    };
    let plist = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key><string>{LABEL}</string>
  <key>ProgramArguments</key>
  <array>
    <string>{}</string>
    <string>menubar</string>
    <string>--background</string>
  </array>
  <key>RunAtLoad</key><true/>
  <key>ProcessType</key><string>Interactive</string>
</dict>
</plist>
"#,
        esc(&exe.to_string_lossy())
    );
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(&path, plist)?;
    Ok(())
}

/// Start it now, in the background, if it isn't running. With the login
/// item in place, launchd starts it (so closing the terminal doesn't stop
/// it); otherwise it's started on its own, detached from this terminal.
pub fn launch() -> Result<bool> {
    if open_running_quietly() {
        return Ok(false);
    }
    if starts_at_login() {
        let uid = std::process::Command::new("id").arg("-u").output()?;
        let domain = format!("gui/{}", String::from_utf8_lossy(&uid.stdout).trim());
        let plist = plist_path()?;
        let loaded = std::process::Command::new("launchctl")
            .args(["bootstrap", &domain])
            .arg(&plist)
            .stderr(std::process::Stdio::null())
            .status()
            .is_ok_and(|s| s.success());
        let started = loaded
            || std::process::Command::new("launchctl")
                .args(["kickstart", &format!("{domain}/{LABEL}")])
                .stderr(std::process::Stdio::null())
                .status()
                .is_ok_and(|s| s.success());
        if started {
            return Ok(true);
        }
    }
    use std::os::unix::process::CommandExt;
    let exe = std::env::current_exe()?;
    std::process::Command::new(exe)
        .args(["menubar", "--background"])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .process_group(0)
        .spawn()
        .context("starting keymule menubar")?;
    Ok(true)
}

fn open_running_quietly() -> bool {
    socket().is_ok_and(|p| UnixStream::connect(p).is_ok())
}

/// keymule's logo on its 24×24 grid (as in the page's sidebar): a rounded
/// square with a keyhole. `in_tile` is the square, `in_keyhole` the hole.
fn in_tile(x: f32, y: f32) -> bool {
    let r = 6.0;
    let qx = (x - 12.0).abs() - (12.0 - r);
    let qy = (y - 12.0).abs() - (12.0 - r);
    (qx.max(0.0).powi(2) + qy.max(0.0).powi(2)).sqrt() + qx.max(qy).min(0.0) <= r
}

fn in_keyhole(x: f32, y: f32) -> bool {
    let (dx, dy) = (x - 12.0, y - 9.6);
    if dx * dx + dy * dy <= 3.3 * 3.3 {
        return true;
    }
    // The slot: 10.3..13.7 wide at y 11.8, widening to 9.4..14.6 at 17.6.
    if !(11.8..=17.6).contains(&y) {
        return false;
    }
    let t = (y - 11.8) / 5.8;
    (10.3 - 0.9 * t..=13.7 + 0.9 * t).contains(&x)
}

/// Share of the pixel at (px, py) that `inside` covers, 4×4 samples.
fn coverage(px: u32, py: u32, inside: impl Fn(f32, f32) -> bool) -> f32 {
    const SS: u32 = 4;
    let mut hit = 0;
    for sy in 0..SS {
        for sx in 0..SS {
            let x = px as f32 + (sx as f32 + 0.5) / SS as f32;
            let y = py as f32 + (sy as f32 + 0.5) / SS as f32;
            hit += inside(x, y) as u32;
        }
    }
    hit as f32 / (SS * SS) as f32
}

/// The menu-bar icon: the logo in black with the keyhole cut out (macOS
/// tints template images for light and dark menu bars).
fn key_icon() -> Icon {
    const N: u32 = 36;
    // The logo fills 30 of the 36 pixels, centred.
    let to_grid = |v: f32| (v - 3.0) / 30.0 * 24.0;
    let mut rgba = Vec::with_capacity((N * N * 4) as usize);
    for py in 0..N {
        for px in 0..N {
            let a = coverage(px, py, |x, y| {
                let (gx, gy) = (to_grid(x), to_grid(y));
                in_tile(gx, gy) && !in_keyhole(gx, gy)
            });
            rgba.extend_from_slice(&[0, 0, 0, (a * 255.0) as u8]);
        }
    }
    Icon::from_rgba(rgba, N, N).expect("valid icon")
}

/// The Dock icon as PNG: the logo at the macOS icon size (a rounded square
/// with the standard margin), in the page's logo colours.
fn dock_png() -> Vec<u8> {
    const N: u32 = 512;
    let (m, body) = (50.0_f32, 412.0_f32);
    let to_grid = |v: f32| (v - m) / body * 24.0;
    // Black and white, like the page's logo.
    let bg = [0.0_f32; 3];
    let mut rgba = Vec::with_capacity((N * N * 4) as usize);
    for py in 0..N {
        for px in 0..N {
            let a = coverage(px, py, |x, y| in_tile(to_grid(x), to_grid(y)));
            if a == 0.0 {
                rgba.extend_from_slice(&[0, 0, 0, 0]);
                continue;
            }
            let w = coverage(px, py, |x, y| in_keyhole(to_grid(x), to_grid(y)));
            for c in &bg {
                rgba.push((c * (1.0 - w) + 255.0 * w) as u8);
            }
            rgba.push((a * 255.0) as u8);
        }
    }
    let mut out = Vec::new();
    {
        let mut enc = png::Encoder::new(&mut out, N, N);
        enc.set_color(png::ColorType::Rgba);
        enc.set_depth(png::BitDepth::Eight);
        if let Ok(mut w) = enc.write_header() {
            let _ = w.write_image_data(&rgba);
        }
    }
    out
}

/// Bring keymule in front of the app the user is in.
fn activate() {
    use objc2::MainThreadMarker;
    use objc2_app_kit::NSApplication;
    if let Some(mtm) = MainThreadMarker::new() {
        #[allow(deprecated)]
        NSApplication::sharedApplication(mtm).activateIgnoringOtherApps(true);
    }
}

/// Give the app keymule's icon in the Dock and the app switcher.
fn set_dock_icon(png: &[u8]) {
    use objc2::{AnyThread, MainThreadMarker};
    use objc2_app_kit::{NSApplication, NSImage};
    use objc2_foundation::NSData;
    let Some(mtm) = MainThreadMarker::new() else {
        return;
    };
    let data = NSData::with_bytes(png);
    if let Some(img) = NSImage::initWithData(NSImage::alloc(), &data) {
        // SAFETY: on the main thread, with a valid image.
        unsafe { NSApplication::sharedApplication(mtm).setApplicationIconImage(Some(&img)) };
    }
}

pub fn run(background: bool) -> Result<()> {
    if open_running() {
        println!("keymule is already in the menu bar; opened its window.");
        return Ok(());
    }
    let sock = socket()?;
    let _ = std::fs::remove_file(&sock);
    let listener = UnixListener::bind(&sock).context("can't listen for other keymule commands")?;
    let url = crate::ui::start()?;
    let base = url.split('#').next().unwrap_or(&url).to_string();

    let mut event_loop = EventLoopBuilder::<Msg>::with_user_event().build();
    event_loop.set_activation_policy(ActivationPolicy::Accessory);

    // Other keymule commands ask this one to open its window.
    let proxy = event_loop.create_proxy();
    std::thread::spawn(move || {
        for conn in listener.incoming() {
            let Ok(mut conn) = conn else { continue };
            let mut buf = [0u8; 16];
            if conn
                .read(&mut buf)
                .is_ok_and(|n| buf[..n].starts_with(b"open"))
            {
                let _ = proxy.send_event(Msg::Open);
            }
        }
    });
    let proxy = event_loop.create_proxy();
    MenuEvent::set_event_handler(Some(move |e| {
        let _ = proxy.send_event(Msg::Menu(e));
    }));

    // The app's own menu bar, shown while the window is open (the menu-bar
    // icon can end up hidden behind the notch): Start at Login, Hide, Quit,
    // and the usual editing keys for the page.
    let app_login =
        CheckMenuItem::with_id("app-login", "Start at Login", true, starts_at_login(), None);
    let app_quit = MenuItem::with_id(
        "quit",
        "Quit keymule",
        true,
        Some(Accelerator::new(Some(Modifiers::SUPER), Code::KeyQ)),
    );
    let app_menu = Menu::new();
    let main = Submenu::with_items(
        "keymule",
        true,
        &[
            &app_login,
            &PredefinedMenuItem::separator(),
            &PredefinedMenuItem::hide(None),
            &PredefinedMenuItem::hide_others(None),
            &PredefinedMenuItem::separator(),
            &app_quit,
        ],
    )?;
    let edit = Submenu::with_items(
        "Edit",
        true,
        &[
            &PredefinedMenuItem::undo(None),
            &PredefinedMenuItem::redo(None),
            &PredefinedMenuItem::separator(),
            &PredefinedMenuItem::cut(None),
            &PredefinedMenuItem::copy(None),
            &PredefinedMenuItem::paste(None),
            &PredefinedMenuItem::select_all(None),
        ],
    )?;
    let window_menu =
        Submenu::with_items("Window", true, &[&PredefinedMenuItem::close_window(None)])?;
    app_menu.append_items(&[&main, &edit, &window_menu])?;
    app_menu.init_for_nsapp();

    let open = MenuItem::with_id("open", "Open keymule", true, None);
    let browser = MenuItem::with_id("browser", "Open in Browser", true, None);
    let login = CheckMenuItem::with_id("login", "Start at Login", true, starts_at_login(), None);
    let quit = MenuItem::with_id("quit", "Quit keymule", true, None);
    let tray_menu = Menu::with_items(&[
        &open,
        &browser,
        &PredefinedMenuItem::separator(),
        &login,
        &PredefinedMenuItem::separator(),
        &quit,
    ])?;

    let mut tray = None;
    let mut window: Option<tao::window::Window> = None;
    let mut webview = None;
    let dock = dock_png();

    event_loop.run(move |event, target, flow| {
        *flow = ControlFlow::Wait;
        let mut show = false;
        match event {
            Event::NewEvents(StartCause::Init) => {
                tray = TrayIconBuilder::new()
                    .with_icon(key_icon())
                    .with_icon_as_template(true)
                    .with_tooltip("keymule")
                    .with_menu(Box::new(tray_menu.clone()))
                    .build()
                    .ok();
                show = !background;
            }
            Event::UserEvent(Msg::Open) => show = true,
            // A click on the Dock icon.
            Event::Reopen { .. } => show = true,
            Event::UserEvent(Msg::Menu(e)) => match e.id.0.as_str() {
                "open" => show = true,
                "browser" => {
                    let _ = std::process::Command::new("open").arg(&url).status();
                }
                id @ ("login" | "app-login") => {
                    let want = if id == "login" {
                        login.is_checked()
                    } else {
                        app_login.is_checked()
                    };
                    if let Err(err) = set_login(want) {
                        eprintln!("keymule: {err:#}");
                    }
                    login.set_checked(starts_at_login());
                    app_login.set_checked(starts_at_login());
                }
                "quit" => {
                    let _ = std::fs::remove_file(&sock);
                    tray.take();
                    *flow = ControlFlow::Exit;
                }
                _ => {}
            },
            Event::WindowEvent {
                event: WindowEvent::CloseRequested,
                ..
            } => {
                // Closing hides the window; keymule stays in the menu bar,
                // out of the Dock.
                if let Some(w) = &window {
                    w.set_visible(false);
                }
                target.set_activation_policy_at_runtime(ActivationPolicy::Accessory);
            }
            _ => {}
        }
        if show {
            if window.is_none() {
                let w = WindowBuilder::new()
                    .with_title("keymule")
                    .with_inner_size(tao::dpi::LogicalSize::new(1120.0, 780.0))
                    .with_min_inner_size(tao::dpi::LogicalSize::new(720.0, 480.0))
                    .build(target);
                match w {
                    Ok(w) => {
                        let base_nav = base.clone();
                        let wv = WebViewBuilder::new()
                            .with_url(&url)
                            // Links that leave keymule's page open in the browser.
                            .with_navigation_handler(move |to| {
                                if to.starts_with(&base_nav) {
                                    return true;
                                }
                                let _ = std::process::Command::new("open").arg(&to).status();
                                false
                            })
                            .with_new_window_req_handler(|to, _| {
                                let _ = std::process::Command::new("open").arg(&to).status();
                                wry::NewWindowResponse::Deny
                            })
                            .build(&w);
                        match wv {
                            Ok(wv) => webview = Some(wv),
                            Err(err) => eprintln!("keymule: can't show the page: {err}"),
                        }
                        window = Some(w);
                    }
                    Err(err) => eprintln!("keymule: can't open a window: {err}"),
                }
            }
            // While the window is open keymule is a normal app: in the Dock,
            // in Cmd-Tab, with its own menu bar.
            target.set_activation_policy_at_runtime(ActivationPolicy::Regular);
            set_dock_icon(&dock);
            if let Some(w) = &window {
                w.set_visible(true);
                // macOS may not let it take focus when the request came from
                // a terminal command, so lift it above other windows at least.
                w.set_always_on_top(true);
                w.set_always_on_top(false);
                w.set_focus();
            }
            activate();
            let _ = &webview;
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_logo_matches_the_page() {
        assert!(in_keyhole(12.0, 9.6), "the keyhole's circle");
        assert!(in_keyhole(12.0, 17.0), "its slot");
        assert!(!in_keyhole(9.0, 12.0), "beside the slot");
        assert!(in_tile(1.0, 12.0) && !in_tile(0.3, 0.3), "rounded corners");
    }

    #[test]
    fn the_dock_icon_is_a_512px_png() {
        let png = dock_png();
        assert!(png.starts_with(b"\x89PNG\r\n\x1a\n"));
        let decoder = png::Decoder::new(std::io::Cursor::new(png));
        let reader = decoder.read_info().unwrap();
        assert_eq!((reader.info().width, reader.info().height), (512, 512));
    }
}
