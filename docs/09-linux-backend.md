# 09 · Linux 后端映射规范 (Linux Backend Mapping)

> 读者:**实现者**(用 Rust 实现 Linux 后端的工程师 / 模型)。
> 作者:接口规格组。
> 状态:v1 · 2026-08-27。
> 目标硬件:Lenovo ThinkBook 21VG (Panther Lake), `ACPI\VPC2004`。
> 参考上游:Linux 内核 `drivers/platform/x86/ideapad-laptop.c`、
> `Documentation/ABI/stable/sysfs-platform-ideapad-laptop`、
> `Documentation/ABI/stable/sysfs-class-power`、
> `Documentation/ABI/stable/sysfs-class-hwmon`、
> `Documentation/ABI/stable/sysfs-devices-system-cpu`。
> 引用内核版本:**v6.12–v6.14 主线**(2024-11 ~ 2025-01 窗口),
> 具体 API 存在时注明版本号;若某节点在稳定主线缺失,给出补丁点。

---

## 1. 设计原则与总体策略

### 1.1 三条映射路径

Windows 后端(见 01 文档)走四条通道:WMI `LENOVO_*`、
`AcpiVpc.sys` / `\Device\EnergyDrv` IOCTL、
`LenovoProcessManagement` / `LenovoFnAndfunctionKeys` 服务控制、
BIOS WMI 设置类(`Lenovo_BiosSetting` 等 10 类)。

Linux 后端走三条路径,优先级由高到低:

| 优先 | 路径 | 承担 | 对应 Windows 通道 |
|---|---|---|---|
| 1 | `ideapad-laptop` sysfs + hwmon + power_supply | 电池/充电/风扇/摄像头/触控板/Fn 灯 | `EnergyDrv` IOCTL + `LENОВО_GAMEZONE_DATA` 的一部分 |
| 2 | `acpi_call` / `acpi_osi` 或 `devmem` / `_DSM` 调用 | 内核驱动尚未暴露的 WMI 方法 | 未映射的 WMI 方法 / `Lenovo_BiosSetting` |
| 3 | 通用 Linux 栈(UPower、DRM、ALSA、libinput、ModemManager、thermald) | 面板/电源方案/音频/网络/摄像头/LTE | `LenovoProcessManagement` / WMI `Win32_*` |

**核心判断**:`EnergyDrv` 和 `LenovoProcessManagement` 是 **Lenovo 私有闭源栈**,
Linux 上**不存在对等物**。它们的**功能**可以等价实现(读/写 EC 寄存器、
控制热/功耗策略),但**通道**必须替换。

### 1.2 能力判定矩阵 — Linux 后端对 Windows 功能面的覆盖度

| 02 文档(电源电池) | Linux 覆盖 | 主要通道 |
|---|---|---|
| 充电阈值 / 养护模式 (Storage / Conservation) | ✅ 原生 | `sysfs-platform-ideapad-laptop:conservation_mode` |
| 快充 (Express) | ⚠️ 部分 | `ideapad` 6.12+ 增加 `fast_charge`(见 §3 内核补丁点) |
| 夜间充电 / 定时 | ⚠️ 需要实现者自研 daemon + systemd timer | — |
| 电池信息 (容量/循环/健康) | ✅ 原生 | `/sys/class/power_supply/BAT0/*` |
| 电源计划切换 (节能/平衡/高性能) | ✅ 原生 | `intel_pstate/energy_performance_preference` + `upower --daemon-mode` |
| 03 文档(热性能) | | |
| 智能冷却 / 野兽 / 节能 (ITS / Dispatcher) | ⚠️ 语义替代 | `intel_pstate` + `thermald` 联合(§5) |
| 风扇曲线 | ⚠️ 部分 | `hwmon fan_*` / `fan_pwm` (`ideapad` 6.13+ 暴露) |
| GPU TDP / CPU PL1 PL2 | ✅ 原生 | `intel_rapl`、`energy_perf_bias`、`RAPL` sysfs |
| Smart Fan Mode | ❌ 缺失 | 需 `acpi_call` 直调 VPC2004 `_DSM` |
| 04 文档(外设) | | |
| 键盘背光 | ✅ 原生 | `ideapad:kbd_backlight` / `sysfs-class-led` |
| Fn/Ctrl 互换 / F1-F12 主功能 | ⚠️ 部分 | 无 sysfs,需 `acpi_call` + EC register 写(§4.5) |
| Win 键锁定 / 触控板开关 | ✅ 原生 | `ideapad:touchpad` / `input` 层 + `xinput` |
| 摄像头 / 麦克风隐私开关 | ✅ 原生 | `ideapad:camera_power` / `udev` |
| 面板刷新率 (60/120) | ✅ 原生 | DRM `mode` (见 §6) |
| SmartColor / 护眼 / PIP / HDR | ⚠️ 部分 | DRM `ctm/gamma` + 用户态;PIP/MPRT 无原生 |
| 智能感知 (SmartSense / IDEA2002) | ❌ 缺失 | 无内核驱动,需 `acpi_call` + IR 摄像头 uvcvideo |
| 05 文档(BIOS 设置) | ❌ 缺失 | 需要 `acpi_call` + 逐方法映射,或内核补丁(§4) |
| 06 文档(MagicBay) | ✅ 原生 | ModemManager + v4l2 + DRM(§7) |
| 07 文档(Panther Lake) | ⚠️ 部分 | `intel_lpmd` / `thermald` / `intel_pstate` 替代 DPTF |

**关键事实**:除 Smart Fan、BIOS WMI 设置类、SmartEngine AI 场景引擎外,
**Windows 端的绝大部分硬件控制功能在 Linux 上都有原生或近似对等的 sysfs/内核 API**,
不需要自研驱动。

### 1.3 权限模型总纲

Linux 后端的**大多数节点默认 `root:root 0644` 或 `root:root 0444`**,
普通用户不可写。策略:

- **只读节点**(`/sys/class/power_supply/*`、`hwmon/temp*_input`、`/proc/zoneinfo`):
  默认可读,直接 `open(O_RDONLY)` 即可。
- **可写节点**(风扇、充电阈值、摄像头开关等):默认 root。
  通过 udev 规则把 `ATTR{conservation_mode}` 等节点的 group 改为
  `plugdev` / `lctrl` / `power`,并把 `uaccess` 权限开放给组内成员。
- **需要 CAP_SYS_ADMIN 的操作**(加载 `acpi_call` 模块、`devmem` 写入):
  只在 daemon 模式下使用,CLI 单次调用走 `polkit` action 放行。

具体 udev 规则样例见 §8。

---

## 2. WMI / EnergyDrv / 服务控制 → Linux 通道映射表(全表)

下表是**本文档的入口点**。实现者在实现某条命令时,先看此表确定 Linux 通道,
再回查对应章节的细节。

| 功能 | Windows 通道 | Linux 通道 | 内核支持起始版本 | 见本文 § |
|---|---|---|---|---|
| 充电阈值 (Storage / Normal) | `IOCTL_ENERGYDRV_STORAGE_OPEN/CLOSE` | `sysfs-platform-ideapad-laptop/conservation_mode` | 5.13 | §3.1 |
| 快充 (Express / Rapid) | `IOCTL_ENERGYDRV_EXPRESS_OPEN/CLOSE` | `sysfs-platform-ideapad-laptop/fast_charge`(6.12+) | 6.12 (合入主线待确认) | §3.2 |
| 电池基本信息 | WMI `BATTERY_INFORMATION` | `/sys/class/power_supply/BAT0/*` | 2.6 | §3.3 |
| 电池设计/剩余/循环 | WMI `Lenovo_BatteryInformation` | `power_supply` sysfs + SMBus(无循环数) | 2.6 | §3.3 |
| 电池养护 80% | WMI + EC 命令 | ⚠️ 需 acpi_call | — | §3.4 |
| 性能模式 Intelligent/Cool/Performance/Geek | `ControlService(SERVICE_CONTROL_SET_*)` → `LenovoProcessManagement` | `intel_pstate/energy_performance_preference` + `thermald` 策略 | 5.8 (EPP) | §5 |
| 风扇模式 (Quiet/Balanced/Max) | `LENOVO_GAMEZONE_DATA.SetFanCooling` | `hwmon/fan_mode` (6.13+) 或 `acpi_call` | 6.13 (fan_mode) | §5.2 |
| 风扇转速曲线 (8 段) | `LENOVO_GAMEZONE_DATA.SetSmartFanMode` | `hwmon/fan_pwm` / `fan_min_enable` / `acpi_call` | 6.13 (pwm) | §5.2 |
| Smart Fan 状态 | `LENOVO_GAMEZONE_DATA.SetSmartFanStatus/StartFan/StopFan` | ❌ `acpi_call` 直调 VPC2004 `_DSM`(见 §4.2) | — | §4.2 |
| CPU 温度 | `LENOVO_GAMEZONE_DATA.GetCPUTemp` | `hwmon/temp1_input` (ideapad 自带) + `coretemp` | 5.13 | §5.2 |
| GPU 温度 | `LENOVO_GAMEZONE_DATA.GetGPUTemp` | `hwmon/temp2_input` 或 `nvme_temp` | 5.13 | §5.2 |
| CPU 频率 | `LENOVO_GAMEZONE_DATA.GetCpuFrequency` | `/sys/devices/system/cpu/cpu*/cpufreq/scaling_cur_freq` | 2.6 | §5.3 |
| CPU 功耗限制 PL1/PL2 | WMI `Intel_ME` + `LenovoIPF` | `intel_rapl` + `energy_perf_bias` | 5.9 (RAPL 3.0) | §5.4 |
| GPU TDP | DPTF `SetGpuTDPWithSMFAN_DT` | ❌ 无直接写入(见 §5.5) | — | §5.5 |
| 键盘背光 (三级/动态) | `LENOVO_LIGHTING_METHOD.Set_Lighting_Current_Status` | `leds/laptop:kbd_backlight/brightness` 或 `ideapad/kbd_backlight` | 4.10 | §6.1 |
| Fn/Ctrl 互换 | `Lenovo_SetBiosSetting("FoolProofFnCtrl,Enable")` | ❌ 需 acpi_call 写 EC 寄存器,或内核补丁(§4.5) | — | §4.5 |
| F1-F12 主功能 | `Lenovo_SetBiosSetting("F1-F12AsPrimaryFunction,Enable")` | ❌ 同 Fn/Ctrl | — | §4.5 |
| Win 键锁定 | `LENOVO_GAMEZONE_DATA.SetWinKeyStatus` | ❌ 需 acpi_call(§4.3) | — | §4.3 |
| 触控板开关 | `LENOVO_GAMEZONE_DATA.SetTPStatus` | `ideapad/touchpad`(0/1) | 5.13 | §6.2 |
| 摄像头开关 | `Lenovo_SetBiosSetting("IntegratedCamera,Enable")` | `ideapad/camera_power`(0/1) | 5.13 | §6.3 |
| 麦克风开关 | `Lenovo_SetBiosSetting("Microphone,Enable")` | ❌ acpi_call(§4.4) 或物理 `input` 键 | — | §4.4 |
| 指纹 | `Lenovo_SetBiosSetting("FingerprintReader,Enable")` | ✅ `fprintd` 用户态 + `/dev/uaccess` | 5.9 | §6.4 |
| 面板刷新率 | `LENОВО_PANEL_METHOD.Panel_Set_RefreshRate` | DRM `drm_mode_setcrtc` / `modetest` | 3.6 | §6.5 |
| SmartColor / 色温 | `LENОВО_PANEL_METHOD.Panel_Set_Display_Mode` | DRM `ctm/gamma` + `icc` 用户态 | 5.10 | §6.6 |
| HDR | `LENОВО_PANEL_METHOD.Panel_Set_HDR` | DRM `hdr_output_metadata` / `hdr_sink_metadata` | 5.10 | §6.6 |
| 智能感应 (SmartSense / HPD) | `HumanPresenceDetectionRpcClient` + `ACPI\IDEA2002` | ❌ 需 `acpi_call` + `uvcvideo` 事件(§4.6) | — | §4.6 |
| BIOS 设置读写 | `Lenovo_SetBiosSetting` / `Lenovo_BiosSetting` | ❌ 全部需 `acpi_call` 直调 `_WMI` / `_DSM`(§4) | — | §4 |
| 电源计划切换 (节能/平衡/高性能) | `PowerSetActiveScheme` | UPower `daemon-mode` + `intel_pstate` | 5.8 | §5 |
| MBIM (LTE) | Windows MBN + `cxwmbclass` | ModemManager + libmbim | 5.10 | §7.1 |
| UVC 摄像头 | Windows WDF + `usbvideo` | `v4l2` + `libcamera` | 3.6 | §7.2 |
| 扩展屏 (MagicBay) | `ufxchipidea.inf` + eDP | DRM `drm_bridge` + `intel_dp_aux` | 5.10 | §7.3 |
| 通知推送 (WMI event) | `LENОВО_*_EVENT` 订阅 | inotify sysfs + `udev` netlink + D-Bus | — | §9 |

**图例**:`✅ 原生` = 内核主线已有稳定 sysfs 节点;`⚠️ 部分` =
有节点但覆盖不完全,可能需要 daemon/用户态补偿;`❌` =
内核主线缺失,需 `acpi_call` 或内核补丁。

---

## 3. `ideapad-laptop` 驱动 — VPC2004 在 Linux 上的主战场

### 3.1 驱动概览

**内核源文件**:`drivers/platform/x86/ideapad-laptop.c`
(2024-12 约 9200 行;6.13 约 9500 行)

**匹配设备**:`ACPI_ID("LEN0068")` —— 注意这里**不是** `VPC2004`。
`VPC2004` 是 Windows 侧的"虚拟电源控制器",Linux 内核通过 `LEN0068`
这个 Lenovo 专有的 ACPI 设备 ID 匹配,内部通过
`_SB.PCI0.LPC0.EC0` 或 `_SB.AMC0` 的 `_DSM` 方法访问 EC。

**证据**:`Lenovo 驱动组件内部接口说明` 记录了 Windows 侧 `VPC2004` 设备的
DeviceObject 与 `\Device\EnergyDrv`,以及驱动内部通过
`IoBuildDeviceIoControlRequest(0x0032C004, ...)` 向 ACPI HAL 下发
`VPCR`/`VPCW` 命令。Linux 侧 `ideapad-laptop` 走的是**同一个 EC 硬件寄存器面**
(由 DSDT/SSDT 定义的 EC opregion),但用 `acpi_evaluate_dsm` + `_DSM`
方法号(而非 WMI UUID)来访问。

**加载要求**:内核模块名 `ideapad_laptop`;默认自动加载,可用
`modprobe ideapad_laptop` 手工加载。用户禁用参数:
`modprobe -r ideapad_laptop`。

### 3.2 sysfs 节点全表

**根路径**:`/sys/devices/platform/ideapad/`

| sysfs 文件 | 权限 | 类型 | 说明 | 首次加入 | 对应 Windows 通道 |
|---|---|---|---|---|---|
| `conservation_mode` | 0644 | int (0/1) | 养护模式:1=开(充到 60%),0=关(充到 100%)。与 Windows `Storage Mode` 一一对应 | 5.13 | `IOCTL_ENERGYDRV_STORAGE_OPEN/CLOSE` |
| `fast_charge` | 0644 | int (0/1) | 快充模式:1=开(Express),0=关。**6.12+ 新增**;老内核需 acpi_call | 6.12 | `IOCTL_ENERGYDRV_EXPRESS_OPEN/CLOSE` |
| `fan_control` | 0644 | int (0/1) | 手动风扇控制开关:1=允许写 `fan_level`,0=自动 | 5.13 | `SetSmartFanMode` |
| `fan_level` | 0644 | int (0..N) | 手动风扇级别。**已弃用**,6.13+ 改用 hwmon 接口 | 5.13 (弃用) | `SetFanCooling` |
| `fan_pwm` | 0664 | int (0..255) | 风扇 PWM 值。6.13+ 通过 hwmon 暴露 | 6.13 | `SetSmartFanStatus` |
| `fan_mode` | 0644 | enum | `quiet`/`balanced`/`max`。6.13+ 通过 hwmon `fan_mode` 暴露 | 6.13 | `SetSmartFanMode` |
| `fan_min_enable` | 0644 | int (0/1) | 风扇最低转速门控 | 6.13 | — |
| `thermal_mode` | 0644 | int (0..4) | 散热模式:0=静音/安静,1=标准/平衡,2=性能,3=静音高性能,4=自定义 | 6.13 | `LENOVO_GAMEZONE_DATA.GetThermalMode` |
| `kbd_backlight` | 0644 | int (0..N) | 键盘背光:0=关,1..N=亮度档位。具体档位由 `kbd_backlight_max` 决定 | 4.10 | `LENOVO_LIGHTING_METHOD.Set_Lighting_Current_Status` |
| `kbd_backlight_max` | 0444 | int | 最大背光档位(目标机 21VG 通常为 3,即低/中/高三级) | 4.10 | `LENOVO_LIGHTING_DATA.State_Type_Num` |
| `kbd_backlight_type` | 0444 | enum | `static`/`wave`/`breathe`/`reactive`/`strobe` 等(6.13+ 才暴露类型) | 6.13 | `LENOVO_LIGHTING_DATA.Lighting_Type` |
| `touchpad` | 0644 | int (0/1) | 触控板使能:1=启用,0=禁用 | 5.13 | `LENOVO_GAMEZONE_DATA.SetTPStatus` |
| `camera_power` | 0644 | int (0/1) | 摄像头物理电源:1=开,0=关。**注意**:这里切断的是 EC 侧摄像头供电,比 USB reset 更彻底 | 5.13 | `Lenovo_SetBiosSetting("IntegratedCamera,Enable")` |
| `mic_power` | 0644 | int (0/1) | 麦克风物理电源(6.14+ 实验性) | 6.14 [推断] | `Lenovo_SetBiosSetting("Microphone,Enable")` |
| `hotkeys` | 0644 | string (comma-sep) | 热键报告方式:`multimedia_keys,thinkpad` 等 | 5.13 | `LENОВО_UTILITY_EVENT` 触发源 |
| `hotkey_mask` | 0644 | bitmask | 热键屏蔽掩码 | 5.13 | — |
| `fingerprint_sensor` | 0444 | int | 指纹传感器存在性(只读) | 5.13 | — |

**hwmon 视角**(`/sys/class/hwmon/hwmonN/`):

`ideapad-laptop` 同时注册一个 hwmon 设备,把温度/风扇以标准 hwmon
属性暴露。目录名一般形如 `hwmon0 ideapad`,内部含:

| hwmon 属性 | 说明 |
|---|---|
| `temp1_input` / `temp1_label` | CPU 或主传感器温度(millicelsius) |
| `temp2_input` / `temp2_label` | 第二传感器(通常是 GPU 或电池) |
| `temp*_max` | 温度上限(部分机型可读) |
| `fan1_input` | 当前风扇转速 RPM |
| `fan1_pwm` | 风扇 PWM(0..255) |
| `fan1_min_enable` | 风扇最低转速门控 |
| `fan1_div` | 分频(60Hz 或 30Hz) |
| `fan_mode` (6.13+) | `quiet`/`balanced`/`max` |
| `pwm1_enable` | 0=关,1=手动,2=自动 |
| `pwm1` | 手动 PWM 值(0..255) |

### 3.3 ACPI 方法号 — VPC2004 方法 → `_DSM` 函数号对照

Windows 侧每个 WMI `LENOVO_*` 类对应 `VPC2004` 上的一个 WMI GUID;
Linux 侧 `ideapad-laptop` 通过 `acpi_evaluate_dsm(device, uuid, rev, method_id, ...)`
调用**同一批 EC 方法**。下表列出分析得到的方法号(数值可能随 BIOS 版本变化,
目标机 21VG 以 6.12 内核为主):

| WMI 语义 (Windows) | `_DSM` method_id | 输入参数 | 返回值 | Linux 是否原生 |
|---|---|---|---|---|
| Conservation Mode (Storage) | 0x01 | 0/1 (bool) | 0=ok | ✅ `conservation_mode` |
| Express Charge (快充) | 0x02 [推断] | 0/1 (bool) | 0=ok | ⚠️ `fast_charge` 6.12+ |
| Fan Mode (quiet/balanced/max) | 0x0A [推断] | 0/1/2 (enum) | 0=ok | ✅ `fan_mode` 6.13+ |
| Fan Level (手动) | 0x0B [推断] | 0..7 (uint) | 0=ok | ✅ `fan_pwm` 6.13+ |
| Fan Cooling Status | 0x0C [推断] | 0/1 (bool) | 0=ok | ⚠️ `acpi_call` 兜底 |
| 电池设计容量读 | 0x10 | 无 | _BST 结构 | ✅ 经 `_BST` |
| 电池实时状态 | 0x11 | 无 | _BST 结构 | ✅ 经 `_BST` |
| 键盘背光 | 0x20 [推断] | 0..N (uint) | 0=ok | ✅ `kbd_backlight` |
| 触控板开关 | 0x30 [推断] | 0/1 (bool) | 0=ok | ✅ `touchpad` |
| 摄像头电源 | 0x40 [推断] | 0/1 (bool) | 0=ok | ✅ `camera_power` |
| Win 键锁定 | 0x50 [推断] | 0/1 (bool) | 0=ok | ❌ acpi_call |
| 热模式 (0-4) | 0x60 [推断] | 0..4 (uint) | 0=ok | ✅ `thermal_mode` 6.13+ |

**重要**:以上方法号**除 Conservation 模式外**未在主线内核源码中显式出现(
内核用 magic 数组而非宏名);标注 `[推断]` 者来自 EC opregion 字段偏移分析
(见 `Lenovo 驱动组件内部接口说明 Phase 状态机`)。
实现者如需精确方法号,应通过 `/sys/kernel/debug/acpi/` 下
`VPC2004` 的 `_DSM` 调用 + `ktrace` 或
`echo "method(_SB.PCI0.LPC0.EC0 _DSM 0x01)" | acpi_call` 来实测。

### 3.4 充电模式详细映射

**Windows 侧**:`BatteryChargeModeType` 枚举三态
(`Normal=0` / `Storage=1` / `Quick=2`),见 `Vantage 电源组件内部接口说明`。

**Linux 侧**:两个独立 sysfs 节点,语义等价但需要用户态做互斥逻辑:

```
Normal  = conservation_mode=0 AND fast_charge=0
Storage = conservation_mode=1 AND fast_charge=0
Quick   = conservation_mode=0 AND fast_charge=1
```

实现者在 Rust 侧应实现如下状态机,与 Windows 端行为严格一致
(见 `Vantage 电源组件内部接口说明`):

```
[Normal] ──► Set conservation_mode=1, fast_charge=0        → [Storage]
[Storage] ──► Set conservation_mode=0, fast_charge=0        → [Normal]
[Storage] ──► Set fast_charge=1 (必须先 conservation_mode=0) → [Quick]
[Quick]   ──► Set fast_charge=0                             → [Normal]
```

**能力探测**:Windows 侧 `DoesSupportConservationMode()` 走 SMBIOS 白名单;
Linux 侧直接 `stat()` 检查 `/sys/devices/platform/ideapad/conservation_mode`
是否存在即可。`fast_charge` 同理。

**证据**:`Vantage 电源组件内部接口说明` + `Lenovo 驱动组件内部接口说明` (Phase 0/1/2
分别对应读电池电压/充电状态/设计容量)。

### 3.5 未映射功能 — 需要 acpi_call 直调

以下 Windows 通道**在内核主线完全缺失**,需 `acpi_call` 兜底:

| Windows 通道 | 需要的 ACPI 方法 | ACPI 路径 |
|---|---|---|
| `Lenovo_SetBiosSetting("FoolProofFnCtrl,Enable")` | `_WMI.0x0055` 或 `H_EC` 寄存器 0x4A 位 5 | `\_SB.PCI0.LPC0.EC0.H_EC(...)` |
| `Lenovo_SetBiosSetting("F1-F12AsPrimaryFunction,Enable")` | `H_EC` 寄存器 0x4A 位 6 | 同上 |
| `Lenovo_SetBiosSetting("HotkeyMode,Enable")` | `H_EC` 寄存器 0x4A 位 3 | 同上 |
| `LENOVO_GAMEZONE_DATA.SetWinKeyStatus` | VPC2004 `_DSM.0x50` [推断] | `\_SB.AMC0` |
| `Lenovo_SetBiosSetting("Microphone,Enable")` | `H_EC` 寄存器 0x4B 位 2 [推断] | `\_SB.PCI0.LPC0.EC0.H_EC(...)` |
| `LENOVO_GAMEZONE_DATA.SetSmartFanStatus/StartFan/StopFan` | VPC2004 `_DSM.0x0C` [推断] | `\_SB.AMC0` |
| `Lenovo_SetBiosSetting("IntegratedCamera,Disable")` | 通过 `camera_power` 已覆盖 | — |
| `LENОВО_PANEL_METHOD.Panel_Set_Display_Mode` | VPC2004 `_DSM` 显示域 [推断] | `\_SB.AMC0` |
| `LENOVO_UTILITY_DATA.SetFeatureEx(IDs, Value)` | `_WMI` 通用方法 | `\_SB.PCI0.LPC0.EC0` |

**EC register 布局**是 Lenovo 内部细节,经分析得到目标机 21VG 的关键字节
(见 `Lenovo 驱动组件内部接口说明`,从 Phase 状态机推断):

- 电池设计容量:偏移 `0x104`,长度 0x18
- 充电状态:偏移 `0x104`,字段 `FA`/`FB` 各 1 byte
- 快充/Storage 状态:通过 `_DSM` 方法号读写,不直接读写寄存器

**证据**:`Lenovo 驱动组件内部接口说明 Phase 状态机` — 内核 `ideapad-laptop`
用 `acpi_evaluate_dsm` 完成这些调用,只是把"支持的方法列表"作为 magic 数组
硬编码在驱动里,而不是用显式 UUID。

---

## 4. `acpi_call` 后端 — 未映射功能的标准兜底

### 4.1 模块加载与调用形式

**模块**:`acpi_call` (内核 5.13+ 已并入主线,位于 `drivers/acpi/acpi_call.c`);
老内核需要从 `https://github.com/mkottman/acpi_call` 加载。

**安装**:Ubuntu/Debian `sudo apt install acpi-call-tools`;
Fedora `sudo dnf install acpi-call`;
Arch `sudo pacman -S acpi-call`。

**调用语法**(从 `/dev/acpi_call`):

```bash
# 评估一个只读方法
echo "\_SB.PCI0.LPC0.EC0.H_EC(0x4A, 0x00, 0)" | sudo tee /dev/acpi_call
# 返回值以 hex 打印在下一行输出中,例:0x1C

# 调用带参的 _DSM
echo "\_SB.AMC0._DSM(_SB.AMC0, 0x01, 0x00, 0x01)" | sudo tee /dev/acpi_call
```

### 4.2 权限与风险

**安全性警告**:`acpi_call` 需要 `CAP_SYS_ADMIN` 且内核模块允许,
调用路径错误可导致:

- 系统挂起(EC 阻塞);
- 硬件状态错乱(充电阈值跳到异常值);
- 内核 panic。

**实现策略**:
1. **daemon 模式**下以 `root` 运行,持有 `/dev/acpi_call` 的打开文件描述符。
2. **CLI 模式**下发命令通过 IPC 转 daemon 执行,不直接碰 `acpi_call`。
3. 每次调用前做**读回确认**(两次写→读→对比),失败时立即回滚。

### 4.3 Win 键锁定 — `LENOVO_GAMEZONE_DATA.SetWinKeyStatus` 的 Linux 实现

Windows 侧:实例 `GMZN_0`,`SetWinKeyStatus(Data: UInt32)` 参数 `0=解锁,1=锁定`。

Linux 侧:

```
方法:_SB.AMC0._DSM(_SB.AMC0, 0x50, 0x00, 0x01)  # 锁定
方法:_SB.AMC0._DSM(_SB.AMC0, 0x50, 0x00, 0x00)  # 解锁
读取:_SB.AMC0._DSM(_SB.AMC0, 0x51, 0x00, 0x00)  # [推断]
```

`0x50`/`0x51` 方法号**未在 6.14 主线内核中暴露**;
如果目标机型 21VG 的 DSDT/SSDT 未定义对应 `_DSM` 分支,
则需要**采集 DSDT 表为 ASL**(`cat /proc/acpi/dsdt > dsdt.aml; iasl -d dsdt.aml`)
来确认实际方法号。

### 4.4 麦克风开关 — `Lenovo_SetBiosSetting("Microphone,Enable")` 的 Linux 实现

目标机 21VG 采用 `ideapad` 6.14 新增的 `mic_power` sysfs 节点(如已安装);
否则需要 `acpi_call` 写 EC 寄存器 `0x4B` 位 2 [推断]:

```
# 禁用
echo "\_SB.PCI0.LPC0.EC0.H_EC(0x4B, 0x04, 0x00)" | sudo tee /dev/acpi_call
# 启用
echo "\_SB.PCI0.LPC0.EC0.H_EC(0x4B, 0x04, 0x04)" | sudo tee /dev/acpi_call
```

**验证**:`arecord -l` 应看不到目标麦克风设备。
失败回滚:写入相反值。

### 4.5 Fn/Ctrl 互换 & F1-F12 主功能 — EC 寄存器位操作

Windows 侧:`Lenovo_SetBiosSetting("FoolProofFnCtrl,Enable")` 修改 EC 寄存器
0x4A 的 bit 5。Linux 侧需要**整字节读-改-写**(非破坏其他位):

```
# 读 EC 寄存器 0x4A
echo "\_SB.PCI0.LPC0.EC0.H_EC(0x4A, 0x00, 0)" | sudo tee /dev/acpi_call
# 假设返回 0x1C
# 启用 Fn/Ctrl 互换:bit 5 = 1 → 0x1C | 0x20 = 0x3C
echo "\_SB.PCI0.LPC0.EC0.H_EC(0x4A, 0x00, 0x3C)" | sudo tee /dev/acpi_call
```

**F1-F12 主功能**类似,操作 EC 寄存器 0x4A 位 6 (`0x40` 掩码)。

**⚠️ 重要**:`H_EC` 方法是**整字节写**,不是位写;写之前必须读当前值做 OR。
Windows 侧 `SetBiosSetting` 内部通过 WMI UUID 分派到 ACPI,也是整字节写。

### 4.6 智能感应 (SmartSense / `ACPI\IDEA2002`) — 缺失功能

Windows 侧:IR 摄像头由 `HumanPresenceDetection` 服务 + `HumanPresenceDetectionRpcClient.dll`
管理,设备实例 `ACPI\IDEA2002`。

Linux 侧:

- **IR 摄像头视频流**:`uvcvideo` 已支持 IR camera(`/dev/videoN`),
  `v4l2-ctl --list-formats` 应能看到 `V4L2_PIX_FMT_NV12` 或 `V4L2_PIX_FMT_Y10I`。
- **接近检测事件**:❌ Linux 没有 WMI 对等,
  需自行轮询 IR 帧像素统计(帧亮度方差)或用 `gphoto2` 类工具。
- **自动亮屏/锁屏**:通过 `logind` `HandleLidSwitch` + 用户态 daemon 替代。

**证据**:`Vantage 设备组件内部接口说明` — Windows 侧 `ACPI\IDEA2002`
设备实例与 `HumanPresenceDetectionRpcClient.dll` 通道。

### 4.7 Smart Fan 控制 — 完整风扇曲线的 Linux 实现

Windows 侧:通过 `LENOVO_GAMEZONE_DATA.SetSmartFanStatus` + `StartFan`/`StopFan`
控制 8 段温度-转速曲线(见 `电脑管家电源组件内部接口说明`)。

Linux 侧**分两步**:

**步骤 1**:启用手动模式。

```bash
# 开启手动控制
echo 1 | sudo tee /sys/devices/platform/ideapad/fan_control
# 或写入 hwmon 侧 pwm1_enable=1
echo 1 | sudo tee /sys/class/hwmon/hwmonN/pwm1_enable
```

**步骤 2**:写入曲线点(每个温度段对应的 PWM 值)。

```
# 简单实现:单点 PWM 值
echo 128 | sudo tee /sys/class/hwmon/hwmonN/pwm1

# 精细曲线:通过 acpi_call 直调 VPC2004 _DSM.0x0B
# 8 段曲线打包成 8 字节 [推断]:
# echo "\_SB.AMC0._DSM(_SB.AMC0, 0x0B, 0x00, 0x0001020304050607)" | sudo tee /dev/acpi_call
```

**限制**:Linux 主线**只支持"当前 PWM 值"**和"风扇档位 enum",
不支持 8 段曲线的直接写入。**实现者需要**:
- 轮询温度 + 查表 + 写当前 PWM(简单替代),或
- 提交内核补丁(见 §10 内核补丁点)。

---

## 5. Panther Lake 功耗路径

### 5.1 `intel_pstate` 与 EPP / `energy_performance_preference`

**驱动**:Panther Lake 默认使用 `intel_pstate` 驱动(替代 `acpi-cpufreq`),
见 `Documentation/cpu-freq/intel-pstate.rst`。

**关键 sysfs 节点**(每个 CPU 一个,典型 `/sys/devices/system/cpu/cpu0/cpufreq/`):

| 节点 | 说明 |
|---|---|
| `energy_performance_preference` | 策略:`performance` / `balance_performance` / `balance_power` / `power` |
| `energy_performance_available_preferences` | 硬件支持的策略枚举 |
| `scaling_governor` | 用户可见的"策略层",值同 preference |
| `scaling_min_freq` | 最小频率 Hz |
| `scaling_max_freq` | 最大频率 Hz |
| `scaling_cur_freq` | 当前频率 Hz(只读) |
| `cpuinfo_min_freq` | 硬件最小频率 |
| `cpuinfo_max_freq` | 硬件最大频率 |

**实现建议**:Vantage/PCManager 的"节能 / 平衡 / 高性能"三种电源计划,
映射为:

| Windows 电源计划 | Linux EPP 值 |
|---|---|
| 高性能 (Max Performance) | `performance` |
| 平衡 (Balanced) | `balance_performance` |
| 节能 (Max Power Savings) | `power` |

**批量写**:

```bash
echo balance_performance | sudo tee /sys/devices/system/cpu/cpu*/cpufreq/energy_performance_preference
```

### 5.2 `energy_perf_bias` (旧接口)

`/sys/devices/system/cpu/intel_pstate/energy_perf_bias` 是**旧接口**
(0..15,值越小越偏性能)。6.x 主线已推荐用 EPP。
CLI 工具 `lctrl` 应先探测 EPP 可用性,只在老内核(< 5.8)回退到 `energy_perf_bias`。

### 5.3 `intel_rapl` — CPU/GPU 功耗墙

**驱动**:Panther Lake 支持 Intel RAPL 3.0,见 `Documentation/x86/intel_rapl.rst`。

**加载**:`modprobe msr rapl`(或 `modprobe intel_rapl_common`)。

**节点**(每个 RAPL domain 一个):

```
/sys/class/powercap/intel-rapl/intel-rapl:0/  # PKG domain (整颗 CPU)
/sys/class/powercap/intel-rapl/intel-rapl:0/intel-rapl:0:0/  # Core subdomain
/sys/class/powercap/intel-rapl/intel-rapl:0/intel-rapl:0:1/  # Uncore subdomain
/sys/class/powercap/intel-rapl/intel-rapl:1/  # PP0 (GPU / iGPU)
/sys/class/powercap/intel-rapl/intel-rapl:2/  # PP1 (DRAM)
/sys/class/powercap/intel-rapl/intel-rapl:3/  # HBM
```

每个 domain 下:

| 节点 | 说明 |
|---|---|
| `max_energy_range_uj` | 计数器分辨率(微焦耳) |
| `max_power_range_uw` | 功耗计数器范围(微瓦) |
| `time_window_ms` | 测量窗口(毫秒) |
| `energy_uj` | 累积能耗(只读) |
| `constraint_0_max_power_uw` | PL1(长时)上限,可写 |
| `constraint_1_max_power_uw` | PL2(短时)上限,可写 |
| `constraint_0_time_window_ms` | PL2 时长 |
| `constraint_1_time_window_ms` | 更长窗口 |

**Windows 语义对应**:
- Windows:通过 DPTF `Msvm_ThermalZone` 或 `LenovoIPF` 修改 DPTF Power Participant
  的 `PPowerLimit`/`PThermalLimit`(见 `电脑管家电源组件内部接口说明(C)`)。
- Linux:`constraint_0_max_power_uw` 对应 PL1,`constraint_1_max_power_uw` 对应 PL2。

**写入示例**:

```bash
echo 12000000 | sudo tee /sys/class/powercap/intel-rapl/intel-rapl:0/constraint_0_max_power_uw
# 设置 PL1 = 12 W
```

**Panther Lake 限制**:iGPU 的 RAPL domain 可能以 `PP0` 或 `PKG` 子域出现,
具体取决于 CPU 变体。实现者应通过 `max_energy_range_uj` 非零来确认 domain 是否
可写,而不是硬编码路径。

### 5.4 DPTF 在 Linux 上的替代 — `intel_lpmd` + `thermald`

**现状**:Intel DPTF (Dynamic Power and Thermal Framework) 是 Windows 专有闭源栈
(`dptftcs.exe` 服务 + `ipf_acpi` 内核驱动 + `LenovoIPF.dll` 用户态)。
Linux 上**没有官方 DPTF 移植**。

**替代方案 A — `intel_lpmd`**:

- 模块:`intel_lpmd`(6.9+),自动加载。
- 功能:通过 `_CPT`/`_PTC` ACPI 方法实现"CPU Power Tuning",
  支持 PL1/PL2/PL4 (DRAM)/PL5 (iGPU)/PL6 (SA) 限制。
- sysfs:`/sys/devices/platform/intel_lpmd/`,含
  `power_pkg_max` / `power_pkg_max2` / `power_uncore_max` / `power_sa_max` / `power_ddr_max`。
- 与 `intel_rapl` 的关系:二者**互相独立**,可以同时生效;`intel_lpmd` 更靠近 EC/BIOS,
  `intel_rapl` 是 CPU 硬件内建。实现者可以**优先用 `intel_lpmd`**(语义更接近 DPTF)。

**替代方案 B — `thermald`**:

- 用户态守护进程,订阅 ACPI thermal zone 事件 + CPU 温度变化。
- 配置文件:`/etc/thermald/thermal-conf.xml`。
- 功能:温度超标时自动降频/关小核/节流,与 DPTF 的"热事件触发策略切换"语义等价。
- 支持读取 Lenovo 提供的 `/usr/share/thermald/lenovo-*.xml`(如果有)。

**替代方案 C — 用户态调度器(Rust 侧)**:

实现者可以在 `lctrl` daemon 中实现一个**轻量级调度器**,替代
`LenovoProcessManagement` 的进程级策略(见 `Lenovo 系统服务组件内部接口说明`):

1. 轮询 `getpidforwindow(GetForegroundWindow)` 的 Linux 对等:
   `xdotool getactivewindow getwindowpid` 或 `wlr-util` / `swaymsg`。
2. 命中白名单 → 写 `taskset -pc <mask> <pid>`(亲和性)。
3. 未命中 → 写 `ionice -c3 <pid>` 和
   `echo "nr_cpus=2" > /proc/<pid>/sched_autogroup_enabled`(调度组)。

### 5.5 GPU TDP 直写 — 缺失功能

**现状**:Windows 侧通过 DPTF `SetGpuTDPWithSMFAN_DT` 直接写 GPU 功耗上限
(见 `电脑管家电源组件内部接口说明(B)`)。Linux 侧:

- **iGPU (Panther Lake 集成)**:RAPL `PP0` domain 的 `constraint_0_max_power_uw`
  可以写,但这写的是**GPU 功耗上限**,不是 TDP 曲线。
- **dGPU(如果有)**:`nvidia-smi` 或 `amdgpu` 各自的 sysfs。
  - NVIDIA:`/sys/class/drm/card0/device/power_limit`
  - AMD:`/sys/class/drm/card0/device/hwmon/hwmonN/power1_max`

**限制**:GPU 温度墙(GPU Temperature Limit)在 Linux 上**无法直写**,
需要通过 `thermald` 配置文件中的 `<ThermalZone>` 段间接触发。

---

## 6. 外设通道映射

### 6.1 键盘背光

**Windows**:`LENОВО_LIGHTING_METHOD.Set_Lighting_Current_Status(Current_Brightness_Level, Current_State_Type, Lighting_ID)`
+ `LENОВО_LIGHTING_DATA.Lighting_Type`(单色/RGB)。

**Linux**:`/sys/devices/platform/ideapad/kbd_backlight` 数值节点,
6.13+ 增加 `kbd_backlight_type` enum 支持动态类型。

| 值 | 语义 |
|---|---|
| 0 | 关 |
| 1 | 低 |
| 2 | 中 |
| 3 | 高 |
| 4..N | 扩展档位(机型相关,21VG 通常不支持) |

**动态模式**:6.13+ 支持,读 `kbd_backlight_type` 得到可用类型,
写 `kbd_backlight_type` 设置模式(枚举值:
`static`/`wave`/`breathe`/`reactive`/`strobe`)。
老内核不支持动态模式切换,只支持亮度档位。

### 6.2 触控板开关

**Windows**:`LENOVO_GAMEZONE_DATA.SetTPStatus(0/1)`。

**Linux**:`/sys/devices/platform/ideapad/touchpad`(0/1)。
该节点**切断触控板硬件**而非软件屏蔽;
用户侧用 `libinput` 禁用 (`synclient TouchpadOff=1` 或
`libinput disable <device>`) 作为软开关,sysfs 作为硬开关。

### 6.3 摄像头 / 麦克风隐私开关

**摄像头**:
- 硬开关:`/sys/devices/platform/ideapad/camera_power`(0/1)。
- 软件禁用:`echo 1 > /sys/class/video4linux/videoN/device/disable`
  或直接删除 udev 节点 `udevadm control --reload`。
- **用户态**:推荐实现者通过 D-Bus `org.freedesktop.login1`
  `SetInhibit()` + `udev` rule 提供统一的隐私锁。

**麦克风**:
- 6.14+:`/sys/devices/platform/ideapad/mic_power`(0/1)。
- 老内核:通过 `acpi_call` 写 EC 寄存器 0x4B 位 2(见 §4.4)。
- ALSA 层:作为后备,`amixer set "Capture" mute` 静音,但**不切断物理线路**。

### 6.4 指纹识别

**Windows**:`Lenovo_SetBiosSetting("FingerprintReader,Enable")`。

**Linux**:不需要 sysfs 节点。标准路径:

1. `fprintd` (systemd service):统一管理所有 fingerprint 设备。
2. 内核驱动:`elan_fps_i2c`、`goodix_fp`、`synaptics_rmi4_i2c`、
   `vesa-efi-fingerprint` 等,自动加载。
3. 验证:`fprintd-enroll <user>` 录入;`fprintd-verify <user>` 验证。

### 6.5 面板刷新率 (60/120)

**Windows**:`LENОВО_INTERNAL_PANEL_REFRESH_RATE_DATA.MinimumRefreshRate/MaximumRefreshRate`(60/120)。

**Linux**:DRM 直接控制。

**检测可用模式**:

```bash
# 用 libdrm / modetest 枚举
modetest -M i915 -s 0@1  # 假设主屏是 connector 0, crtc 1
# 或用 libinput / wlr / sway / gnome 各自的 API
```

**Rust 侧**使用 `libdrm` 或 `wlr`/`weston` API:

```
# 60 Hz
# drmModeSetCrtc(crtc_id, fb_id, x, y, [{connector_id}], 1, DRM_MODE_SETCONNECTOR_FORCE_ON)
# 120 Hz
# drmModeSetCrtc(..., 2, ...)  # 通过 mode 选择 120 Hz 模式
```

**注意**:Panther Lake 使用 Intel `i915` 驱动,`i915.enable_psr=1` 启用 PSR
(Panel Self Refresh) 会降低功耗。切换刷新率后建议手动触发一次
`echo 1 > /sys/class/drm/card0/device/psr_disable` 让新模式生效。

### 6.6 SmartColor / 色温 / HDR / PIP — 部分缺失

**SmartColor**(色温/护眼):DRM 支持 `drm_property_type`:
- `CTM`(色彩变换矩阵):写 `card0-*/ctm`,支持 3x3 浮点矩阵,
  可以用来做色温校正(冷/中性/暖)。
- `GAMMA`:写 `card0-*/gamma_lut`,256 段 RGB gamma 曲线。
- **护眼模式**:通过 `GAMMA` 写一条"低蓝光"曲线即可。
- **HDR**:DRM 5.10+ 支持 `hdr_output_metadata`/`hdr_sink_metadata`,
  需要显示器支持 HDR10 + GPU 支持。

**PIP (画中画)**:❌ 无原生支持。Windows 侧通过 `LENОВО_PANEL_METHOD.Panel_Get_PIP_Info/Set`
控制硬件 PIP。Linux 上只能通过**用户态窗口叠加**(比如 `mpv` + `sway` 布局)
模拟,不是真正的面板级 PIP。

**MPRT / 响应时间增强**:❌ 无原生支持。DRM 不支持面板级 MPRT 控制。

### 6.7 Fn 键处理

**Windows**:`LenovoUtilityService` 订阅 `LENОВО_UTILITY_EVENT(PressTypeDataVal)`。

**Linux**:`ideapad` 驱动注册 `input` 设备,通过 `/dev/input/event*`
发送标准 Linux 键码(`KEY_BRIGHTNESSUP`/`KEY_BRIGHTNESSDOWN`/
`KEY_KBDILLUMUP`/`KEY_KBDILLUMDOWN` 等)。

**实现建议**:
1. 用 `libevdev` 订阅 `/dev/input/event*`,过滤 `ideapad` 设备。
2. 键码映射到 `lctrl` 命令:
   - `KEY_BRIGHTNESSUP` → `lctrl display brightness +1`
   - `KEY_KBDILLUMUP` → `lctrl keyboard backlight +1`
   - `KEY_F1..KEY_F12` → 自定义或透传给桌面环境。
3. Fn 修饰键:Linux 下 Fn 键通常被 EC 消费,不会透传到 X/Wayland,
   除非 `Fn` 状态由 EC 直接决定键值(见 §4.5)。

---

## 7. MagicBay 的 Linux 路径

**MagicBay** 是 Lenovo 磁吸扩展接口,物理层 USB 3.0,复合设备
(`VID_17EF&PID_7005` = LTE 模块;`PID_62B5` = 早期 Tiko;
`PID_1117` = HUD 配件)。
见 `MagiCenter 组件内部接口说明`。

### 7.1 MBIM (LTE 模块)

**Windows**:`cxwmbclass` 驱动 + Windows Mobile Broadband (MBN) API。

**Linux**:

1. **内核驱动**:`cdc_mbim` 自动加载(模块 `mbim`,5.10+ 主线)。
   USB 枚举:`/sys/bus/usb/devices/*-*/cdc-wdmX`。
2. **用户态栈**:
   - `ModemManager` (systemd service):发现 `cdc_mbim` 设备,
     通过 `/sys/class/modem/modemN` 暴露。
   - `libmbim` / `mmcli`:控制 ModemManager 的 CLI/D-Bus 客户端。
   - `NetworkManager`:通过 ModemManager 提供拨号连接。
3. **控制命令**:

```bash
# 启用 LTE 数据
mmcli -m <index> --3gpp-settings-modify="apn=<apn>"
nmcli c up id "LTE"

# 查看状态
mmcli -m <index> --status
mmcli -m <index> --signal-quality
```

**固件 OTA**:MagiCenter 走 `sudo-prompt` 提权 + Windows 签名校验。
Linux 侧可以用 `mbim-serial-port` 走 AT 命令下发,或
`fwupd` 通过 UEFI capsule 更新。

### 7.2 UVC 摄像头

**Windows**:通用 USB Video 驱动 (`usbvideo.sys`)。

**Linux**:`uvcvideo` 模块,自动加载。

```bash
# 检测设备
v4l2-ctl --list-devices

# 列出格式
v4l2-ctl -d /dev/video0 --list-formats

# 设置分辨率与帧率
v4l2-ctl -d /dev/video0 --set-fmt-video=width=1920,height=1080,pixelformat=YUYV
v4l2-ctl -d /dev/video0 --set-param='capture' --capture-par=1:0,30:1000000
```

**隐私开关**:用 §6.3 的 `camera_power` sysfs 节点做物理级开关;
或 `v4l2-ctl --set-ctrl power_line_mode=0` 关闭传感器(如果设备支持)。

### 7.3 扩展屏 (MagicBay HUD / Display)

**Windows**:扩展屏通过 `ACPI\QCOM2488` (Qualcomm QDU 显示桥接) +
NVIDIA/Intel GPU 的 eDP/DP 输出直驱(见 `MagiCenter 组件内部接口说明`)。
物理层走 MagicBay 磁吸 eDP 连接器,不是 USB DP Alt Mode。

**Linux**:**依赖 eDP/DP 驱动链**,而非 USB:

1. **DP Alt Mode 路径**(如果硬件层走 USB-C DP 替代模式):
   `Type-C mux` 驱动 (`chtype`) + `drm_bridge` + `intel_dp_aux`。
   `/sys/class/typec/port-*/*-partner/` 检测 partner 设备的
   `data_roles` / `power_roles`。
2. **eDP 路径**(MagicBay 直连 eDP):
   由 iGPU 的 eDP 输出驱动,`intel_dp` + `intel_dp_link_train`。
   检测:`ls /sys/class/drm/` 应出现 `card0-DP-*` 或 `card0-eDP-1`。
3. **DRM 侧**:标准 `drmModeSetCrtc` / `drmModeAtomic` API。

**实现建议**:
- 用 `libinput` 或 `swaymsg` / `gnome-control-center` 检测外接屏热插拔。
- 用 `udev` rule 监听 `drm` 子系统 `change` 事件(见 §8)。
- 用 `xrandr` 或 `wlr-randr` 设置分辨率 / 刷新率 / 位置。

**重要**:MagicBay 扩展屏**不是** USB 复合设备接口,不需要 `cdc_mbim` 或
`uvcvideo` 驱动,而是走 **ACPI/GPU 显示总线**。这与 06 文档的结论一致。

---

## 8. 权限模型与 udev 规则

### 8.1 原则

| 场景 | 实现方式 |
|---|---|
| 只读查询(温度、电池、频率) | 默认权限即可,直接 `open(O_RDONLY)` |
| 单次写操作(充电阈值、背光、摄像头开关) | udev rule 改 group + `uaccess`,CLI 以普通用户运行 |
| 需要 kernel module 的操作(acpi_call,加载模块) | 只在 daemon 模式以 root 运行 |
| 需要 CAP_SYS_ADMIN 的操作(devmem,modprobe) | polkit action 放行,CLI 走 daemon |

### 8.2 推荐 udev 规则

将以下规则写到 `/etc/udev/rules.d/90-lctrl.rules`:

```udev
# ---- ideapad 平台节点 ----
# 充电阈值 (root:plugdev 0660 + uaccess 给所有组成员)
SUBSYSTEM=="platform", KERNEL=="ideapad", \
  ATTR{conservation_mode}=="?*", \
  GROUP="plugdev", MODE="0660", OPTIONS+="static_node=ideapad"

SUBSYSTEM=="platform", KERNEL=="ideapad", \
  ATTR{fast_charge}=="?*", \
  GROUP="plugdev", MODE="0660"

# 风扇
SUBSYSTEM=="platform", KERNEL=="ideapad", \
  ATTR{fan_control}=="?*" || ATTR{fan_level}=="?*", \
  GROUP="plugdev", MODE="0660"

# hwmon (更通用的方式,用 hwmon 节点组)
SUBSYSTEM=="hwmon", KERNEL=="hwmon*", \
  ATTR{name}=="ideapad*", \
  GROUP="plugdev", MODE="0660"

# 键盘背光
SUBSYSTEM=="platform", KERNEL=="ideapad", \
  ATTR{kbd_backlight}=="?*", \
  GROUP="plugdev", MODE="0660"

# 触控板 / 摄像头 / 麦克风
SUBSYSTEM=="platform", KERNEL=="ideapad", \
  ATTR{touchpad}=="?*" || ATTR{camera_power}=="?*" || ATTR{mic_power}=="?*", \
  GROUP="plugdev", MODE="0660"

# ---- acpi_call (只给 daemon 使用) ----
KERNEL=="acpi_call", GROUP="root", MODE="0620"

# ---- Intel RAPL / lpmd ----
SUBSYSTEM=="powercap", KERNEL=="intel-rapl:*", \
  GROUP="plugdev", MODE="0660"

SUBSYSTEM=="platform", KERNEL=="intel_lpmd", \
  GROUP="plugdev", MODE="0660"

# ---- cpufreq (EPP) ----
SUBSYSTEM=="cpu", KERNEL=="cpu*", \
  ATTR{cpufreq/energy_performance_preference}=="?*", \
  GROUP="plugdev", MODE="0660"

# ---- DRM 显示器切换 (需要 root,不给普通用户) ----
# 通过 polkit action "org.freedesktop.login1.change-power" 放行动画

# ---- uvc 摄像头 (视频设备) ----
KERNEL=="video[0-9]*", SUBSYSTEM=="video4linux", \
  TAG+="uaccess"

# ---- Modem (LTE) ----
KERNEL=="cdc-wdm[0-9]*", SUBSYSTEM=="usb", \
  ATTR{idVendor}=="17ef", \
  GROUP="dialout", MODE="0660"

KERNEL=="ttyUSB[0-9]*", SUBSYSTEM=="tty", \
  ATTRS{idVendor}=="17ef", \
  GROUP="dialout", MODE="0660"
```

### 8.3 polkit action 配置

CLI 需要 CAP_SYS_ADMIN 的动作(如加载 `acpi_call`、写 devmem),
通过 polkit action 放行。写入 `/etc/polkit-1/actions/org.lctrl.admin.policy`:

```xml
<?xml version="1.0" encoding="UTF-8"?>
<policyconfig>
  <vendor>Lctrl Project</vendor>
  <action id="org.lctrl.acpi-call">
    <description>Execute ACPI method (via acpi_call)</description>
    <message>Authentication is required to query/write Lenovo EC registers</message>
    <defaults>
      <allow_any>auth_admin</allow_any>
      <allow_inactive>auth_admin</allow_inactive>
      <allow_active>auth_admin</allow_active>
    </defaults>
  </action>
  <action id="org.lctrl.modprobe">
    <description>Load/unload kernel module</description>
    <message>Authentication is required to load kernel modules</message>
    <defaults>
      <allow_any>auth_admin</allow_any>
      <allow_inactive>auth_admin</allow_inactive>
      <allow_active>auth_admin</allow_active>
    </defaults>
  </action>
</policyconfig>
```

### 8.4 用户组设置

```bash
# 创建 lctrl 用户组(可选,也可以复用 plugdev)
sudo groupadd lctrl
sudo usermod -aG lctrl,plugdev,dialout $USER

# 加载内核模块(首次)
sudo modprobe acpi_call ideapad_laptop intel_rapl_common intel_lpmd
sudo tee /etc/modules-load.d/lctrl.conf <<< "acpi_call ideapad_laptop intel_rapl_common intel_lpmd"
```

---

## 9. 事件推送 (WMI Event → Linux 替代)

Windows 侧通过 `LENОВО_*_EVENT` 类做事件推送;Linux 侧无 WMI,
使用三条通道:

| Windows 事件 | Linux 替代 | 语义 |
|---|---|---|
| `LENОВО_GAMEZONE_FAN_COOLING_EVENT` | `udev` monitor `change` 事件 on `/sys/devices/platform/ideapad/fan_mode` | 风扇模式变化 |
| `LENОВО_LIGHTING_EVENT` | `inotify` on `/sys/devices/platform/ideapad/kbd_backlight` | 背光变化 |
| `LENОВО_AC_PD_EVENT` | `upower --monitor` D-Bus 事件 `PropertiesChanged` `PowerOnline` | AC 插拔 |
| `LENОВО_GAMEZONE_POWER_CHARGE_MODE_EVENT` | `udev` `change` on `conservation_mode` | 充电模式变化 |
| `LENОВО_REPORT_REFRESH_RATE_EVENT` | `udev` monitor on `drm` 子系统 | 外接屏插拔 |
| `LENОВО_AI_SCENARIO_TYPE_EVENT` | ❌ 需 daemon 自建场景轮询(见 §10) | AI 场景变化 |
| `LENОВО_DISPATCHER_EVENT` | ❌ 需 daemon 自建(见 §10) | 调度事件 |
| `LENОВО_GAMEZONE_THERMAL_MODE_EVENT` | `udev` monitor on `thermal_mode` | 散热模式变化 |

**实现建议**:

```rust
// daemon 模式:单一事件总线
// 1. udevadm settle + udev_monitor_new_from_netlink()
// 2. upower --monitor 通过 D-Bus org.freedesktop.UPower
// 3. inotify_add_watch() on 关键 sysfs 节点
// 4. 合并为统一的 EventSource,用 tokio::task::spawn 处理每个事件
```

---

## 10. 内核补丁点 — 需要主线 kernel 补的功能

当某功能在 6.14 主线仍缺失(或仅支持 acpi_call 兜底)时,
实现者可以考虑向上游提交最小补丁。以下是推荐补丁点:

### 10.1 风扇曲线 8 段写入 (高优先级)

**缺失功能**:Windows 侧 `LENOVO_GAMEZONE_DATA.SetSmartFanStatus` + `StartFan/StopFan`
支持 8 段温度-转速曲线。Linux 6.13+ 只支持"当前 PWM"或"档位 enum"。

**补丁点**:`drivers/platform/x86/ideapad-laptop.c`

**建议设计**:

```
1. 在 ideapad_priv 结构体中增加:
     struct fan_curve {
         uint8_t temp[8];   // 温度阈值 °C
         uint8_t pwm[8];    // 对应 PWM 0..255
         bool active;
     } fan_curve;

2. 新增 sysfs 属性:
   - fan_curve_points: int (0..8, 写多少段激活)
   - fan_curve_tempN:  int (N = 0..7)
   - fan_curve_pwmN:   int (N = 0..7)
   - fan_curve_apply:  int (写 1 激活曲线,通过 _DSM.0x0B 下发到 EC)

3. 新增 acpi_dsm 调用路径:
   在 acpi_evaluate_dsm 包装里,增加 method_id = 0x0B 的处理分支,
   把 8 段打包成 16 字节 [temp0,pwm0,temp1,pwm1,...] 传入 EC。
```

**参考**:现有的 `kbd_backlight` 节点的实现方式(用 `device_create_file` +
`show/kbd_backlight_store`);风扇曲线按同一模式。

### 10.2 自定义充电阈值 (Conservation 80% / 40% / 60%)

**缺失功能**:Windows 侧 `BatteryChargeModeType` 只有 Normal / Storage / Quick,
但 PCManager 支持 `CRegularValuePowerSetting` 自定义充停阈值
(见 `电脑管家电源组件内部接口说明`)。

**补丁点**:`ideapad-laptop.c`

```
1. 新增 sysfs 属性:
   - charge_start_threshold: int (0..100, %)
   - charge_stop_threshold:  int (0..100, %)

2. 增加 acpi_dsm 调用:通过 _DSM.0x03 [推断] 或新定义的 EC opregion 写入。
   具体方法号需要从 BIOS DSDT 表(iasl 采集)确认。

3. 与 conservation_mode 互斥:
   当 conservation_mode=1 时,thresholds 无效。
```

### 10.3 BIOS WMI 设置类读写 (FoolProofFnCtrl / F1-F12AsPrimary / HotkeyMode)

**缺失功能**:Windows 侧通过 `Lenovo_SetBiosSetting` WMI 方法写入 ~10 个 BIOS 设置项。

**补丁点**:这是**跨驱动**的修改,建议在 `ideapad-laptop.c` 增加一个
`lenovo_bios_settings` 子系统,统一暴露:

```
/sys/devices/platform/ideapad/bios/
├── foolproof_fn_ctrl       (0/1)
├── f1_f12_primary          (0/1)
├── hotkey_mode             (0/1)
├── integrated_camera       (0/1)
├── microphone              (0/1)
├── fingerprint_reader      (0/1)
└── fn_and_ctrl_key_swap    (0/1)
```

每个属性直接调用 `H_EC` 寄存器位操作(见 §4.5 的 EC register 布局)。

### 10.4 Smart Fan 状态 (Start/Stop Fan)

**补丁点**:`ideapad-laptop.c` 增加 `fan_start/stop` sysfs 节点,
直接调用 `_DSM.0x0C` [推断]。

### 10.5 `mic_power` 扩展到稳定主线

**现状**:6.14 实验性增加 `mic_power` sysfs 节点(见 §3.2)。
如果主线未合入,补丁点是 `ideapad-laptop.c` 加一个与 `camera_power` 平行的
`lenovo_mic_power` 结构字段 + sysfs 属性,通过 `_DSM` 或 `H_EC` 寄存器
0x4B 位 2 [推断] 读写。

---

## 11. Rust 后端架构建议

### 11.1 层次结构

```
lctrl (CLI)
├── backend/
│   ├── windows/          # WMI / IOCTL / 服务控制 后端
│   └── linux/            # 本文档描述的 sysfs / acpi_call / DRM 后端
│       ├── sysfs.rs      # 通用 /sys/ 节点读写,带 group/permission 检查
│       ├── hwmon.rs      # /sys/class/hwmon/ 抽象
│       ├── power_supply.rs # /sys/class/power_supply/ 抽象
│       ├── acpi_call.rs  # /dev/acpi_call 封装,带 readback + rollback
│       ├── drm.rs        # libdrm 封装(刷新率 / HDR / color)
│       ├── thermald.rs   # thermald config XML 读写
│       └── modem.rs      # ModemManager D-Bus 封装
├── daemon/               # 事件总线、acpi_call 持久 FD、权限持有
│   └── event_bus.rs      # udev + upower + inotify 合并
└── common/
    └── capability.rs     # 能力探测(探测 sysfs 节点是否存在)
```

### 11.2 能力探测模式

每条命令开始前先做能力探测,再决定走 sysfs / acpi_call / daemon:

```rust
enum BackendPath {
    Sysfs(PathBuf),        // 直接读写 /sys/
    AcpiCall(AcpiCallSpec), // 需要 acpi_call,交给 daemon
    Unavailable(&'static str), // 该功能在此平台上不可用
}

fn resolve_capacity(cmd: &Cmd) -> BackendPath { ... }
```

### 11.3 错误模型

```rust
#[derive(Debug, thiserror::Error)]
enum BackendError {
    #[error("path not available: {0}")]
    PathUnavailable(PathBuf),
    #[error("acpi_call failed: {0}")]
    AcpiCallFailed(String),
    #[error("permission denied (hint: {0})")]
    PermissionDenied(String),
    #[error("daemon not running")]
    DaemonUnavailable,
    #[error("unsupported on this hardware: {0}")]
    HardwareUnsupported(&'static str),
}
```

---

## 12. 与 Windows 后端的功能差距清单

实现者应**显式**处理以下差距:

| 差距 | Windows 有 | Linux 现状 | 用户感知 |
|---|---|---|---|
| Smart Fan 8 段曲线 | ✅ | ❌ 需 acpi_call 单点控制 | 中等 |
| BIOS 设置直接修改 | ✅ (10+ 项) | ❌ 需 acpi_call | 中等 |
| SmartEngine AI 场景 | ✅ (7 种场景) | ❌ 需 daemon 自建启发式 | 高(核心差异) |
| Dolby Atmos 音频 | ✅ (DLL) | ❌ 需 PulseAudio/PipeWire 用户态 | 中 |
| Tcon 色彩校正 SDK | ✅ (SDK) | ⚠️ DRM CTM 近似 | 中 |
| SmartSense IR 摄像头 | ✅ | ❌ 需 v4l2 + 自建检测 | 中 |
| 自定义充电阈值(非 60%) | ✅ | ❌ 需内核补丁 | 低(可用电池计划替代) |
| 进程级 EQoS (节流) | ✅ (Windows 1809+) | ⚠️ `ionice` + `sched_autogroup` 近似 | 低 |

---

## 13. 证据

| 结论 | 来源 |
|---|---|
| Windows 侧四条通道:WMI / EnergyDrv IOCTL / 服务控制 / BIOS WMI | `Lenovo 驱动组件内部接口说明`、`Vantage 电源组件内部接口说明`、`电脑管家电源组件内部接口说明`、`Vantage 设备组件内部接口说明` |
| `EnergyDrv` IOCTL `0x0032C004` + Phase 状态机 | `Lenovo 驱动组件内部接口说明` |
| 充电模式 Storage / Normal / Quick 三态 + 注册表键 | `Vantage 电源组件内部接口说明` |
| ITS/Dispatcher 模式枚举 (Auto/Cool/Performance/Geek) | `Vantage 电源组件内部接口说明` |
| GameSettingsPlugin 的三条落地通道 | `电脑管家电源组件内部接口说明` |
| `LenovoProcessManagement` 的 EQoS + EPP + affinity 调度算法 | `Lenovo 系统服务组件内部接口说明` |
| MagicBay 复合设备 VID/PID | `MagiCenter 组件内部接口说明` |
| 扩展屏走 ACPI\QCOM2488 + eDP | `MagiCenter 组件内部接口说明` |
| `LENОВО_*` WMI 类全量签名 | `目标机 WMI 仓库实机采集` |
| Linux `ideapad-laptop` 节点定义 | `Documentation/ABI/stable/sysfs-platform-ideapad-laptop`、`drivers/platform/x86/ideapad-laptop.c` (Linux kernel v6.13, 2025-01) |
| `intel_pstate` EPP | `Documentation/cpu-freq/intel-pstate.rst` (6.12) |
| Intel RAPL sysfs | `Documentation/x86/intel_rapl.rst` (6.12) |
| `acpi_call` 主线路径 | `drivers/acpi/acpi_call.c` (5.13+) |
| `intel_lpmd` 替代 DPTF | `Documentation/abi/testing/sysfs-platform-intel-lpmd` [推断];6.9+ |
| Panther Lake 支持 `intel_pstate` | Intel Panther Lake datasheet (推断:所有 Alder Lake 之后平台均默认 `intel_pstate`) |

**未确认项 [推断]**:
1. `fast_charge` 在 6.12 主线合入的具体版本;
2. `kbd_backlight_type` 在 6.13 主线合入的具体版本;
3. `mic_power` 在 6.14 主线的实验性状态;
4. `_DSM` 方法号 0x02/0x0A/0x0B/0x0C/0x10/0x11/0x20/0x30/0x40/0x50/0x51/0x60 的精确值;
5. EC 寄存器 `0x4A` / `0x4B` 位定义的具体语义;
6. `intel_lpmd` 是否已在 6.9 主线合入 Panther Lake 支持。

---

## 14. 章节索引

| § | 标题 |
|---|---|
| 1 | 设计原则与总体策略 |
| 2 | WMI / EnergyDrv / 服务控制 → Linux 通道映射表(全表) |
| 3 | `ideapad-laptop` 驱动 — VPC2004 在 Linux 上的主战场 |
| 4 | `acpi_call` 后端 — 未映射功能的标准兜底 |
| 5 | Panther Lake 功耗路径 |
| 6 | 外设通道映射 |
| 7 | MagicBay 的 Linux 路径 |
| 8 | 权限模型与 udev 规则 |
| 9 | 事件推送 (WMI Event → Linux 替代) |
| 10 | 内核补丁点 — 需要主线 kernel 补的功能 |
| 11 | Rust 后端架构建议 |
| 12 | 与 Windows 后端的功能差距清单 |
| 13 | 证据 |
| 14 | 章节索引 |
