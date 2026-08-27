# 00 · 净室实现总纲（Clean-Room Charter)

> 读者：**实现者**（负责用 Rust 重写 Lenovo 硬件控制中心的人/模型）。
> 作者：接口规格组（本文档系列的撰写方）。
> 状态：v1 · 2026-08-26

## 1. 项目目标

从零实现一个跨平台（Windows / Linux）的命令行硬件控制中心 **lctrl**（暂定名，实现者可改），
覆盖 Lenovo Vantage + 联想电脑管家（PCManager) + Lenovo MagiCenter 三套官方软件在
**Lenovo 21VG (ThinkBook, Panther Lake 平台）** 上的全部硬件控制功能，并为 Panther Lake
的低功耗调优提供一等支持。

- 形态：无状态 CLI 为主；可选常驻 daemon（仅用于事件监听/自动策略，CLI 的每个功能都必须能在无 daemon 时工作）。
- 语言：Rust。实现者**不得**复制、拆解任何 Lenovo/Intel/Microsoft 二进制；仅依据本系列文档与公开资料（Intel ESIF SDK、USB-IF MBIM/UVC 规范、Linux 内核文档等）实现；
  只能依据本系列文档（行为与接口规范）实现。
- 排除范围：官方软件中的应用商店、广告、浏览器、杀毒/安全扫描、账号体系、遥测上报
  （详见各功能文档的「净室范围」标注）。

## 2. 来源声明（净室属性）

本系列文档是**净室接口规格**。其全部信息仅来自以下三类来源：

1. **产品内部接口资料**：Lenovo Vantage / 电脑管家 / MagiCenter 各组件随产品发行的
   内部接口说明、组件字符串资源、INF/注册表参数与常量表；
2. **目标机实机只读验证**：在目标机上以只读方式枚举/调用公开管理接口
   (WMI、设备接口、服务控制、命名管道、sysfs)得到的实测数据；
3. **公开规范**：Intel 公开 ESIF SDK (Apache-2.0)、USB-IF MBIM/UVC 规范、
   Microsoft WDF/WMI 公开文档、Linux 内核文档。

本系列文档**不包含**任何通过拆解、提取厂商二进制而获得的信息；
文档中出现的地址/常量/类名均为上述来源中的接口事实。实现者可安全地仅依据本系列文档开发。

## 3. 净室法律边界（实现者必须遵守）

1. **隔离**：规格组与实现组不共享代码。本系列文档是唯一的信息传递媒介。
2. **许可与合规**：目标是对用户自有硬件的互操作控制（interoperability），属于
   合理使用范畴；但实现者不得规避任何激活/授权机制（本项目中不存在此类机制），
   不得分发 Lenovo 的任何二进制、驱动或资源文件。
3. **文档自足**：若某接口在文档中描述不清，实现者应向规格组提问，而不是自行分析厂商二进制。
4. **商标**：不得在产物中使用 "Lenovo"、"ThinkBook"、"Vantage" 等商标作为产品名；
   可在兼容性说明中引用。

## 4. 文档地图

| 文档 | 内容 |
|---|---|
| `00-cleanroom-charter.md` | 本文档：方法论、边界、全局约定 |
| `01-hal-interfaces.md` | 硬件抽象层：设备节点、IOCTL 表、WMI 类/方法、ACPI 方法、EC 通道 |
| `02-power-battery.md` | 电源与电池：充电阈值、养护模式、快充、适配器检测、USB 供电 |
| `03-thermal-performance.md` | 散热与性能：性能模式、风扇、温度墙、PL1/PL2、DBDC |
| `04-peripherals.md` | 外设：键盘/背光/Fn 键、触控板、面板（刷新率/色彩）、摄像头/麦克风、智能感应、音频 |
| `05-bios-settings.md` | BIOS 设置读写接口（WMI Lenovo_* 族） |
| `06-magicbay.md` | MagicBay 磁吸配件：LTE(MBIM)、摄像头、扩展屏、热插拔协商 |
| `07-pantherlake-tuning.md` | Panther Lake 低功耗调优：DPTF/IPF、RAPL、调度提示、策略集 |
| `08-architecture.md` | Rust CLI 架构设计（命令树、daemon、插件、错误模型） |
| `09-linux-backend.md` | Linux 后端映射（ideapad_laptop/sysfs/ACPI/内核补丁点） |
| `10-config-schema.md` | 配置文件 schema 与调优 DSL |
| `11-sysupdate-diagnostics.md` | 系统更新/硬件诊断/系统信息（含净室取舍清单） |
| `A-wmi-reference.md` | root\WMI Lenovo 类全量参考（实机采集 + 人工注释） |
| `B-evidence.md` | 来源与验证说明：每条结论的来源分类与验证方式 |

## 5. 全局技术事实（实现前先读）

### 5.1 目标机器

- 机型：Lenovo 21VG,`THINKBOOK_14_G8+_IPH`（SMBIOS)，用户确认机型为 ThinkBook 14+ 2026；同平台文档通用。
- CPU:Intel Panther Lake（具备 Intel IPF/DPTF 热管理栈，设备 `ACPI\INTC10D4/10D5/10D8`)。
- EC/电源控制 ACPI 设备：`ACPI\VPC2004`(Lenovo ACPI-Compliant Virtual Power Controller)。
- WMI 入口：`root\WMI` 下约 60 个 `LENOVO_*` / `Lenovo_*` 类（全量签名见附录 A)。

### 5.2 官方软件栈分层（分析结论，详见各专项文档）

```
┌─ LenovoVantage (UWP 前端, .NET) ────────┐  ┌─ PCManager (WPF/CEF 混合) ──┐  ┌─ MagiCenter (Electron) ─┐
│  RpcClient 族 (PowerRpcClient 等)       │  │ 插件宿主 WSPluginHost 等    │  │ asar + .node 原生模块   │
└──────────────┬──────────────────────────┘  └──────────┬─────────────────┘  └───────────┬─────────────┘
               ▼ RPC (named pipe)                        ▼ 自研 IPC + WMI                  ▼ USB/MBIM/Windows API
┌─ LenovoVantageService (5.1.2607.5, .NET, Addin 宿主) ──┴─ LenovoPcManagerService ────────┐
│  36 个 Addin（电源/设备/智能/更新/诊断…），全部经统一 RpcServer 暴露                       │
└──────────────┬──────────────────────────┬─────────────────────────────┬────────────────┘
               ▼ WMI (root\WMI LENOVO_*)    ▼ 自研 IPC                    ▼ Windows Mobile Broadband (MBIM)
┌─ ACPI 固件 (PNP0C14 WMI 映射, VPC2004) ──┴─ LNVDispatcherService ─┬─ LenovoUtilityService (Fn 键) ─┐
└──────────────┬──────────────────────────┬────────────────────────┴───────────────┬──────────────┘
               ▼                            ▼                                        ▼
        AcpiVpc.sys (EC 通道)      lenovoDriverBus.sys (虚拟总线)          LnvDrvFdn.sys (文件监控 minifilter)
               ▼                            ▼                                        ▼
        Embedded Controller          Lenovo AI Turbo / Dispatcher           进程活动遥测（调度输入）
               │
               ▼
        Intel DPTF/IPF (ipfsvc.exe, Dptf*.dll 策略库) — Panther Lake 功耗/热管理
```

### 5.3 关键通道速查

| 通道 | 用途 | 细节文档 |
|---|---|---|
| `root\WMI` `LENOVO_GAMEZONE_DATA` | 性能模式/风扇/温度/超频/面板 | 01, 03 |
| `root\WMI` `LENOVO_UTILITY_DATA` | 杂项功能开关（Fn 锁定等） | 01, 04 |
| `root\WMI` `LENOVO_OTHER_METHOD` | 通用特性读写（GetFeatureValue/SetFeatureValue/GetDataByCommand/GetDataByPackage) | 01 |
| `root\WMI` `LENOVO_SR_DATA` | 智能感应/EC 监控 | 04 |
| `root\WMI` `Lenovo_BiosSetting` 等 10 类 | BIOS 设置读写 | 05 |
| `AcpiVpc.sys` 设备 IOCTL | EC 直连（保留通道，优先走 WMI) | 01 |
| Intel DPTF IPC (IpcServer) | Panther Lake 功耗策略热切换 | 07 |
| USB VID_17EF&PID_7005 (MBIM/UVC/显示） | MagicBay 配件 | 06 |

### 5.4 实现优先级（规格组建议）

- P0（解放用户刚需）：性能模式切换、充电阈值/养护模式、风扇读取与控制、电池信息、
  键盘背光、Fn 键行为、BIOS 设置读写、面板刷新率。
- P1:Panther Lake 功耗调优（PL1/PL2、DPTF 策略、调度提示）、智能感应开关、
  摄像头/麦克风隐私开关、AlwaysOnUSB、MagicBay LTE。
- P2:AI 场景自适应（以用户自定义规则替代官方 ML 模型）、硬件自检、MagicBay 显示/摄像头配件。

## 6. 术语表

| 术语 | 含义 |
|---|---|
| EC | Embedded Controller，嵌入式控制器（电池/风扇/热键硬件控制者） |
| VPC2004 | Lenovo ACPI 虚拟电源控制器设备 ID,WMI 功能的固件载体 |
| DPTF / IPF | Intel Dynamic Tuning / Innovation Platform Framework，热与功耗策略框架 |
| DTT | Intel Dynamic Tuning Technology(`ipfsvc.exe` 服务） |
| RAPL | Running Average Power Limit,Intel 功耗限制 MSR/寄存器机制 |
| PL1/PL2 | 长时/短时功耗限制 |
| DBDC | Dynamic Battery Drain Control，电池直充限流（目标机实测三档阈值，见 03/07 文档） |
| MBIM | Mobile Broadband Interface Model,USB 4G/LTE 标准控制协议 |
| MagicBay | 联想磁吸扩展接口（USB 3.0 物理层，复合设备） |
| Addin | VantageService 的插件单元（独立 exe，经 RPC 注册） |
