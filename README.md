# GModeSwitcher

戴尔 G15 笔记本性能模式切换工具，常驻系统托盘，是戴尔官方 AWCC（Alienware Command Center）
的开源替代品。本项目是 [tcc-g15](https://github.com/AlexIII/tcc-g15) 的 Rust 重写版本。

## 功能

- 纯系统托盘程序，没有窗口界面，不占用任务栏
- 右键托盘图标，在性能模式之间切换（Custom / Balanced / G-Mode）
- 记住上次使用的模式，下次启动时自动恢复
- 开机自启动开关：注册一个以最高权限运行的计划任务，登录后自动启动
- 退出时自动切回 Balanced，不把机器留在 G-Mode 或 Custom

## 运行要求

- **需要管理员权限**：程序通过 WMI 接口读写戴尔热控设置，必须以管理员身份运行
- Windows 10 / 11，64 位
- 戴尔 G15 系列笔记本；其它使用同一套 AWCC WMI 接口的机型可能可用，但未经验证

## 安装与使用

1. 从 [Releases](https://github.com/BlazeSnow/GModeSwitcher/releases) 下载
   `GModeSwitcher-<版本>-windows-amd64.zip` 并解压
2. 双击 `GModeSwitcher.exe`；程序会自行请求管理员权限，在 UAC 提示中选择「是」
3. 托盘出现图标后，右键图标选择要切换到的性能模式

### 已知限制

- 部分机型在切换到 G 模式的瞬间会出现短暂卡顿，这是戴尔热控接口本身的行为

## 从源码构建

需要 Rust 工具链（stable）与 MSVC 构建工具。

```bash
cargo build --release
```

产物为 `target/release/GModeSwitcher.exe`。

## 许可证

本项目以 [GNU General Public License v3.0](LICENSE) 授权。

上游 [tcc-g15](https://github.com/AlexIII/tcc-g15)（© AlexIII）同样以 GPL v3 授权，本项目的
WMI 接口用法与行为参考自该项目，在此致谢。
