# 03 · 散热 / 风扇 / 性能模式规范

> 本文档面向 **实现者**,规定 `lctrl perf` 命令子树在 Lenovo 21VG (ThinkBook 14+ 2026, Panther Lake) 上实现时的全部行为契约:性能模式状态机、风扇控制、温度读取、功耗墙 (PL1/PL2/DBDC) 与超频、智能调度、PCManager 省电策略、WMI 事件订阅。

依赖:
- 通道与设备节点详见 01 (`01-hal-interfaces.md`)。
- 电池/充电阈值详见 02 (`02-power-battery.md`)。
- Windows 内核调度/DPTF/IPF 深挖见 07 (`07-pantherlake-tuning.md`)。
- Linux 后端映射见 09 (`09-linux-backend.md`)。

---

## 1. 概述与命名

本文档覆盖三条独立子系统:

| 子系统 | 顶层命令 | 硬件通路 |
|---|---|---|
| 性能模式 (Fn+Q) | `lctrl perf mode` | WMI `LENOVO_GAMEZONE_DATA` + `LenovoProcessManagement` 服务 |
| 风扇 | `lctrl perf fan` | WMI `LENOVO_FAN_TABLE_DATA` / `LENOVO_FAN_METHOD` / `LENOVO_FAN_MAX_SPEED_DATA` / `LENOVO_FAN_TEST_DATA` |
| 温度 | `lctrl perf temp` | WMI `LENOVO_GAMEZONE_DATA` 方法族 + (可选) Linux `/sys/class/thermal/` |
| 功耗墙 | `lctrl perf pl1` / `lctrl perf pl2` / `lctrl perf dbdc` | ESIF IPC (ipfsvc) + Windows Power API |
| 超频 | `lctrl perf oc` | WMI `LENOVO_CPU_OVERCLOCKING_DATA` / `LENOVO_GPU_OVERCLOCKING_DATA` / `LENOVO_MEMORY_OC_DATA` |

**关键约束**:所有"下发型"命令在 Windows 上均要求以管理员权限执行,且 `AcpiVpc.sys` (EnergyVpc) 与 `ipf_acpi` 驱动必须已加载;Linux 后端仅通过 `ideapad_laptop` / sysfs / ACPI 暴露能力,若缺失应返回 `ENOTSUP`。

**与 PCManager / Vantage 互斥**:同一时刻只能有一方控制 `LENOVO_GAMEZONE_DATA` 与 `LenovoProcessManagement` 服务;`lctrl` 应主动检测二者是否运行 (`QueryServiceStatus` / `GetNamedSecurityInfo` 命名管道),并提示用户先停止官方软件。

---

## 2. 性能模式体系

### 2.1 模式枚举

Windows 上的 ITS/Dispatcher 服务(`LenovoProcessManagement`)和 WMI `LENOVO_GAMEZONE_DATA` 各有一套独立的模式编号体系。`lctrl` 必须对二者同时写入,语义才能一致。

**A) ITS/Dispatcher 服务枚举 (`ItsModeType`)**

| 值 | 名称 | 中文标签 | 说明 |
|---|---|---|---|
| 0 | `None` | — | 无效 |
| 1 | `ItsAuto` | 智能 / 平衡 | 默认模式,服务根据场景在 Cool / Performance 间自切换 |
| 2 | `MmcCool` | 安静 | 静音优先,风扇策略低转速,PL1 低 |
| 3 | `MmcPerformance` | 野兽 / 性能 | 性能优先,全速风扇,PL1 高 |
| 4 | `MmcGeek` | 极客 | 仅 Dispatcher V4+ 支持;更高 PL2,可能开启 CPU OC |

**B) `LENOVO_GAMEZONE_DATA` WMI 侧模式编号**

WMI 类没有暴露枚举;通过方法 `SetSmartFanMode(in Data)` / `GetSmartFanMode(out Data)` 传递 `Data: UInt32`:

| Data | 名称 | 语义 |
|---|---|---|
| 0 | 标准 (Standard) | 默认风扇曲线 |
| 1 | 安静 (Silent) | 对应 ITS MmcCool |
| 2 | 性能 (Performance) | 对应 ITS MmcPerformance |
| 3 | 自定义 (Custom) | 走 `Fan_Set_Table` 的自定义曲线 |

**C) `lctrl perf mode` 对外枚举**

实现者应统一映射为三个面向用户的一级模式,内部同时更新 ITS 与 WMI:

| `lctrl` 参数 | ITS 服务值 | WMI `SetSmartFanMode` | WMI `SetFanCooling` |
|---|---|---|---|
| `quiet` (节能) | `MmcCool=2` | `1` (Silent) | `1` |
| `balanced` (智能/平衡) | `ItsAuto=1` | `0` (Standard) | `1` |
| `performance` (野兽) | `MmcPerformance=3` | `2` (Performance) | `2` |

> Geek 模式 (`MmcGeek=4`) 仅在 `IsDispatcherV4()` 返回 true 时可用,单独作为 `lctrl perf mode --geek` 子选项暴露。

### 2.2 注册表键

ITS/Dispatcher 版本与能力全部持久化于以下注册表:

```
HKLM\SYSTEM\CurrentControlSet\Services\LenovoProcessManagement\Performance\PowerSlider
```

| 子键 / 值 | 类型 | 用途 |
|---|---|---|
| `VERSION` | `DWORD` | 版本号: `0x1000`=Dispatcher V2, `0x2000`=V3, `0x3000`=V4 |
| `ITS_CURRENT_SETTING` | `DWORD` | 上一次通过服务下发的 ITS 模式号 |
| `ITS_CURRENT_SETTING_V` | `DWORD` | 与上相同(备用) |
| `CURRENT_SETTING` | `DWORD` | 用户"手动选择的"模式 (Fn+Q 未自动切时读它) |
| `CURRENT_STATE` | `DWORD` | "实际生效的"模式 (自动切换时会变化) |
| `AUTOMATIC_MODE_SETTING` | `DWORD` | 是否开启自动切换 |
| `ITS_FN_CAPABILITY` | `DWORD` | Fn+Q 能力位域:bit 5=把 Geek 标签改为 Creator,bit 12=把 BSM 标签改为 Quiet |
| `ITS_QUIET_PERFORMANCE_MODE_CAPABILITY` | `DWORD` | 静音/性能子模式能力位域 |
| `ITS_SE_HW_CAPABILITY` | `DWORD` | SmartEngine 硬件能力 |
| `ITS_SE_VANTAGESETTING` | `DWORD` | Vantage 侧设置 (与 PCManager 共享) |
| `POWER_SLIDER` | `DWORD` | 位域:支持的子模式集合 |
| `GPU_MODE` | `DWORD` | DGPU 工作模式 |

**版本判定算法**:

```
let v = ReadDWORD(REG, "PowerSlider\\VERSION")
if v >= 0x3000 → Dispatcher V4 (支持 MmcGeek)
else if v >= 0x2000 → Dispatcher V3
else if v >= 0x1000 → Dispatcher V2
else → 老 ITS 通道 (回退)
```

### 2.3 下发通道

**首选:Windows Service Control Manager (SCM) 直调**

1. 以管理员打开服务句柄:
   `OpenSCManager(NULL, NULL, SC_MANAGER_CONNECT)` → `OpenServiceW(hSC, "LenovoProcessManagement", SERVICE_USER_DEFINED_CONTROL)`。
2. 调用 `ControlService(hSvc, dwControlCode, &lp)`,其中 `dwControlCode` 采用如下常量 (与 ITS 模式一一对应,常量名来自 `PowerBattery.dll` 导出):

| 常量 | 语义 |
|---|---|
| `SERVICE_CONTROL_SET_INTELLIGENT` | 切到 `ItsAuto` |
| `SERVICE_CONTROL_SET_INTELLIGENT_COOLING_COOL` | 切到 `MmcCool` |
| `SERVICE_CONTROL_SET_INTELLIGENT_COOLING_HIGH_PERFORMANCE` | 切到 `MmcPerformance` |
| `SERVICE_CONTROL_SET_INTELLIGENT_COOLING_ENABLE` / `_DISABLE` | 开启/关闭 Intelligent Cooling |
| `SERVICE_CONTROL_SET_QUIET_PERFORMANCE_ENABLE` / `_DISABLE` | 静音/性能子模式切换 |
| `SERVICE_CONTROL_SET_iEPM_ENABLE` / `_DISABLE` | 进入/退出 iEPM |
| `SERVICE_CONTROL_SET_iGEEK_ENABLE` | 进入 Geek 模式 (V4+) |

**实际数值**:这些常量是 `SERVICE_CONTROL_USER_DEFINED=0x80` 起的连续数值,`PowerBattery.dll` 内联常量的绝对值随 AcpiVpc 版本变动;**实现者应优先从注册表 `VERSION` 对照版本并按下表顺序尝试**。
实机探测记录(2026-08-27):对 Dispatcher 连续下发 `0x80..0x8F` 共 16 个码,`ControlService`
**全部返回成功、无 `ERROR_INVALID_SERVICE_CONTROL`**,但空载下无可观测效应——即该服务的
控制处理器对未识别码也静默吞掉,**"调用成功"不等于"语义生效"**。判定语义必须依赖
可观测遥测(`LENOVO_DISPATCHER_EVENT` 的 `PowerLevel` 变化、负载下频率/风扇曲线),
逐码验证后再固化映射表。

> ⚠️ **实机勘误 (2026-08-27 三轮复核,ThinkBook 14 G8+ 21VG)**:直接 WMI 调用
> `LENOVO_GAMEZONE_DATA` 的方法族(`GetSmartFanMode`/`SetSmartFanMode`/`GetPowerChargeMode`/
> `IsSupportSmartFan`/`GetCPUTemp` 等)在本机**全部返回 `Invalid object`(0x80041008)**。
> 根因已实机定位(三层对照实验):同命名空间下
> `LENOVO_OTHER_METHOD.GetFeatureValue` 返回 `Invalid parameter`(方法体可达、参数不符);
> `LENOVO_FAN_METHOD`/`LENOVO_LIGHTING_METHOD`/`LENOVO_PANEL_METHOD` 返回 `Not supported`
> (方法体可达、固件声明不支持);唯 GAMEZONE 族为 `Invalid object`——即 **ACPI WMI 映射器
> 找不到对应的 AML 方法对象**:本机 BIOS 的 GMZN 作用域只有数据块,没有实现 GameZone 方法
> (GameZone 是 Legion 系列特性,ThinkBook 仅注册骨架)。与客户端栈/会话/权限无关
> (wmic 类静态调用、实例调用、CIM 三种路径已交叉验证;读固件表通道被 VBS 全局封锁,err=1,故以行为证据为准)。
> **结论:`LENOVO_GAMEZONE_DATA` 方法族在本机型属固件未实现,永久不可用,不是回退通道**;
> 模式切换走 `LenovoProcessManagement`(Dispatcher)SCM 控制消息或 ITS 服务契约。

### 2.4 回读与轮询

下发后 `lctrl` 必须**确认生效**,采用与 Vantage 一致的轮询策略:每 50 ms 读一次当前模式,最多 10 次 (500 ms 总超时),直到读回值与下发值一致。

- Dispatcher V4+: `GetDispatcherMode()` — 从 WMI `Lenovo_SetBiosSetting.CurrentSetting` 或 `LENOVO_GAMEZONE_DATA.GetThermalMode()` 联合推断,返回值是 `SupportedMmcModeType` bitmask。
- Dispatcher V2/V3 或 ITS: `GetITSMode()` — 从注册表 `CURRENT_STATE` 或 `CURRENT_SETTING` 读。

```
bit 0x01 = MmcAuto      (支持自动)
bit 0x02 = MmcCool      (支持安静)
bit 0x08 = MmcPerformance (支持性能)
bit 0x10 = MmcGeek      (仅 V4+)
```

**读/写分离语义**:若回写成功但轮询失败,应返回错误 `EIO` 并在 stderr 输出 "write succeeded but readback timed out"。若读失败,应**以注册表 `CURRENT_STATE` 为准**回显。

### 2.5 Fn+Q 三态切换状态机

```
[Boot]
    │  读取 CURRENT_SETTING
    ▼
[InitialMode]
    │
    ├── Fn+Q 按键 (EC 上报 → 服务通知 → `lctrl` 或官方软件拦截)
    │     └─► Intelligent ↔ Performance 循环切换 (默认行为)
    │
    ├── 电池电量 < 阈值 (AC/DC 事件, RegisterPowerSettingNotification)
    │     └─► 切到 Quiet (NonGamePowerStatusChange)
    │
    ├── AI 场景识别 (LENOVO_AI_SCENARIO_TYPE_EVENT)
    │     └─► Intelligent 子模式 (SetIntelligentSubMode 0/1)
    │
    └── 手动 CLI: `lctrl perf mode set <quiet|balanced|performance>`
          └─► 直接下发,覆盖 CURRENT_SETTING

[Auto Transition 开启]
    └─► 服务在 MmcCool ↔ MmcPerformance 之间自切换, CURRENT_STATE 变化但 CURRENT_SETTING 不变
```

### 2.6 `lctrl` CLI 契约

```
lctrl perf mode [get | set <quiet|balanced|performance> | list | geek-enable | geek-disable]
lctrl perf mode status                       # 输出: 模式, ITS版本, 支持子模式 bitmask
```

- `set`: 首选 SCM,失败回退 WMI + 注册表;返回 `ModeResult{ mode, its_version, readback_ok }`。
- `list`: 遍历 `SupportedMmcModeType` 位域输出可用模式。
- `geek-*`: 仅在 Dispatcher V4+ 允许。

---

## 3. 风扇

### 3.1 风扇硬件能力枚举

Windows 上先读 `LENOVO_FAN_TEST_DATA` 实例:

| 字段 | 类型 | 语义 |
|---|---|---|
| `NumOfFans` | `UInt32` | 风扇数量 (本机 21VG = 1) |
| `FanId[]` | `UInt32Array` | 每个风扇的硬件 ID |
| `FanMinSpeed[]` | `UInt32Array` | 每风扇最低转速,单位 RPM (本机 2100) |
| `FanMaxSpeed[]` | `UInt32Array` | 每风扇最高转速,单位 RPM (本机 5100) |

再读 `LENOVO_FAN_MAX_SPEED_DATA` 实例:

| 字段 | 类型 | 语义 |
|---|---|---|
| `Fan_Id` | `UInt8` | 风扇硬件 ID |
| `Fan_CurrentMaxSpeed` | `UInt16` | 当前生效的最大转速 (RPM 或百分比,见 `Fan_Flag`) |
| `Fan_DefaultMaxSpeed` | `UInt16` | BIOS 出厂默认最大转速 |
| `Fan_Flag` | `UInt8` | 单位标志: `0` = RPM, `1` = 百分比 (0..255) [推断] |

### 3.2 风扇策略 (WMI 方法族)

| WMI 方法 | 方向 | 参数 |
|---|---|---|
| `LENOVO_GAMEZONE_DATA.IsSupportFanCooling(out Data)` | 读 | `Data: 0`=不支持, `1`=支持 |
| `LENOVO_GAMEZONE_DATA.SetFanCooling(in Data)` | 写 | `0`=关闭, `1`=Smart, `2`=Performance |
| `LENOVO_GAMEZONE_DATA.GetFanCoolingStatus(out Data)` | 读 | 同上 |
| `LENOVO_GAMEZONE_DATA.IsSupportSmartFan(out Data)` | 读 | 是否支持 Smart Fan 子模式 |
| `LENOVO_GAMEZONE_DATA.SetSmartFanMode(in Data)` | 写 | `0`=标准, `1`=安静, `2`=性能, `3`=自定义 |
| `LENOVO_GAMEZONE_DATA.GetSmartFanMode(out Data)` | 读 | 同上 |
| `LENOVO_GAMEZONE_DATA.GetSmartFanSetting(out Data)` | 读 | 自定义曲线参数 |
| `LENOVO_GAMEZONE_DATA.SetSmartFanStatus(in Data)` | 写 | `0`=关闭 Smart Fan, `1`=开启 |
| `LENOVO_GAMEZONE_DATA.GetSmartFanStatus(out Data)` | 读 | 同上 |
| `LENOVO_GAMEZONE_DATA.StartFan` | 写 | 强制风扇转 (参数 `0`=开) |
| `LENOVO_GAMEZONE_DATA.StopFan` | 写 | 停风扇 (参数 `0`=停) |

**Smart Fan 模式含义**:
- **标准 (0)**:BIOS 出厂曲线,自动调速,转速受温度驱动。
- **安静 (1)**:降低风扇曲线斜率,牺牲性能换静音;对应 ITS `MmcCool`。
- **性能 (2)**:提高风扇曲线斜率,提前升速;对应 ITS `MmcPerformance`。
- **自定义 (3)**:走 `Fan_Set_Table` 用户曲线。

### 3.3 风扇曲线表 (`FanTable`) 数据结构

WMI 类 `LENOVO_FAN_TABLE_DATA` 是一个**实例数组**,每个实例对应"一个 Fan-Sensor 组合"的一条曲线:

```
LENOVO_FAN_TABLE_DATA (实例化多次,一次取一个曲线)
├─ InstanceName          : String      实例名 (内部标识)
├─ Fan_Id                : UInt16      风扇硬件 ID
├─ Sensor_ID             : UInt32      关联的温度传感器 ID
├─ Mode                  : UInt16      曲线模式: 0=标准, 1=静音, 2=性能
├─ DesignMaxFanSpeedNumber : UInt8     曲线阶数上限
├─ FanSpeedStep          : UInt16      风扇转速步进, 单位 RPM
├─ SensorTemperatureStep : UInt16      温度步进, 单位 0.1 °C
├─ MinSensorTemperature  : UInt16      曲线最低温度阈值, 单位 0.1 °C
├─ MaxSensorTemperature  : UInt16      曲线最高温度阈值, 单位 0.1 °C
├─ CurrentFanMinSpeed    : UInt16      当前风扇最低转速 (RPM)
├─ CurrentFanMaxSpeed    : UInt16      当前风扇最高转速 (RPM)
├─ StartOnlyUpwardAdjustNumber : UInt8 起始仅升阶点数
├─ EndOnlyUpwardAdjustNumber   : UInt8 尾部仅升阶点数
├─ Reserved              : UInt8
├─ FanTable_Data         : UInt16Array 曲线数据 (长度 = FanTable_Len)
├─ FanTable_Len          : UInt32      FanTable_Data 元素个数
├─ SensorTable_Data      : UInt16Array 传感器表数据
└─ SensorTable_Len       : UInt32
```

**`FanTable_Data` 曲线编码 (关键,PCManager 证据强烈支持)**:

通过 `LENOVO_FAN_METHOD.Fan_Set_Table(in FanTable: UInt8Array)` 写入时,数组每个台阶占 **4 字节**:

```
offset 0..1  : 温度阈值 (UInt16, 单位 0.1 °C)
offset 2..3  : 目标风扇转速 (UInt16, 单位 RPM)
```

阶数 = `FanTable_Len / 2` (元素数 = 温度阶数 × 2,每个元素 UInt16 对应 `Fan_Get_Table` 的 `UInt32Array` 元素,每元素 = 一阶 {温度, 转速} 打包)。

> **注意 `Fan_Get_Table` 与 `Fan_Set_Table` 的类型差异**:
> - `Fan_Get_Table(FanID, SensorID)`: 返回 `UInt32Array FanTable + UInt32 FanTableSize + UInt32Array SensorTable + UInt32 SensorTableSize`。每个 `UInt32` 元素是 `{低 16 位 = 温度, 高 16 位 = 转速}` 的打包。
> - `Fan_Set_Table(UInt8Array)`: 输入 `UInt8Array`,长度 = `FanTableSize × 2 × 4` (每阶 4 字节,字节顺序 = 小端)。
> - 实现者必须做一次类型转换 (32-bit 拆 16-bit 再序列化)。

**读取接口 `Fan_Get_Table`**:
```
参数 FanID: UInt8  — 风扇硬件 ID (来自 LENOVO_FAN_TEST_DATA.FanId[])
参数 SensorID: UInt8 — 传感器 ID (通常 0 代表 CPU 温度, 1 代表 GPU 温度 [推断])
返回 FanTable: UInt32Array, 长度 FanTableSize
返回 SensorTable: UInt32Array, 长度 SensorTableSize
```

**转速百分比换算**:
```
percentage = (rpm - FanMinSpeed) / (FanMaxSpeed - FanMinSpeed) * 100
```
RPM 单位在 `Fan_Flag = 0` 时直接使用;`Fan_Flag = 1` 时需要乘以 `(FanMaxSpeed / 255)` 换算。

### 3.4 直读通道 (`\\.\SIO8786`)

PCManager `GameSettingsPlugin.dll` 通过 `DeviceIoControl` 打开 `\\.\SIO8786` (Intel Super I/O 芯片, 挂载在 EC 上), 用自定义命令字 `SIO8786GetFan` 实时读取当前风扇实际转速,绕过 WMI 缓存。

- **设备路径**:`\\.\SIO8786`。
- **方法**:`CreateFile` 打开, `DeviceIoControl` 发送 IO 控制码;具体的 `IOCTL_CODE` 数值在 `GameSettingsPlugin.dll` 内部,随驱动版本变动;实现者应:
  1. 优先使用 WMI 回读 (`GetFanCoolingStatus` / `Fan_Get_Table` 中的当前阶段) 获取"目标"转速。
  2. 若设备节点存在,尝试直读作为"实际"转速;若失败降级为 WMI 目标值。
- **Linux 后端**:无等效 `SIO8786` 设备。通过 `/sys/devices/platform/lenovo_*` 或 ACPI `THRF` 暴露实际转速,若缺失则回退 WMI (Windows 通过 SMBIOS 或 /proc/acpi/thermal_zone)。

### 3.4b 备选写入通道 (`LnCoolit.sys`,未决)

PCManager 另捆绑自有散热驱动 `Modules/drivers/LnCoolit/x64/LnCoolit.sys`(由 `IdeaFanPlugin.dll` 装载使用,
推测用于下发自定义风扇曲线)。**其 IOCTL 码表未完成解析**——净室实现不需要它:
WMI `LENOVO_FAN_METHOD`(§3.3 `Fan_Set_Table`)已覆盖风扇曲线写入,`LnCoolit` 仅为官方的冗余私有路径。
标注 [推断];若实测发现 `Fan_Set_Table` 在目标机不可用,再回到 LnCoolit 组件分析。
证据:`电脑管家组件内部结构说明。

### 3.5 手动 / 自动 / 全速模式

`lctrl` 应暴露三种风扇控制"档位",各自对应 WMI 层不同组合:

| `lctrl` 档 | WMI 组合 |
|---|---|
| `auto` (默认) | `SetSmartFanMode(2)` + `SetFanCooling(1)` |
| `manual <rpm\|%>` | `SetSmartFanMode(3)` + `Fan_Set_Table([...])` 构建单台阶曲线 |
| `fullspeed` | `SetFanCooling(2)` (Performance) + `SetSmartFanMode(2)`; 或通过 `SetFeatureValue` 全速标志 [推断] |
| `off` | `SetFanCooling(0)` 或 `StopFan` |
| `smart` | `SetSmartFanStatus(1)` + `SetSmartFanMode(0)` |

**`manual <rpm>` 语义**:把整个 `FanTable` 覆盖为**两个台阶**:
1. 温度阈值 0°C (起点),目标转速 = 请求的 RPM 值。
2. 温度阈值 = `MaxSensorTemperature`,目标转速 = 请求的 RPM 值。

这样无论温度如何,风扇都被"锁定"在请求的转速。

**`fullspeed` 语义**:全速模式。除调用 `SetFanCooling(2)` 外,还建议通过 `LENOVO_GAMEZONE_DATA.IsSupportOD(out Data)` + `SetODStatus(1)` 开启 OverDrive (若硬件支持)。

### 3.6 `lctrl` CLI 契约

```
lctrl perf fan list                          # 输出所有风扇: FanId, MinSpeed, MaxSpeed
lctrl perf fan status                        # 输出当前策略: SmartFanMode, FanCooling, SmartFanEnabled
lctrl perf fan table <fan-id>                # 输出当前曲线的温度-转速对
lctrl perf fan set <mode> [--rpm N | --pct P]  mode ∈ {auto|manual|smart|fullspeed|off}
lctrl perf fan table-write <path.json>       # 加载外部 JSON 曲线 (自定义模式)
```

JSON 曲线 schema:
```json
{
  "fan_id": 0,
  "sensor_id": 0,
  "mode": "standard|silent|performance|custom",
  "steps": [
    {"temp_c": 40.0, "rpm": 2500},
    {"temp_c": 55.0, "rpm": 3500},
    {"temp_c": 70.0, "rpm": 4500},
    {"temp_c": 85.0, "rpm": 5100}
  ]
}
```

---

## 4. 温度传感器枚举与读取

### 4.1 传感器来源

`lctrl perf temp` 需要聚合三种温度来源:

| 来源 | WMI/接口 | 粒度 |
|---|---|---|
| EC 报告 (WMI) | `LENOVO_GAMEZONE_DATA.GetCPUTemp()` / `.GetGPUTemp()` | 单值 UInt32 (单位 °C,整数) |
| 曲线关联 | `LENOVO_FAN_TABLE_DATA.Sensor_ID` + `LENOVO_FAN_METHOD.SensorTable` | 每个 Fan-Sensor 组合一列 |
| OS 热区 (Linux) | `/sys/class/thermal/thermal_zone<N>/type` + `temp` | 内核热区,单位 mK |

### 4.2 `LENOVO_GAMEZONE_DATA` 温度方法

| 方法 | 返回值 | 语义 |
|---|---|---|
| `GetCPUTemp(out Data)` | `Data: UInt32`, °C | 封装后当前 CPU 温度 |
| `GetGPUTemp(out Data)` | `Data: UInt32`, °C | 封装后当前 dGPU 温度 (无独显返回 0 或 `0xFFFF`) |
| `GetCpuFrequency(out Data)` | `Data: UInt32`, MHz | 当前 CPU 频率 (可作性能侧面指标) |

> 温度传感器**枚举**通过遍历 `LENOVO_FAN_TABLE_DATA` 实例,每个实例的 `Sensor_ID` 唯一标识一个传感器;传感器物理位置 (CPU / GPU / 电池 / 主板) 由 `InstanceName` 字符串推断 (通常包含 `CPU`, `GPU`, `BAT`, `MAIN` 关键字)。

### 4.3 Linux 后端映射

Linux 上应优先使用 `/sys/class/thermal/` 下的内核热区:

```
/sys/class/thermal/thermal_zone0/type   → "x86_pkg_temp" 或 "cpu_package"
/sys/class/thermal/thermal_zone0/temp   → 温度, 单位 mK
```

`lctrl` 应构建**别名映射**:

| WMI 语义 | Linux 类型字符串 |
|---|---|
| CPU | `x86_pkg_temp`, `cpu_package`, `coretemp`, `k10temp` |
| GPU | `nvgpu`, `radeon`, `nvidia_gpu` |
| 电池 | `BAT0`, `battery` |
| 主板 | `acer_wmi` (ThinkBook 无, 仅参考), `ideapad` |

### 4.4 `lctrl` CLI 契约

```
lctrl perf temp list          # 列出所有传感器: id, name, source, location
lctrl perf temp read <id>     # 读取单一传感器 (°C)
lctrl perf temp watch         # 持续轮询 (默认 1s 间隔)
```

返回 JSON:
```json
{
  "id": "cpu0",
  "name": "CPU Package",
  "source": "wmi|sysfs|acpi",
  "value_c": 62.0,
  "max_c": 100.0,
  "critical_c": 105.0
}
```

---

## 5. 功耗墙 (PL1 / PL2 / DBDC / 超频)

### 5.1 通道判定 (关键)

`lctrl perf pl1` / `pl2` 的下发通道**取决于运行平台与 DTT 状态**:

| 平台 | 首选通道 | 降级通道 |
|---|---|---|
| Windows + DTT 服务运行 | ESIF IPC(`ipfsvc.exe`,DTT 客户端协议;named pipe/TCP localhost) | Windows Power API (`GUID_PROCESSOR_THROTTLE`) |
| Windows + 无 DTT | Windows Power API | (返回 ENOTSUP) |
| Linux | `ideapad_laptop` sysfs (`intel_pstate` / `amd-pstate`) | (返回 ENOTSUP, 除非内核支持写 `/sys/devices/cpu/intel_pstate/*`) |

> **实机更正(2026-08-27)**:`root\WMI` 中**不存在** `Intel_TuningTechnologyService` 或任何
> Intel/DTT WMI 类(实机全量枚举,`目标机实机接口探测记录`)。DTT 用户态控制面只有
> ESIF IPC 与 BIOS 间接路径;不要按 WMI 类名探测。

**分析结论** (PCManager 5.1):

- **节能 / 智能模式**下,PCManager **不直写 DTT**。CPU PL1 通过 `PowerWriteACValueIndex(GUID_PROCESSOR_THROTTLE, …)` 落到 Windows 电源方案,由 Windows 电源管理器自动转发到 IPF Power Participant。
- **性能 / Smart Fan 模式**下,PCManager 使用 `SetGpuTDPWithSMFAN_DT` 和 `SetGpuTemperatureWithSMFAN_DT` **直接**修改 DPTF Power Participant 的 GPU TDP 和 GPU 温度墙 (方法名含 `SMFAN`,来源: GameSettingsPlugin 组件字符串资源)。
- **CPU PL1/PL2 在 DTT 上**经 ESIF IPC 到达 `ipfsvc.exe`;`ipf_acpi` 驱动挂载在 `ACPI\INTC10D8\TPWR`(Power Participant) 和 `ACPI\INTC10D4\IETM`(IPF 管理器)。PCManager 的 `*_DT` 名字是其 RPC 契约名,不是 WMI 类。

### 5.2 PL1 / PL2 读写契约

**读 (PL1)**:
1. ESIF IPC:向 `ipfsvc.exe` 查询 TPWR 参与者的当前功率限(ESIF 客户端协议,`participant get` 原语 [推断命令名])。
2. 备选: `Lenovo_SetBiosSetting.CurrentSetting` + `InstanceName` 匹配 `PowerPolicy`。
3. 底层: MSR `0x610 PKG_POWER_LIMIT` 直读(需 ring0 驱动)。
4. Linux: 读 `/sys/devices/system/cpu/intel_pstate/max_perf_pct` (百分比, 换算到 W 需结合 TDP 标称)或 RAPL `constraint_0_power_limit_uw`。

**写 (PL1)**:
1. ESIF IPC:写 TPWR 参与者功率限,单位 mW。
2. 备选: `PowerWriteACValueIndex(acdc, GUID_PROCESSOR_THROTTLE, 0, 0, &dw, sizeof(dw))`,其中 `dw` 是百分比 (0..255)。
3. 底层: MSR `0x610` 直写(需 ring0;**可能被 EC 周期覆写**,见 07 §3.1)。
4. Linux: 写 `/sys/devices/system/cpu/intel_pstate/max_perf_pct` 或 RAPL sysfs(若只写百分比, 换算: W = TDP × pct / 100)。

**PL2 差异**:PL2 是短时 (10s) 峰值功率, 在 ESIF/DTT 参与者的 `PPLimitShort` 字段
(对应 MSR 0x610 的 Limit2 位段 / RAPL `constraint_1_*`)。`lctrl` 应:
- 枚举 ESIF 参与者列表,定位 TPWR(Power Participant,`ACPI\INTC10D8\TPWR`)。
- 分别读 `PPLimit` (PL1) 和 `PPLimitShort` (PL2) 字段。

### 5.3 DBDC — 电池直充控制

`LENOVO_REPORT_DBDC_DATA` WMI 类:

| 字段 | 类型 | 语义 |
|---|---|---|
| `Counts` | `UInt32` | 阈值档数 (本机 = 3) |
| `CurrentLimit[]` | `UInt32Array` | 各档电流上限 (单位 mA) |
| `ROS_Power[]` | `UInt32Array` | 各档系统可用功率 (单位 mW) |
| `Threshold[]` | `UInt32Array` | 电池电量百分比阈值 (高 → 低) |

本机实测 (来自 `目标机 WMI 实例实机采集`):

```
CurrentLimit = [7500, 5000, 4500]   # 7.5A / 5A / 4.5A
Threshold    = [100,  40,  20]      # >100%(充电中), 40%, 20%
ROS_Power    = [对应 3 档系统可用功率]
```

语义:当电池电量 ≥ 某档 `Threshold[i]` 时,EC 限制电池放电电流为 `CurrentLimit[i]`,系统功率上限为 `ROS_Power[i]`。典型行为:电池满电 (≥ 100%) 允许最大放电;电量 < 20% 时严格限流以保护电池。

**`lctrl perf dbdc get`**:输出三档阈值与上限。
**`lctrl perf dbdc set <threshold> <limit_mA>`**:通过 `LENOVO_OTHER_METHOD.SetFeatureValue(IDs=<DBDC_ID>, value=<limit>)` 写入 (IDs 常量需从 `LENOVO_CAPABILITY_DATA_00` / `_01` / `_02` 枚举中获得)。

### 5.4 超频 (CPU / GPU / 内存 OC 数据类)

> **硬件适配提示**:目标机 21VG (Panther Lake) **没有独立 GPU**, `LENOVO_GPU_OVERCLOCKING_DATA` 实例通常返回"不支持";CPU 与内存 OC 也可能被 SMBIOS 过滤。`lctrl` 应**先枚举**再暴露子命令;若所有 OC 能力均 `IsSupport=0`, 则隐藏 `lctrl perf oc` 命令并在 help 中说明 "OC not supported on this platform"。

**CPU 超频**:`LENOVO_CPU_OVERCLOCKING_DATA` + `LENOVO_CPU_METHOD` + `LENOVO_GAMEZONE_CPU_OC_DATA`

| 类 | 关键字段 |
|---|---|
| `LENOVO_CPU_OVERCLOCKING_DATA` | `CpuType` (UInt8), `mode` (UInt8), `DefaultValue`, `MinValue`, `MaxValue`, `Interval`, `ScaleValue`, `OCValue`, `Tuneid`, `NOCOrderid`, `OCOrderid`, `Reserved` |
| `LENOVO_GAMEZONE_CPU_OC_DATA` | `DefaultValue`, `MinValue`, `MaxValue`, `Interval`, `OCValue`, `ScaleValue`, `Tuneid`, `NOCOrderid`, `OCOrderid` |
| `LENOVO_CPU_METHOD` | `CPU_Set_OC_Data(mode: UInt8, TuneID: UInt32, value: UInt32) -> Boolean` |

- **能力探测**:`LENOVO_GAMEZONE_DATA.IsSupportCpuOC(out Data)`, `Data=1` 表示支持。
- **BIOS 层 OC**:`LENOVO_GAMEZONE_DATA.IsBIOSSupportOC(out Data)`, `SetBIOSOC(in Data)` 下发 (需重启生效)。
- **读取当前 OC**:`LENOVO_GAMEZONE_DATA.GetBIOSOCMode(out Data)`。

**GPU 超频**:`LENOVO_GPU_OVERCLOCKING_DATA`

| 关键字段 |
|---|
| `GpuType` (UInt8), `mode`, `Capability`, `ClockID`, `PStateID`, `OCOffsetFreq`, `OCOffsetScale`, `OCMinOffset`, `OCMaxOffset`, `defaultvalue`, `Interval`, `Tuneid`, `NOCOrderid`, `OCOrderid` |

能力探测: `LENOVO_GAMEZONE_DATA.IsSupportGpuOC(out Data)`。

**内存超频**:`LENOVO_MEMORY_OC_DATA` + `LENOVO_MEMORY_METHOD`

`LENOVO_MEMORY_OC_DATA` 字段 (大量):

| 字段 | 类型 | 语义 |
|---|---|---|
| `MEM_OC_Ability` | `UInt8` | 能力位域 |
| `MEM_OC_Frequency_Scaler` | `UInt16` | 频率缩放因子 |
| `MEM_OC_Min_Frequency` | `UInt16` | 最小频率 (MHz) |
| `MEM_OC_Max_Frequency` | `UInt16` | 最大频率 (MHz) |
| `MEM_OC_Default_Frequency` | `UInt16` | 出厂默认频率 |
| `MEM_OC_Customize_Frequency` | `UInt16` | 用户自定义频率 |
| `MEM_OC_Customize_NMode` | `UInt16` | 自定义 N-Mode |
| `MEM_OC_Customize_tCL` | `UInt16` | 自定义 CAS Latency |
| `MEM_OC_Customize_tCLK` | `UInt16` | 自定义 tCLK |
| `MEM_OC_Customize_tCWL` | `UInt16` | 自定义 tCWL |
| `MEM_OC_Customize_tFAW` | `UInt16` | 自定义 tFAW |
| `MEM_OC_Customize_tRAS` | `UInt16` | 自定义 tRAS |
| `MEM_OC_Customize_tRCD_tRP` | `UInt16` | 自定义 tRCD / tRP |
| `MEM_OC_Customize_tREFI` | `UInt16` | 自定义 tREFI |
| `MEM_OC_Customize_tRFC` | `UInt16` | 自定义 tRFC |
| `MEM_OC_Customize_tRRD` | `UInt16` | 自定义 tRRD |
| `MEM_OC_Customize_tRTP` | `UInt16` | 自定义 tRTP |
| `MEM_OC_Customize_tWTR` | `UInt16` | 自定义 tWTR |
| `MEM_OC_Customize_VDD` | `UInt16` | 自定义电压 (mV) |
| `MEM_OC_XMP_Numbers` | `UInt8` | XMP profile 数量 |

方法:

| WMI 方法 | 参数 |
|---|---|
| `LENOVO_MEMORY_METHOD.MEM_Get_OC_Status(mode: UInt8, out Status: UInt8)` | 读取某 mode 下的状态 |
| `LENOVO_MEMORY_METHOD.MEM_Set_OC_Status(mode: UInt8, Status: UInt8)` | 开关某 mode |
| `LENOVO_MEMORY_METHOD.MEM_Set_OC_Data(MEM_OCData: UInt8Array)` | 写入完整自定义配置 |

`LENOVO_GAMEZONE_DATA.GetMemoryOCInfo(out Data)` 返回压缩的内存 OC 状态,`OCBindWithThermal` 是内部方法符号 (超频状态与温控模式绑定)。

### 5.5 功耗墙与超频的联动

- 超频启用后,PCManager 会通过 `OCBindWithThermal` 将 OC 与温控模式绑定:智能模式下禁用 OC,野兽模式下启用 OC。`lctrl` 应实现类似策略:当 `perf mode set performance` 时,若硬件支持 OC 且用户配置 `oc_with_perf=1`,则自动开启 CPU OC。
- GPU OC 与 `SetGpuTDPWithSMFAN_DT` / `SetGpuTemperatureWithSMFAN_DT` 联动:OC 模式下 GPU TDP 上限提高,GPU 温度墙提高 (从默认 75°C 提高到 85°C) [推断自笔记 3.3 参数表]。

### 5.6 `lctrl` CLI 契约

```
lctrl perf pl1 [get | set <mW>]           # CPU 长时功耗墙
lctrl perf pl2 [get | set <mW>]           # CPU 短时峰值
lctrl perf dbdc [get | set <thr> <mA>]    # DBDC 电池直充
lctrl perf oc cpu   [get | set <MHz> | enable | disable]
lctrl perf oc gpu   [get | set <MHz> | enable | disable]
lctrl perf oc mem   [get | set-xmp <n> | customize <json>]
```

---

## 6. 智能调度 (LNVDispatcherService / ResScheduler 等价)

### 6.1 官方实现机制

PCManager 的 `ResScheduler.exe` 是**进程级**智能调度器,通过以下 API 生效:

| API | 用途 |
|---|---|
| `NtQuerySystemInformation(SystemProcessInformation)` | 枚举系统进程 |
| `PdhCollectQueryData` | PDH 计数器,读 CPU 时间/工作集 |
| `SetProcessAffinityMask` | 进程亲和性 (P-core / E-core) |
| `GetLogicalProcessorInformationEx(RelationGroupRelationship)` | 枚举处理器组,识别 E-core (Group 1) vs P-core (Group 0) |
| `SetThreadPriority` | 线程优先级调节 |
| `SetProcessWorkingSetSizeEx` | 后台进程工作集收缩 |
| `AppPolicyGetThreadInitializationType` / `AppPolicyGetProcessTerminationMethod` | 只读:查询 QoS / EcoQoS 状态 |
| `RegisterPowerSettingNotification` | 订阅 `GUID_PROCESSOR_POWER_SAVING` / `GUID_ACDC_POWER_SOURCE` |

**与 Thread Director 关系**:PCManager 未使用 `NtSetInformationProcess(ProcessGroupAffinity)` 或 `SetThreadGroupAffinity`,而是"设置 64 位亲和性掩码 + 保留所有组",让 Windows 11 内核的 Thread Director 在 P/E 之间自然选择。这与 Thread Director 天然兼容。

**未使用 EcoQoS API**:`ProcessEnergyPreference` / `ProcessEnergyCap` / `ProcessMemoryPriority` 在样本中均未出现,PCManager 5.1 采用"工作集收缩 + 线程优先级降级"代替。

### 6.2 进程分类维度

1. **前台窗口**:通过 `WTSGetActiveConsoleSessionId` + `GetWindowThreadProcessId` 关联前台窗口进程。
2. **游戏白名单**:`UpdateGameWhiteList` 从注册表 `HKCU\SOFTWARE\Lenovo\LenovoPcManager\ResScheduler` 载入。
3. **CPU 密集**:PDH `ProcessorTime` 超过 `ProcessThreshold` 阈值。
4. **GPU 相关**:与 `DataPlugin.dll` 提供的 DGPU 状态交叉。
5. **后台系统进程**:排除列表。

### 6.3 策略来源 (IPC 消息)

主程序通过 JSON 命名 IPC 与调度器通信:

```
NEWPLUGIN_PROCESSORSCHEDULER_MESSAGE_GETINFO
NEWPLUGIN_PROCESSORSCHEDULER_MESSAGE_GETSTATUS
NEWPLUGIN_PROCESSORSCHEDULER_MESSAGE_LOADCONFIG
NEWPLUGIN_PROCESSORSCHEDULER_MESSAGE_QUERYPROCESSES
NEWPLUGIN_PROCESSORSCHEDULER_MESSAGE_RESTOREPROCESSES
NEWPLUGIN_PROCESSORSCHEDULER_MESSAGE_SETSTATUS
NEWPLUGIN_PROCESSORSCHEDULER_MESSAGE_UPDATECONFIG
NEWPLUGIN_WSWORKINGSETPLUGIN_MESSAGE_CHECKPARAMTER
```

策略加载:本地规则 (`LOADCONFIG`) → 云端热更新 (`UPDATECONFIG` + `GetContentHash` 校验) → `QueryProcesses` 防抖触发 → `RestoreProcesses` 还原。

### 6.4 `lctrl` 的最小可用等价

`lctrl` 是**无状态 CLI**,不支持持续进程调度守护。因此 `lctrl perf schedule` 只做**一次性**动作:

```
lctrl perf schedule top        # 输出当前前台 + 高 CPU 进程 (读)
lctrl perf schedule boost <pid> [--core=p|e|auto]   # 提升单一进程优先级/亲和性
lctrl perf schedule throttle <pid>                   # 降低优先级 + 工作集收缩
```

- `boost`:调用 `SetProcessPriorityBoost`, `SetThreadPriority(HIGH)`, 若识别到 P/E 组则设置亲和性到 P-core。
- `throttle`: `SetThreadPriority(BELOW_NORMAL)`, `SetProcessWorkingSetSizeEx(min, min)`。
- `top`: 使用 `NtQuerySystemInformation(SystemProcessInformation)` 输出 Top-10 CPU 消耗进程。

Linux 后端:用 `ps` / `/proc/<pid>/stat` 替代 PDH;`chrt` 设置调度策略;`cgset` / cgroups v2 `memory.max` 替代工作集收缩。

---

## 7. PCManager 省电策略 — 系统电源设置 (Power API) 覆盖

### 7.1 不新增电源方案

PCManager **不创建新的电源计划**,而是复用系统内置方案 (平衡 / 节能 / 高性能) 后,通过 `PowerWrite*ValueIndex` 覆盖高级电源设置。`lctrl` 应遵循同一策略,避免污染系统电源方案列表。

### 7.2 覆盖的 GUID 域

| GUID | 中文 | 覆盖场景 |
|---|---|---|
| `GUID_PROCESSOR_THROTTLE` | 处理器 throttle 限制 | PL1 百分比上限 |
| `GUID_VIDEO_SUBGROUP` | GPU 子组 | GPU 节能 / 性能 |
| `GUID_PROCESSOR_IDLE_SUBGROUP` | 处理器空闲 | C-state 深度 |
| `GUID_DISK_SUBGROUP` | 磁盘 | 硬盘停止时间 |
| `GUID_SYSTEM_SUBGROUP → PROC_THR_STATE` | 处理器状态 | 处理器节流状态 |
| `GUID_ACDC_POWER_SOURCE` | AC/DC 电源通知 | 触发模式切换 |
| `GUID_PROCESSOR_POWER_SAVING` | 处理器节能 | 节能策略 |

### 7.3 读/写 API 契约

**读**:`PowerReadACValueIndex(schemeGUID, subGroupGUID, powerSettingGUID, 0, &type, &bytes, &value, &bytesRequired)`,DC 同理。

**写**:`PowerWriteACValueIndex(...)` / `PowerWriteDCValueIndex(...)` 分别对 AC / DC 电源状态生效。

**切换方案**:`PowerSetActiveScheme(NULL, &schemeGUID)`。

### 7.4 三种模式下的具体覆盖 (推断,基于笔记 3.3 参数表)

| 参数 | Quiet | Balanced | Performance |
|---|---|---|---|
| CPU PL1 (%PL1) | 低 (~30%) | 中 (~60%) | 高 (~100%) |
| CPU PL2 | 跟随 PL1 | PL1 × 1.5 | 跟随 DTT 策略 |
| GPU TDP | `SetGpuTDPWithSMFAN_DT` → 50% | 跟随场景 | 100% |
| GPU 温度墙 | `SetGpuTemperatureWithSMFAN_DT` → 70 °C | 75 °C | 85 °C |
| C-state 深度 | 深 | 中 | 浅 |
| 硬盘停止 | 短 | 中 | 关 |
| 内存 OC | 关 | 关 | 允许 |

### 7.5 `lctrl` 持久化

`lctrl` 应将每次 `pl1` / `pl2` / `mode` 变更**同时写入注册表**(Windows) 或 `~/.config/lctrl/perf.json`(Linux),以支持 `--restore` 恢复默认。

```
lctrl perf mode set performance   # 同时写入 CURRENT_SETTING
lctrl perf pl1 set 15000          # 写入 GUID_PROCESSOR_THROTTLE 与注册表
lctrl perf pl1 restore            # 从注册表恢复
```

---

## 8. WMI 事件订阅

`lctrl perf watch` 与 daemon 模式需要订阅以下 WMI 事件类 (通过 WQL `SELECT * FROM __InstanceModificationEvent` 或 `__EventFilter` + `__FileWatcher`)。

### 8.1 事件类全表

| WMI 事件类 | 关键字段 | 触发时机 | `lctrl` 用途 |
|---|---|---|---|
| `LENOVO_GAMEZONE_THERMAL_MODE_EVENT` | `mode: UInt32` | 温控模式变化 (Fn+Q / CLI) | 同步 `mode` 状态 |
| `LENOVO_GAMEZONE_SMART_FAN_MODE_EVENT` | `mode: UInt32`, `version: UInt32` | Smart Fan 模式变化 | 同步风扇策略 |
| `LENOVO_GAMEZONE_SMART_FAN_SETTING_EVENT` | `mode: UInt32` | 用户修改自定义曲线 | 重载 `fan table` |
| `LENOVO_GAMEZONE_FAN_COOLING_EVENT` | `EventId: UInt32` | 风扇冷却启停 | 同步 `fan status` |
| `LENOVO_GAMEZONE_POWER_CHARGE_MODE_EVENT` | `mode: UInt32` | 充电模式变化 (与热有关:快充发热) | 联动告警 |
| `LENOVO_DISPATCHER_EVENT` | `PowerLevel: UInt32` | Dispatcher 分派事件 | 模式切换通知 |
| `LENOVO_REPORT_STATUS_TO_DISPATCHER_EVENT` | `Type`, `Value` | 通用回拨 | 通用 |
| `LENOVO_REPORT_2D3D_STATUS_EVENT` | `Status` (0=2D, 1=3D) | DGPU 前后切 | 联动 GPU OC |
| `LENOVO_AI_SCENARIO_TYPE_EVENT` | `Type` (0..N) | AI 场景识别 | 智能模式子状态 |
| `LENOVO_AI_CHIP_EVENT` | `Status` | NPU 状态 | (可选) |
| `LENOVO_REPORT_POWER_CONSUMPTION_CHANGE_EVENT` | `ModeID[]`, `PowerConsumption[]`, `NumbersOfMode` | 每模式功耗统计 | PL1/PL2 实际值 |
| `LENOVO_SMART_THERMAL_MONITOR_EVENT` | `Status` | 智能温控状态 | 高温告警 |

### 8.2 订阅机制

Windows 上通过 WMI Event Query:

```
WQL: "SELECT * FROM LENOVO_GAMEZONE_THERMAL_MODE_EVENT"
```

`lctrl perf watch` 应:
1. 打开 `root\WMI` 命名空间。
2. 创建 `__EventFilter` + `__EventConsumer` (或直接 `IWbemServices::CreateAsyncQuery`)。
3. 循环读取事件,格式化为 JSON 行输出。
4. Ctrl+C 时注销所有事件订阅。

Linux 上无 WMI;应:
1. 监听 `/sys/class/thermal/thermal_zone*/temp` (inotify)。
2. 监听 `udev` 事件 (AC 插入/拔出)。
3. 轮询 ACPI `THRF` 对象 (若内核暴露)。

### 8.3 热事件阈值 (推断)

`LENOVO_SMART_THERMAL_MONITOR_EVENT` 的 `Status` 字段值推断:

| Status | 语义 |
|---|---|
| 0 | 正常 |
| 1 | 温度接近阈值 (警告) |
| 2 | 高温 (触发降频) |
| 3 | 过热 (触发关机保护) |

`lctrl` 在收到 Status ≥ 1 时应输出告警日志。

---

## 9. 错误模型与边界情况

| 错误码 | 场景 |
|---|---|
| `ENOENT` | WMI 类不存在 (非 Lenovo 机器) |
| `ENOTSUP` | 硬件能力检测返回 0 (`IsSupport*=0`) |
| `EPERM` | 非管理员运行 |
| `EIO` | WMI 调用返回非 0 HRESULT |
| `ETIMEDOUT` | 下发后轮询 500ms 未确认 |
| `EBUSY` | PCManager 或 Vantage 正在控制同一通道 |

**降级策略**:
1. 首选通道失败 → 次选通道尝试一次。
2. 所有通道失败 → 返回错误 + 人类可读的故障原因 + 建议命令 (如 "以管理员运行" / "先停止 Lenovo Vantage")。
3. 读失败但写成功 → 返回写成功,标注 `readback: false`。
4. Linux 上无 `AcpiVpc` 等效 → 若 `/sys/class/thermal/` 与 `ideapad_laptop` 也不可用,返回 `ENOTSUP` 而不是静默。

---

## 10. 证据出处

| 声明 | 证据 |
|---|---|
| ITS/Dispatcher 枚举与 `SERVICE_CONTROL_*` 常量 | `Vantage 电源组件内部接口说明`; `PowerBattery.dll` 导出符号 |
| 注册表 `PowerSlider` 键树 | `Vantage 电源组件内部接口说明` 关键常量段 |
| Dispatcher V2/V3/V4 阈值 | `Vantage 电源组件内部接口说明` 版本能力判定表 |
| 轮询 50ms × 10 次 | `Vantage 电源组件内部接口说明` |
| `LENOVO_GAMEZONE_DATA` 全方法集 (40+) | `目标机 WMI 仓库实机采集` `LENOVO_GAMEZONE_DATA` (行 126-236) |
| `LENOVO_FAN_TABLE_DATA` 结构 | `目标机 WMI 仓库实机采集` 行 314-333 |
| `Fan_Get_Table` / `Fan_Set_Table` 签名 | `目标机 WMI 仓库实机采集` 行 334-345 |
| `LENOVO_FAN_TEST_DATA` / `LENOVO_FAN_MAX_SPEED_DATA` | `目标机 WMI 仓库实机采集` 行 353-360, 445-451 |
| 风扇策略 WMI 方法全表 | `电脑管家电源组件内部接口说明` |
| 直读 `\\.\SIO8786` | `电脑管家电源组件内部接口说明`;GameSettingsPlugin 组件字符串资源 |
| `LENOVO_REPORT_DBDC_DATA` 结构 | `目标机 WMI 仓库实机采集` 行 346-352 |
| DBDC 实测数据 | `目标机 WMI 实例实机采集`; `电脑管家电源组件内部接口说明` |
| CPU OC 数据类 | `目标机 WMI 仓库实机采集` 行 553-567, 642-653 |
| GPU OC 数据类 | `目标机 WMI 仓库实机采集` 行 457-472, 665-678 |
| 内存 OC 数据类 | `目标机 WMI 仓库实机采集` 行 587-609 |
| 内存 OC 方法 | `目标机 WMI 仓库实机采集` 行 679-689 |
| DTT 硬件实例 | `目标机 MagicBay/DPTF 实机枚举数据`: `ipf_acpi` on `INTC10D5/10D8/INTC10D4` |
| PCManager `_DT` 方法族 | `电脑管家电源组件内部接口说明` |
| 节能不直写 DTT | `电脑管家电源组件内部接口说明` |
| ResScheduler 进程调度 API | `电脑管家电源组件内部接口说明`;ResScheduler 组件字符串资源 |
| ResScheduler IPC 消息 | `电脑管家电源组件内部接口说明` |
| 未使用 EcoQoS | `电脑管家电源组件内部接口说明` |
| Power API 覆盖 GUID 域 | `电脑管家电源组件内部接口说明 (A)` |
| 模式参数表 | `电脑管家电源组件内部接口说明` |
| WMI 事件类 (14 个) | `目标机 WMI 仓库实机采集` 行 1-125 |
| 版本判定阈值 | `Vantage 电源组件内部接口说明` |

**[推断] 条目**:
1. `Fan_Flag` 单位标志的取值语义 (§3.1)。
2. `Sensor_ID` 中 0=CPU, 1=GPU 的编号约定 (§3.3)。
3. PL1/PL2 WMI 方法 `GetCurrentPowerLimit` / `SetPowerLimit` 的具体签名 (§5.2)。
4. `SetFeatureValue` 中 DBDC 的 `IDs` 常量值 (§5.3)。
5. `LENOVO_SMART_THERMAL_MONITOR_EVENT` 的 `Status` 分级 (§8.3)。
6. 各模式的具体 CPU PL1 / PL2 / GPU TDP 数值 (§7.4 参数表)。
7. 全速模式下 `SetODStatus(1)` 的语义与是否可行 (§3.5)。
