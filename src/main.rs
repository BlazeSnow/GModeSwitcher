// Tray-only app: never allocate a console window. (Cargo rejects `bin.*.windows_subsystem`
// as an unused manifest key, so the equivalent rustc attribute is used instead.) It is
// skipped when testing, otherwise the test harness would be a GUI binary and its output
// would go nowhere.
#![cfg_attr(not(test), windows_subsystem = "windows")]

mod autostart;
mod build_meta;
mod icon;
mod log;
mod paths;
mod settings;
mod thermal;
mod win;

use std::sync::mpsc::Sender;

use tao::event::Event;
use tao::event_loop::{ControlFlow, EventLoopBuilder};
use tray_icon::menu::{CheckMenuItem, IsMenuItem, Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tray_icon::{TrayIcon, TrayIconBuilder};

use crate::thermal::{Command, ThermalMode, WorkerEvent};

const ID_MODE_BALANCED: &str = "mode.balanced";
const ID_MODE_GMODE: &str = "mode.gmode";
const ID_AUTOSTART: &str = "autostart";
const ID_QUIT: &str = "quit";

enum UserEvent {
    Menu(MenuEvent),
    /// Result of a mode switch, reported by the WMI worker.
    ModeApplied {
        mode: ThermalMode,
        ok: bool,
    },
    /// The worker could not set WMI up at all; there is nothing left to do but quit.
    InitFailed(String),
}

fn main() {
    log::line("--- start ---");

    let instance = match win::SingleInstanceGuard::acquire() {
        Some(guard) => guard,
        None => {
            log::line("another instance is already running, exiting silently");
            return;
        }
    };

    if !win::is_elevated() {
        log::line("not elevated, asking for elevation");
        // Release the mutex so the elevated copy can take it.
        drop(instance);
        if win::relaunch_elevated() {
            log::line("elevated restart requested, exiting");
        } else {
            log::line("elevated restart failed (UAC declined?)");
            win::error_box(
                "GModeSwitcher",
                "无法以管理员身份重新启动。\n切换性能模式需要管理员权限，请在弹出的 UAC 提示中选择“是”。",
            );
        }
        return;
    }
    log::line("running with administrator rights");

    let mode = settings::load();
    log::line(&format!("saved mode: {}", mode.name()));

    // Self-healing for a moved executable: the logon task is repaired here, and the
    // result doubles as the initial check mark of the menu item. Needs the
    // administrator rights confirmed above, so it runs after that check.
    let autostart_enabled = autostart::reconcile_at_startup();

    // The event loop and its proxy have to exist before the worker thread starts,
    // otherwise the worker has nowhere to report its init result to.
    let mut event_loop_builder = EventLoopBuilder::<UserEvent>::with_user_event();
    let event_loop = event_loop_builder.build();
    let proxy = event_loop.create_proxy();
    MenuEvent::set_event_handler(Some({
        let proxy = proxy.clone();
        move |event| {
            let _ = proxy.send_event(UserEvent::Menu(event));
        }
    }));

    // Every COM/WMI object lives on this worker thread; the UI thread never touches
    // COM itself, it only sends commands and receives `UserEvent`s.
    let commands = match thermal::spawn({
        let proxy = proxy.clone();
        move |event| {
            let user_event = match event {
                WorkerEvent::Applied { mode, ok } => UserEvent::ModeApplied { mode, ok },
                WorkerEvent::InitFailed(message) => UserEvent::InitFailed(message),
            };
            let _ = proxy.send_event(user_event);
        }
    }) {
        Ok(commands) => commands,
        Err(message) => {
            log::line(&format!("cannot start the WMI worker: {message}"));
            win::error_box("GModeSwitcher 启动失败", &message);
            std::process::exit(1);
        }
    };

    let menu = Menu::new();
    let header = PredefinedMenuItem::section_header("模式");
    let separator_a = PredefinedMenuItem::separator();
    let separator_b = PredefinedMenuItem::separator();
    let balanced = CheckMenuItem::with_id(
        ID_MODE_BALANCED,
        "Balanced",
        true,
        mode == ThermalMode::Balanced,
        None,
    );
    let g_mode = CheckMenuItem::with_id(
        ID_MODE_GMODE,
        "G-Mode",
        true,
        mode == ThermalMode::GMode,
        None,
    );
    let autostart_entry =
        CheckMenuItem::with_id(ID_AUTOSTART, "开机自启动", true, autostart_enabled, None);
    let quit_entry = MenuItem::with_id(ID_QUIT, "退出", true, None);
    let items: [&dyn IsMenuItem; 7] = [
        &header,
        &balanced,
        &g_mode,
        &separator_a,
        &autostart_entry,
        &separator_b,
        &quit_entry,
    ];
    if let Err(error) = menu.append_items(&items) {
        log::line(&format!("failed to build tray menu: {error}"));
        win::error_box(
            "GModeSwitcher 启动失败",
            &format!("无法创建托盘菜单：{error}"),
        );
        std::process::exit(1);
    }

    let mut builder = TrayIconBuilder::new()
        .with_menu(Box::new(menu))
        .with_tooltip(tooltip(mode));
    if let Some(icon) = icon::tray_icon(mode) {
        builder = builder.with_icon(icon);
    }
    let tray = match builder.build() {
        Ok(tray) => tray,
        Err(error) => {
            log::line(&format!("failed to create tray icon: {error}"));
            win::error_box(
                "GModeSwitcher 启动失败",
                &format!("无法创建托盘图标：{error}"),
            );
            std::process::exit(1);
        }
    };

    let mut app = App {
        tray,
        commands,
        balanced,
        g_mode,
        autostart_entry,
        mode,
    };

    // Apply the remembered mode on startup; the accounting happens when the worker
    // reports the result back as a user event.
    app.switch_mode(mode);

    let mut app = Some(app);
    event_loop.run(move |event, _target, control_flow| {
        *control_flow = ControlFlow::Wait;
        let Event::UserEvent(event) = event else {
            return;
        };

        let mut quit = false;
        let mut init_failure = None;
        {
            let Some(state) = app.as_mut() else {
                return;
            };
            match event {
                UserEvent::Menu(event) => match event.id.0.as_str() {
                    ID_MODE_BALANCED => state.switch_mode(ThermalMode::Balanced),
                    ID_MODE_GMODE => state.switch_mode(ThermalMode::GMode),
                    ID_AUTOSTART => state.toggle_autostart(),
                    ID_QUIT => quit = true,
                    other => log::line(&format!("unknown menu event: {other}")),
                },
                UserEvent::ModeApplied { mode, ok } => state.on_mode_applied(mode, ok),
                UserEvent::InitFailed(message) => init_failure = Some(message),
            }
        }

        if let Some(message) = init_failure {
            // Dropping the app removes the tray icon before the dialog shows up.
            drop(app.take());
            log::line(&format!("WMI worker is unavailable, exiting: {message}"));
            win::error_box("GModeSwitcher 启动失败", &message);
            std::process::exit(1);
        }

        if quit {
            // Quitting leaves the machine in whatever mode it is in now: the mode is
            // not reset on exit, so the next start reapplies the saved one.
            // Dropping the app removes the tray icon before the process ends.
            drop(app.take());
            log::line("--- exit ---");
            std::process::exit(0);
        }
    });
}

fn tooltip(mode: ThermalMode) -> String {
    format!("GModeSwitcher - {}", mode.name())
}

struct App {
    tray: TrayIcon,
    /// Command channel of the WMI worker; the UI thread holds no COM object at all.
    commands: Sender<Command>,
    balanced: CheckMenuItem,
    g_mode: CheckMenuItem,
    autostart_entry: CheckMenuItem,
    mode: ThermalMode,
}

impl App {
    /// Asks the worker to switch mode. The check marks, the tooltip and the settings
    /// file are only touched once the worker reports back in `on_mode_applied`.
    fn switch_mode(&mut self, mode: ThermalMode) {
        log::line(&format!("requesting {} mode", mode.name()));
        if let Err(error) = self.commands.send(Command::Apply(mode)) {
            log::line(&format!("WMI worker is gone, cannot switch mode: {error}"));
        }
    }

    /// Accounting for a finished switch. On success the check marks, the settings
    /// file, the tooltip and the tray icon (which is tinted per mode) all follow the
    /// new mode; on failure the mode did not change, so nothing but the marks is
    /// touched and the icon stays as it was.
    fn on_mode_applied(&mut self, mode: ThermalMode, ok: bool) {
        if !ok {
            log::line(&format!("switching to {} failed", mode.name()));
            win::error_box(
                "GModeSwitcher",
                &format!(
                    "切换到 {} 模式失败。\n请确认已安装 Dell AWCC 热控驱动；部分机型不支持该模式。",
                    mode.name()
                ),
            );
            self.refresh_mode_marks();
            return;
        }

        self.mode = mode;
        settings::save(mode);
        self.refresh_mode_marks();
        if let Some(icon) = icon::tray_icon(mode) {
            let _ = self.tray.set_icon(Some(icon));
        }
        let _ = self.tray.set_tooltip(Some(tooltip(mode)));
        log::line(&format!("mode is now {}", mode.name()));
    }

    fn toggle_autostart(&mut self) {
        let requested = self.autostart_entry.is_checked();
        let result = if requested {
            autostart::enable()
        } else {
            autostart::disable()
        };
        let action = if requested { "启用" } else { "取消" };
        match result {
            Ok(()) => log::line(&format!("autostart {action} succeeded")),
            Err(message) => {
                log::line(&format!("autostart {action} failed: {message}"));
                win::error_box(
                    "GModeSwitcher",
                    &format!("{action}开机自启动失败：\n{message}"),
                );
            }
        }
        // Trust the task scheduler, not the click, and show the real state.
        self.autostart_entry.set_checked(autostart::is_enabled());
    }

    fn refresh_mode_marks(&self) {
        self.balanced
            .set_checked(self.mode == ThermalMode::Balanced);
        self.g_mode.set_checked(self.mode == ThermalMode::GMode);
    }
}
