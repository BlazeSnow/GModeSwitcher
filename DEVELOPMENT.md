# GModeSwitcher 开发说明

面向开发者的说明。面向使用者的功能、安装与使用见 [README.md](README.md)。
开发时必须遵守的仓库级约定见 [AGENTS.md](AGENTS.md)。

## 架构

纯托盘程序：没有窗口、没有前端，启动后常驻通知区域，只做一件事——切换戴尔笔记本的
热控性能模式。

`src/` 下的模块：

| 文件 | 职责 |
| --- | --- |
| `main.rs` | 入口：单实例、提权检查、托盘图标与右键菜单、事件循环、切换结果的记账 |
| `thermal.rs` | WMI 调用层，以及拥有全部 COM 对象的工作线程 |
| `autostart.rs` | 开机自启动：用 `schtasks` 增删查名为 `GModeSwitcher` 的计划任务；启动时对比登记的 exe 路径，不一致就重建（自愈） |
| `icon.rs` | 按模式解码内嵌的图标，生成托盘图标：平衡模式用蓝底 `#1a6497`，G 模式用红底 `#b82f2f` |
| `settings.rs` | `settings.json` 读写：当前模式，以及启用自启动时登记的可执行文件路径 |
| `log.rs` | 极简文件日志，无控制台时的唯一诊断通道 |
| `paths.rs` | 数据目录 `%APPDATA%\GModeSwitcher`，首次使用时创建 |
| `win.rs` | Win32 封装：命名互斥体、提权检测、`ShellExecuteW` 提权重启、隐藏窗口调用外部命令、`MessageBoxW` |

数据目录下有两个文件：`settings.json`（当前模式与登记的自启动路径）与
`gmode-switcher.log`（运行日志）。

## 必须遵守的约束

### 所有 COM/WMI 调用必须在同一条线程上

`wmi` crate 的 `WMIConnection` 走 MTA（内部用 `CoIncrementMTAUsage`），COM 代理与创建它的
单元绑定。因此程序把**全部** COM 操作收拢在一条专属工作线程里（`thermal::spawn`，线程名
`gmode-wmi`），UI 线程只通过 `mpsc` 发命令、通过 tao 的事件收结果，**不得**接触任何 COM
对象。新增功能时如果需要调用 WMI，一律加进工作线程的命令枚举，不要图省事在 UI 线程直接调。

违反这条的症状是 `0x8001010E`：启动时（事件循环之前）调用可能没事，事件循环跑起来之后再
从 UI 线程调用就会失败。

### WMI 调用规格

- 命名空间 `root\WMI`，类 `AWCCWmiMethodFunction`，方法 `Thermal_Control`。
- 参数编码 `arg = ((mode & 0xFF) << 8) | 1`；参数名从方法签名现场读出（实际是 `arg2`），
  不要硬编码。
- 模式值：`Balanced = 0x97`、`G_Mode = 0xAB`。
- **USTT 回退是实机上真的会走到的路径，不是理论上的兼容措施**：实测 `0x9701` 被拒绝（出参为
  `0xFFFFFFFF`），回落到 `0xA001` 才成功；成功值会被缓存，之后切 Balanced 只发 `0xA001`。
  探测顺序固定为 `0x97` → `0xA0`，取第一个成功的值。
- **必须在实例上调用，不能只用类名**。该类不是 singleton，键属性是 `InstanceName`，类名不是
  合法的实例路径，用类名会得到 `0x8004102F`。实例路径由 `exec_query` 枚举后经
  `IWbemClassWrapper::path()` 取得，取第一个能读到 `__Path` 的实例。
- 实例路径的形态：主体是类名加键值，形如
  `AWCCWmiMethodFunction.InstanceName="ACPI\PNP0C14\AWCC_0"`；`__Path` 也可能返回带
  `\\<计算机名>\root\WMI:` 前缀的全限定形式。`ExecMethod` 两种都接受，代码不要对有没有前缀
  作假设。
- 该类在实测机型上只枚举到 1 个实例，即便如此也不要退回类名：枚举不到实例时应当直接报错。
- 方法的 MOF 是 `void Thermal_Control([in] uint32 arg2, [out] uint32 argr)`：**void 返回，
  没有 `ReturnValue` 属性**，结果在出参 `argr` 里。实测出参对象就只有 `argr` 一个属性，成功时
  值为 `UI4(0)`。
- 判据：出参值**等于 0** 才算成功，`0xFFFFFFFF` 是失败哨兵（实测被拒的模式值返回的正是它）。
  不要把它当成温度那样的数值读。

### 需要管理员权限

访问该 WMI 接口需要管理员权限（未提权时连枚举实例都会返回 access denied）。程序检测到未提权
时会用 `ShellExecuteW` + `runas` 重新拉起自己然后退出。

由此推出：**开机自启动不能用注册表 Run 键**（系统不会静默启动需要提权的程序），必须用计划
任务 + `RunLevel HighestAvailable`，`autostart.rs` 里的 XML 模板就是这么写的。

### 不要解析 `schtasks` 的输出

`schtasks` 打印的内容既不是 UTF-8，也不遵循它自己那份任务 XML 里声明的 UTF-16，而是**系统
ANSI 代码页**。实测数据（中文 Windows）：`schtasks /query /xml` 的全量输出 325869 字节里有
396 个非 ASCII 字节，整体按 UTF-8 解码直接失败（`invalid start byte 0xb5`），按 GBK 才能解出
`<Author>Mozilla</Author><Description>当 Firefox 未运行时，在后台更新…</Description>` 这样的
内容。`win::run_hidden_captured` 的 `from_utf8_lossy` 遇到这些字节只会得到替换字符。

因此：判断任务是否存在**只看退出码**；需要知道「任务里登记的可执行文件路径」时，用
`settings.json` 里我们自己记录的那个路径（`autostart_path`），**不要回读任务**。否则路径里
只要有中文，读回来的字符串永远和真实路径不相等，就会每次启动都重写一次任务；日志里那行
`schtasks … exited with …` 的输出也可能是乱码，看退出码即可。

### 没有控制台，日志是唯一诊断通道

程序是 GUI 子系统（不会弹黑框），出问题不会显示任何东西。`log.rs` 会把关键事件追加写入
`%APPDATA%\GModeSwitcher\gmode-switcher.log`，排查问题先看它。涉及 WMI 的日志行会带上线程
id、实例路径、参数名和出参的十六进制值，足以定位调用是否成功。

日志写入失败会被静默忽略，不能因为日志出错而崩溃。

### 编码

- 源码与文档一律 **UTF-8**。
- `.ps1` 脚本是 **UTF-8 with BOM + CRLF**：Windows PowerShell 5.1 在没有 BOM 时会按 GBK
  解析，中文注释和字符串会乱码。
- Git 的行尾与 LFS 规则见 `.gitattributes`。

## 常见错误码

| 现象 | 原因 |
| --- | --- |
| `0x8004102F`（`WBEM_E_INVALID_METHOD_PARAMETERS`） | object path 用了类名而不是实例路径 |
| `0x8001010E`（`RPC_E_WRONG_THREAD`） | COM 代理在其所属单元之外被使用，即跨线程调了 WMI |
| access denied | 进程没有管理员权限 |

## 构建与运行

```bash
cargo build --release
```

产物是 `target\release\GModeSwitcher.exe`。程序运行时需要管理员权限。

构建启用了**静态链接 CRT**：`.cargo/config.toml` 给 `x86_64-pc-windows-msvc` 加了
`-C target-feature=+crt-static`，因此产物只依赖 Windows 系统 DLL，导入表里没有
`VCRUNTIME140.dll` 与 `api-ms-win-crt-*`，在干净系统上也能直接双击运行。代价是体积约
+96 KB。

这套参数放在 `.cargo/config.toml` 而不是 CI 的 `RUSTFLAGS` 里，是为了让本地构建与发布产物
完全一致；环境变量 `RUSTFLAGS` 的优先级高于该文件，两边都不要设置它。

仓库根目录的脚本：

| 脚本 | 用途 |
| --- | --- |
| `run.ps1` | 构建并提权启动（`-Release` 用 release 构建，`-SkipBuild` 跳过构建） |
| `version.ps1` | 把 `VERSION` 里的版本号同步进 `Cargo.toml` 的 `[package] version` |
| `tag.ps1` | 读 `VERSION` 打 tag 并推送，触发发布工作流 |

改版本号的流程：编辑 `VERSION` → 执行 `version.ps1` → 构建一次让 `Cargo.lock` 跟上 → 执行
`tag.ps1`。

### 自测

```bash
cargo test
```

测试都是各模块内的 `#[cfg(test)] mod tests`：仓库只有 bin target、没有 lib，因此既不需要也
不可能建 `tests/` 集成目录。它们只覆盖纯逻辑：

- 协议参数编码与模式常量、`name()` / `from_name()` 往返，以及老 `settings.json` 里残留的
  `"Custom"` 必须降级为 Balanced 而不是报错；
- 计划任务 XML 的字节形态（UTF-16LE + BOM）与关键节点、`<Command>` 里的可执行文件路径、
  确认没有 `<Arguments>`，以及「登记路径与当前路径不一致才重建任务」的比对规则（含大小写不同
  视为相同、没有记录时重建一次）；
- 两版内嵌托盘图标（平衡版、G 模式版）都能解码成 32×32 RGBA、四角透明，且两版底色确实不同
  （蓝版偏蓝、红版偏红）；
- `settings.json` 的解析与落盘格式，包括模式与 `autostart_path` 互不覆盖、清除记录、以及没有
  `autostart_path` 的老文件仍能解析。

这批测试不访问 WMI、不碰计划任务、不需要管理员权限、不依赖硬件与网络，也不读写 `%APPDATA%`，
在普通开发者会话里直接跑即可。测试构建会跳过 `windows_subsystem` 属性（`#![cfg_attr(not(test),
...)]`），否则测试二进制是 GUI 子系统、输出会被吞掉。

CI **刻意不跑测试**：发布工作流只做构建与打包。

## 发布

打 tag 后由 `.github/workflows/release.yml` 在 Windows runner 上执行 `cargo build --release`，
再用 `ci/package.ps1` 打包成 `GModeSwitcher-<版本>-windows-amd64.zip` 并发布到 GitHub Release。
只构建 amd64。

## 与上游项目的关系

本项目把 [tcc-g15](https://github.com/AlexIII/tcc-g15)（Python + PySide6）的**模式切换**部分
重写为 Rust。上游以 GPLv3 授权，本项目同样以 GPLv3 授权，见 [LICENSE](LICENSE)。

上游具备但本项目**有意不实现**的功能：温度与风扇转速显示、手动风扇调速、Custom 模式、G 键
热键、fail-safe 自动兜底、主窗口界面。上游对这套 WMI 接口的记录在它自己的
`WMI-AWCC-doc.md` 里，需要更多方法 ID 或未文档化接口时可以去查。

## 已知限制与风险

- 出参读取的优先级是「有 `ReturnValue` 就用它，否则取签名里的第一个出参」；这个方法是 void
  返回，实测只有一个出参 `argr`。换机型或驱动版本后若出参顺序变化，取到的值可能不是结果码。
- 退出时 UI 线程最多阻塞 5 秒等待把模式复位为 Balanced；超时会记日志并继续退出，此时模式可能
  没有复位成功。
- 切到 G-Mode 的瞬间系统可能短暂卡顿，这是戴尔热控接口自身的行为，无法规避。
- `exe` 尚未嵌入图标（需要资源编译器 `rc.exe`），`icons/app.ico` 已备好待用。
- 自启动任务若被用户在任务计划程序里**禁用**（而不是删除），程序会把它当成仍然存在：
  `schtasks /query` 对禁用任务同样返回成功。此时若又需要重写任务（程序被移动过、或没有路径
  记录），重写会把 `<Enabled>true</Enabled>` 写回去，等于取消了用户的禁用。
