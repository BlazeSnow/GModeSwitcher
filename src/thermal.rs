use std::collections::HashMap;

use wmi::{Variant, WMIConnection};

use crate::log;

/// `AWCCWmiMethodFunction.Thermal_Control(uint32 arg)` lives in this namespace/class.
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

        let object_path = instance_path(&connection).unwrap_or_else(|| CLASS.to_string());
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
        let result = self.call(arg);
        match &result {
            Ok(Some(return_value)) => log::line(&format!(
                "{METHOD}({:#06X}) returned {return_value:#010X}",
                arg
            )),
            Ok(None) => log::line(&format!("{METHOD}({:#06X}) returned nothing", arg)),
            Err(error) => log::line(&format!("{METHOD}({:#06X}) failed: {error}", arg)),
        }
        // The method returns 0 on success and 0xFFFFFFFF on failure.
        matches!(result, Ok(Some(0)))
    }

    fn call(&self, arg: u32) -> Result<Option<u32>, String> {
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
            .map_err(|error| error.to_string())?;
        let Some(out) = out else {
            return Ok(None);
        };
        let value: Variant = out
            .get_property("ReturnValue")
            .map_err(|error| error.to_string())?;
        Ok(u32::try_from(value).ok())
    }
}

/// First `AWCCWmiMethodFunction` instance path, mirroring the original implementation
/// which calls the method on an instance rather than on the class itself.
fn instance_path(connection: &WMIConnection) -> Option<String> {
    let instances: Vec<HashMap<String, Variant>> = connection
        .raw_query(format!("SELECT * FROM {CLASS}"))
        .ok()?;
    let mut first = instances.into_iter().next()?;
    first.remove("__Path")?.try_into().ok()
}
