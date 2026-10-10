// Tray-only app: never allocate a console window. (Cargo rejects `bin.*.windows_subsystem`
// as an unused manifest key, so the equivalent rustc attribute is used instead.)
#![windows_subsystem = "windows"]

mod autostart;
mod icon;
mod log;
mod paths;
mod settings;
mod thermal;
mod win;

use tao::event::Event;
use tao::event_loop::{ControlFlow, EventLoopBuilder};
use tray_icon::menu::{CheckMenuItem, IsMenuItem, Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tray_icon::{TrayIcon, TrayIconBuilder};

use crate::thermal::{ThermalControl, ThermalMode};

const ID_MODE_BALANCED: &str = "mode.balanced";
const ID_MODE_GMODE: &str = "mode.gmode";
const ID_AUTOSTART: &str = "autostart";
const ID_QUIT: &str = "quit";

enum UserEvent {
    Menu(MenuEvent),
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

    let thermal = match ThermalControl::new() {
        Ok(thermal) => thermal,
        Err(message) => {
            log::line(&format!("WMI initialization failed: {message}"));
            win::error_box("GModeSwitcher 启动失败", &message);
            std::process::exit(1);
        }
    };

    let mut event_loop_builder = EventLoopBuilder::<UserEvent>::with_user_event();
    let event_loop = event_loop_builder.build();
    let proxy = event_loop.create_proxy();
    MenuEvent::set_event_handler(Some(move |event| {
        let _ = proxy.send_event(UserEvent::Menu(event));
    }));

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
    let autostart_entry = CheckMenuItem::with_id(
        ID_AUTOSTART,
        "开机自启动",
        true,
        autostart::is_enabled(),
        None,
    );
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
    if let Some(icon) = icon::tray_icon() {
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
        thermal,
        balanced,
        g_mode,
        autostart_entry,
        mode,
    };

    // Apply the remembered mode on startup.
    app.switch_mode(mode);

    let mut app = Some(app);
    event_loop.run(move |event, _target, control_flow| {
        *control_flow = ControlFlow::Wait;
        let Event::UserEvent(UserEvent::Menu(event)) = event else {
            return;
        };

        let mut quit = false;
        {
            let Some(state) = app.as_mut() else {
                return;
            };
            match event.id.0.as_str() {
                ID_MODE_BALANCED => state.switch_mode(ThermalMode::Balanced),
                ID_MODE_GMODE => state.switch_mode(ThermalMode::GMode),
                ID_AUTOSTART => state.toggle_autostart(),
                ID_QUIT => quit = true,
                other => log::line(&format!("unknown menu event: {other}")),
            }
        }

        if quit {
            if let Some(state) = app.as_mut() {
                state.restore_balanced();
            }
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
    thermal: ThermalControl,
    balanced: CheckMenuItem,
    g_mode: CheckMenuItem,
    autostart_entry: CheckMenuItem,
    mode: ThermalMode,
}

impl App {
    fn switch_mode(&mut self, mode: ThermalMode) {
        log::line(&format!("switching to {}", mode.name()));
        if !self.thermal.apply(mode) {
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

    /// Never leave the machine in G-Mode after quitting.
    fn restore_balanced(&mut self) {
        if self.mode == ThermalMode::Balanced {
            log::line("already Balanced, nothing to restore");
        } else {
            log::line("restoring Balanced before exit");
            if !self.thermal.apply(ThermalMode::Balanced) {
                log::line("failed to restore Balanced before exit");
            }
        }
        settings::save(ThermalMode::Balanced);
    }

    fn refresh_mode_marks(&self) {
        self.balanced
            .set_checked(self.mode == ThermalMode::Balanced);
        self.g_mode.set_checked(self.mode == ThermalMode::GMode);
    }
}
