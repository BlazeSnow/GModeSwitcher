use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;

use wmi::{Variant, WMIConnection};

use crate::log;

/// `AWCCWmiMethodFunction.Thermal_Control([in] uint32 arg2, [out] uint32 argr)` lives
/// in this namespace and class.
const NAMESPACE: &str = "root\\WMI";
const CLASS: &str = "AWCCWmiMethodFunction";
const METHOD: &str = "Thermal_Control";

/// Some USTT-capable models reject the usual Balanced value and want this one instead.
const USTT_BALANCED: u32 = 0xA0;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ThermalMode {
    Balanced,
    GMode,
}

impl ThermalMode {
    pub const fn name(self) -> &'static str {
        match self {
            ThermalMode::Balanced => "Balanced",
            ThermalMode::GMode => "G_Mode",
        }
    }

    /// Names that are not one of the modes this app has fall through to `None`, so a
    /// leftover name from an older settings file (e.g. `Custom`, which the app no
    /// longer offers) is safe to read: the caller falls back to Balanced.
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "Balanced" => Some(ThermalMode::Balanced),
            "G_Mode" | "G-Mode" | "GMode" => Some(ThermalMode::GMode),
            _ => None,
        }
    }

    /// Raw mode value; see the switch argument encoding in [`ThermalControl::switch_to`].
    const fn value(self) -> u32 {
        match self {
            ThermalMode::Balanced => 0x97,
            ThermalMode::GMode => 0xAB,
        }
    }
}

/// Commands the UI thread sends to the WMI worker thread.
pub enum Command {
    /// Switch mode; the result is reported back through the worker's sink.
    Apply(ThermalMode),
    /// Switch mode and block the worker until it reports the result into `Sender<bool>`.
    /// Used on exit, where the UI thread has to wait for the restore to happen.
    ApplyBlocking(ThermalMode, Sender<bool>),
    Shutdown,
}

/// Everything the worker thread reports back to the UI thread.
pub enum WorkerEvent {
    Applied {
        mode: ThermalMode,
        ok: bool,
    },
    /// The worker could not set WMI up at all; the UI has to report this and quit.
    InitFailed(String),
}

/// Starts the thread that owns every COM/WMI object.
///
/// COM proxies are apartment-bound, so the connection must be created and used on
/// one single thread; the UI thread only sends commands and receives results. The
/// returned `Err` means the thread could not be started at all — a WMI setup
/// failure is reported later through [`WorkerEvent::InitFailed`].
pub fn spawn(sink: impl Fn(WorkerEvent) + Send + 'static) -> Result<Sender<Command>, String> {
    let (commands, incoming) = mpsc::channel();
    thread::Builder::new()
        .name("gmode-wmi".to_string())
        .spawn(move || run(incoming, sink))
        .map_err(|error| format!("无法启动 WMI 工作线程：{error}"))?;
    Ok(commands)
}

fn run(incoming: Receiver<Command>, sink: impl Fn(WorkerEvent)) {
    log::line(&format!(
        "worker thread started (thread id {:?})",
        thread::current().id()
    ));

    let mut control = match ThermalControl::new() {
        Ok(control) => control,
        Err(message) => {
            log::line(&format!("WMI initialization failed: {message}"));
            sink(WorkerEvent::InitFailed(message));
            return;
        }
    };

    while let Ok(command) = incoming.recv() {
        match command {
            Command::Apply(mode) => {
                let ok = control.apply(mode);
                sink(WorkerEvent::Applied { mode, ok });
            }
            Command::ApplyBlocking(mode, ack) => {
                let _ = ack.send(control.apply(mode));
            }
            Command::Shutdown => break,
        }
    }
    log::line("worker thread stopped");
}

pub struct ThermalControl {
    connection: WMIConnection,
    /// Object path of the class instance the method is invoked on.
    object_path: String,
    /// Name of the method's single input parameter, read from the WMI signature.
    parameter: String,
    /// Balanced value known to be accepted by this machine, if it has been probed yet.
    balanced_value: Option<u32>,
}

impl ThermalControl {
    pub fn new() -> Result<Self, String> {
        log::line(&format!(
            "creating the WMI connection on thread {:?}",
            thread::current().id()
        ));
        let connection = WMIConnection::with_namespace_path(NAMESPACE)
            .map_err(|error| format!("无法连接 WMI 命名空间 {NAMESPACE}：{error}"))?;

        // Fails when the AWCC driver is missing or the machine is not supported.
        let class = connection.get_object(CLASS).map_err(|error| {
            format!("未找到 WMI 类 {CLASS}（未安装 AWCC 驱动或机型不支持）：{error}")
        })?;

        let signature = class
            .get_method(METHOD)
            .map_err(|error| format!("无法读取 {CLASS}.{METHOD} 方法签名：{error}"))?
            .ok_or_else(|| format!("{CLASS}.{METHOD} 没有输入参数"))?;
        let parameter = signature
            .list_properties()
            .map_err(|error| format!("无法读取 {CLASS}.{METHOD} 参数列表：{error}"))?
            .into_iter()
            .next()
            .ok_or_else(|| format!("{CLASS}.{METHOD} 的 WMI 输入签名异常，无法确定参数名"))?;
        log::line(&format!("{CLASS}.{METHOD} input parameter: {parameter}"));

        // The class is not a singleton — calling the method on the class name itself
        // is rejected with WBEM_E_INVALID_METHOD_PARAMETERS (0x8004102F).
        let object_path = instance_path(&connection)?;
        log::line(&format!("{CLASS} object path: {object_path}"));

        Ok(Self {
            connection,
            object_path,
            parameter,
            balanced_value: None,
        })
    }

    pub fn apply(&mut self, mode: ThermalMode) -> bool {
        if mode != ThermalMode::Balanced {
            return self.switch_to(mode.value());
        }
        if let Some(value) = self.balanced_value {
            return self.switch_to(value);
        }
        // Probe once: try 0x97 first, fall back to the USTT value, then remember the winner.
        for value in [ThermalMode::Balanced.value(), USTT_BALANCED] {
            if self.switch_to(value) {
                self.balanced_value = Some(value);
                return true;
            }
        }
        false
    }

    fn switch_to(&self, mode_value: u32) -> bool {
        let arg = ((mode_value & 0xFF) << 8) | 1;
        log::line(&format!(
            "{METHOD}({arg:#06X}) called from thread {:?}",
            thread::current().id()
        ));
        match self.call(arg) {
            Ok(0) => {
                log::line(&format!("{METHOD}({arg:#06X}) succeeded"));
                true
            }
            // 0xFFFFFFFF is the failure sentinel; only 0 counts as success.
            Ok(code) => {
                log::line(&format!(
                    "{METHOD}({arg:#06X}) failed, out parameter is 0x{code:08X}"
                ));
                false
            }
            Err(error) => {
                log::line(&format!("{METHOD}({arg:#06X}) failed: {error}"));
                false
            }
        }
    }

    /// Calls `Thermal_Control(arg)` and returns its `[out]` parameter value.
    ///
    /// The MOF is `void Thermal_Control([in] uint32 arg2, [out] uint32 argr)`, so a
    /// non-`void` method's `ReturnValue` property does not exist here; the out value
    /// is read by its real name instead.
    fn call(&self, arg: u32) -> Result<u32, String> {
        let class = self
            .connection
            .get_object(CLASS)
            .map_err(|error| error.to_string())?;
        let signature = class
            .get_method(METHOD)
            .map_err(|error| error.to_string())?
            .ok_or_else(|| format!("{CLASS}.{METHOD} 没有输入参数"))?;
        let in_params = signature
            .spawn_instance()
            .map_err(|error| error.to_string())?;
        in_params
            .put_property(&self.parameter, arg)
            .map_err(|error| error.to_string())?;

        let out = self
            .connection
            .exec_method(&self.object_path, METHOD, Some(&in_params))
            .map_err(|error| error.to_string())?
            .ok_or_else(|| format!("{CLASS}.{METHOD} 没有返回出参对象"))?;

        let names = out
            .list_properties()
            .map_err(|error| format!("无法列出 {METHOD} 的出参：{error}"))?;
        log::line(&format!("{METHOD} out parameters: {names:?}"));

        let mut values = Vec::with_capacity(names.len());
        for name in &names {
            match out.get_property(name) {
                Ok(value) => {
                    log::line(&format!("{METHOD} out {name} = {}", describe_value(&value)));
                    values.push((name.clone(), value));
                }
                Err(error) => log::line(&format!("{METHOD} out {name} unreadable: {error}")),
            }
        }

        // Prefer `ReturnValue` if the method ever grows one, otherwise take the first
        // out parameter, which is `argr` for this method.
        let (name, value) = values
            .iter()
            .find(|(name, _)| name == "ReturnValue")
            .or_else(|| values.first())
            .ok_or_else(|| format!("{CLASS}.{METHOD} 没有可读出的出参"))?;
        value_as_u32(value)
            .ok_or_else(|| format!("{CLASS}.{METHOD} 的出参 {name} 不是整数：{value:?}"))
    }
}

/// Out parameter as `u32`; signed variants are read as their raw bit pattern.
fn value_as_u32(value: &Variant) -> Option<u32> {
    match value {
        Variant::UI1(value) => Some(u32::from(*value)),
        Variant::UI2(value) => Some(u32::from(*value)),
        Variant::UI4(value) => Some(*value),
        Variant::UI8(value) => u32::try_from(*value).ok(),
        Variant::I1(value) => Some(*value as u32),
        Variant::I2(value) => Some(*value as u32),
        Variant::I4(value) => Some(*value as u32),
        Variant::I8(value) => u32::try_from(*value).ok(),
        _ => None,
    }
}

/// Log rendering of an out value: the raw variant plus its numeric form in hex.
fn describe_value(value: &Variant) -> String {
    match value_as_u32(value) {
        Some(code) => format!("{value:?} (0x{code:08X})"),
        None => format!("{value:?}"),
    }
}

/// Object path of the `AWCCWmiMethodFunction` instance the method is called on.
///
/// The class is not a singleton (its key property is `InstanceName`), so the method
/// has to run on an instance path, exactly like the original implementation does.
fn instance_path(connection: &WMIConnection) -> Result<String, String> {
    let query = format!("SELECT * FROM {CLASS}");
    let instances = connection
        .exec_query(&query)
        .map_err(|error| format!("无法枚举 {CLASS} 的实例（{query}）：{error}"))?;

    let mut seen = 0usize;
    let mut path = None;
    let mut last_path_error = None;
    for instance in instances {
        match instance {
            Ok(instance) => {
                seen += 1;
                if path.is_none() {
                    match instance.path() {
                        Ok(found) => path = Some(found),
                        Err(error) => {
                            log::line(&format!(
                                "cannot read the object path of instance #{seen}: {error}"
                            ));
                            last_path_error = Some(error.to_string());
                        }
                    }
                }
            }
            Err(error) => log::line(&format!("cannot read an instance of {CLASS}: {error}")),
        }
    }
    log::line(&format!("{CLASS} instances found: {seen}"));

    path.ok_or_else(|| match last_path_error {
        Some(error) => format!("{CLASS} 的实例都无法取得 __Path：{error}"),
        None => format!("系统上没有 {CLASS} 实例（未安装 AWCC 驱动或机型不支持）"),
    })
}
