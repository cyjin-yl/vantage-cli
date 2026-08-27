# 01 · 硬件抽象层接口规范 (HAL Interfaces)

> 读者:**Windows 后端实现者**(用 Rust 写跨平台 CLI 的硬件通道层)。
> 前置:`00-cleanroom-charter.md`(术语、文档地图)。全量 WMI 类签名见附录 A。
> 状态:v1 · 2026-08-27 · 目标机 Lenovo ThinkBook 21VG (Panther Lake)。

---

## 0. 一页速查

| 通道 | 用途 | 首选? | 特权要求 | 服务依赖 |
|---|---|---|---|---|
| `root\WMI` (LENOVO_* 方法类) | 性能/电池/风扇/背光/面板/BIOS/超频 | ✅ 首选 | 管理员或 SYSTEM | AcpiVpc.sys 加载即可;不需要联想服务 |
| `root\WMI` (LENOVO_* 事件类) | 热键/电源/场景/状态回调 | ✅ 首选(被动订阅) | 一般用户即可 | LnvEvtProvider.dll / ACPI notify |
| `\\.\EnergyDrv` (AcpiVpc.sys) | 直连 EC 电池、快充、适配器、USB 充电、DGPU 状态 | ⚠️ 次选,特定功能必须 | 管理员(SD 允许 SYSTEM/BA) | AcpiVpc.sys 服务必须启动 |
| `\\.\pipe\ipfsrv.public` | Panther Lake DPTF/EPP/TGP 策略切换 | ⚠️ 调优场景必须 | 管理员 | `ipfsvc.exe` 服务 |
| `\\.\pipe\LenovoDispatcher.*` [推断] | 借用 LNVDispatcher 的调度/DBDC 能力 | ⚠️ 备选 | 管理员 | `LNVDispatcherService.exe` |
| 命名端口(FltMgr) `LnvDrvFdn` | 与签名校验过滤驱动通信 | ❌ 无需对接 | — | — |
| `lenovoDriverBus` 虚拟 PDO | 供第三方驱动枚举的虚拟设备 | ❌ 无需直接对话 | — | — |
| 用户态 MSR (`LnvMSRIO.sys`) | CPU EPP / DYTC | ⚠️ 备选 | 管理员 | `LNVDispatcherService.exe` |
| Linux 侧:sysfs/ACPI/RAPL | 全部等价能力(见 09 文档) | — | root | — |

**决策铁律(实现者必读)**:

1. **凡是能在 `root\WMI` 上通过方法调用完成的读写,一律优先走 WMI**。WMI 方法由 `VPC2004` ACPI 固件直接响应,不依赖任何联想服务(LNVDispatcher/LenovoUtilityService/ipfsvc 都可卸载),跨用户会话、跨 SID 均能生效。
2. **只有当需要 `GBMD`(battery guard)/`GAPD`(adapter)/USB 充电/DGPU 状态查询**等 `LENOVO_*` 未暴露的能力时,才必须打开 `\\.\EnergyDrv` 下发 `0x831020xx` 直驱 IOCTL。
3. **Panther Lake 的 Panther Lake 特有调优(EPP、TGP、DPTF 策略切换)**必须走 `ipfsvc.exe` 的 named pipe,或者通过 `LENOVO_CPU_METHOD`/`Lenovo_BiosSetting` 间接下发。
4. **LNVDispatcher/LenovoUtilityService 属于"可选的便利层"**—— 直连 WMI + 直驱 IOCTL 已能覆盖 90% 用户可见功能;只有需要订阅热键事件流、AI 场景、OSD 时,才需要这些服务活着。
5. **lenovoDriverBus / LnvDrvFdn 是内部辅助层,实现者永远不应直接对话**;它们的存在是 PnP 枚举和签名校验,不影响功能路径。

---

## 1. 通道总览与选择策略

### 1.1 五层栈

```
┌─ 用户态 CLI (lctrl) ──────────────────────────────────────────┐
│                                                                │
│   优先调用 WMI 方法类 (LENOVO_*_METHOD / LENOVO_*_DATA)        │
│   优先订阅 WMI 事件类 (LENOVO_*_EVENT)                         │
│                                                                │
├────────────────────────────────────────────────────────────────┤
│  Windows 内核 / 用户态服务层                                    │
│                                                                │
│   WMI Provider (LnvEvtProvider.dll + ACPI _WMI)                │
│   AcpiVpc.sys (EnergyDrv) ─ 直驱 IOCTL 通道                    │
│   ipfsvc.exe ── Named Pipe `\\.\pipe\ipfsrv.public`            │
│   LNVDispatcherService.exe ── Named Pipe / WMI / MSR           │
│   LenovoUtilityService.exe ── WMI 路由 + 前台热键代理          │
│   LnvDrvFdn.sys ── 签名校验,不影响功能                         │
│   lenovoDriverBus.sys ── 虚拟总线 PnP 枚举,不影响功能           │
│                                                                │
├────────────────────────────────────────────────────────────────┤
│  ACPI 固件层 (DSDT / SSDT)                                     │
│   ACPI\VEN_VPC&DEV_2004 (VPC2004) ─ _WMI / _BST / _BIF / _BIX │
│   ACPI\ACPI000E (EC) ─ Region(EC) 读写                        │
│   ACPI\ACPI0008 (EmbeddedController)                           │
│                                                                │
└────────────────────────────────────────────────────────────────┘
```

### 1.2 何时走哪条通道

| 功能族 | 通道 | 原因 |
|---|---|---|
| 性能模式切换 (静音/均衡/性能/极致) | `LENOVO_GAMEZONE_DATA.SetFanCooling` / `SetBIOSOC` / `SetODStatus` | WMI 直抵 EC |
| 风扇曲线 / 智能风扇模式 | `LENOVO_FAN_METHOD.Fan_Get_Table` / `Fan_Set_Table` | 结构化数据只有 WMI 提供 |
| CPU/GPU/内存超频读取与设置 | `LENOVO_CPU_METHOD` / `LENOVO_GPU_OVERCLOCKING_DATA` / `LENOVO_MEMORY_METHOD` | 全在 WMI |
| 电池信息、充电阈值、快充、保养 | `\\.\EnergyDrv` `0x831020f8` (GBMD 相关)+ `SetFeatureValue` 组合;充电模式也可 `LENOVO_GAMEZONE_DATA.GetPowerChargeMode` | 快充打开/关闭必须直驱 |
| 适配器识别 (65W/100W/USB 充电) | `\\.\EnergyDrv` `IOCTL_PMDRV_GAPD` 宏(具体码待查) | 官方文档未走 WMI |
| USB 充电开关 | `\\.\EnergyDrv` 直驱 [推断:复用 GBMD 通道] | 无 WMI 方法 |
| 键盘背光 | `LENOVO_LIGHTING_METHOD.Set_Lighting_Current_Status` | 清晰 WMI 语义 |
| 面板刷新率/色彩/游戏辅助 | `LENOVO_PANEL_METHOD.*` + `LENOVO_INTERNAL_PANEL_REFRESH_RATE_DATA` | WMI 完整支持 |
| BIOS 设置读写 | `Lenovo_BiosSetting` (读) / `Lenovo_SetBiosSetting` (写) / `Lenovo_SaveBiosSettings` / `Lenovo_GetBiosSelections` / `Lenovo_BiosPasswordSettings` 等 | 标准 Lenovo BIOS WMI |
| 智能感应 (Smart Sense) | `LENOVO_SMART_THERMAL_MONITOR_EVENT` 订阅 + UMDF `LnvSST.dll` [推断] | 事件驱动 |
| Panther Lake EPP/TGP/DPTF 策略 | `\\.\pipe\ipfsrv.public` → ESIF_DATA_XML;或 `Lenovo_BiosSetting` 的 `EffectivePowerMode*` key | 走 DPTF 通道 |
| 热键事件捕获 | `LENOVO_UTILITY_EVENT` WMI 订阅 | 无需联想服务也可收到(ACPI 直推) |
| AI 场景切换 | `LENOVO_AI_SCENARIO_TYPE_EVENT` 订阅 + `LENOVO_REPORT_STATUS_TO_DISPATCHER_EVENT` 上报 | 订阅即可 |

### 1.3 通道优先级(伪代码)

```rust
enum Channel { Wmi, EnergyDrvIoctl, IpfPipe, DispatcherPipe }

fn pick_channel(feature: Feature) -> Channel {
    // 规则 1: WMI 方法类覆盖 → 走 WMI
    if wmi_has_method(feature) { return Channel::Wmi; }
    // 规则 2: Panther Lake DPTF/EPP/TGP → 走 ipfsvc
    if feature.requires_dptf() { return Channel::IpfPipe; }
    // 规则 3: 电池快充 / 适配器 / USB 充电 / DGPU 状态 → 直驱 EnergyDrv
    if feature.is_ec_direct_only() { return Channel::EnergyDrvIoctl; }
    // 规则 4: 需要前台 OSQ 或 AI 场景 → 走 Dispatcher pipe (可选)
    Channel::DispatcherPipe
}
```

---

## 2. `root\WMI` 接口全表

### 2.1 通用约定

- 命名空间:`root\WMI`(所有 `LENOVO_*` 与 `Lenovo_*` 类)。
- **实例方法(实机确认)**:`LENOVO_UTILITY_DATA`、`LENOVO_OTHER_METHOD` 等类的方法 `static=False`,
  必须**绑定实例调用**(`Get-CimInstance … | Invoke-CimMethod` / `ManagementObject.InvokeMethod`)。
  静态调用(类级 `Invoke-CimMethod -ClassName`)一律报"无效的方法参数"。
  实机:`LENOVO_UTILITY_DATA.GetIfSupportOrVersion(datatype)` 返回 `Data`=版本号(0=不支持);
  本机支持表:datatype 1→v3, 3→v2, 4→v2, 其余 0..10 为 0;已知 10=DolbyAudio,18=PrecisionTouchpad(Data≥24 即支持)。
- 实例命名:大多数方法类有 `Active:bool` 与 `InstanceName:string` 两个属性;`InstanceName` 形如 `ACPI\PNP0C14\GMZN_0`、`ACPI\VEN_VPC&DEV_2004\...`。查询用:
  ```
  SELECT * FROM LENOVO_GAMEZONE_DATA WHERE Active = true
  ```
- 方法返回:**全部方法签名均为 `Boolean` 返回值**(WMI 层语义:方法是否被固件/驱动接受),真正的成功/失败状态编码在 `[out] Data` 字段里:
  - `Boolean = true` 且 `Data == 0` → 操作成功。
  - `Boolean = true` 且 `Data != 0` → 方法执行了但返回了 Lenovo 私有状态码(需按功能族查表)。
  - `Boolean = false` → 固件/驱动拒绝(可能是功能不支持、权限不足、参数越界)。
- 参数 `[Description,ID,in/out] Data : UInt32` 里的 `Data` 是**无类型的 32 位整数**;同一方法在不同 `IDs` 下语义不同。**`IDs` 是主索引**——通过 `LENOVO_CAPABILITY_DATA_00/01/02` 或 `LENOVO_DISCRETE_DATA` 反查具体功能 ID。
- 事件类基类字段(所有 `*_EVENT` 都带):
  ```
  SECURITY_DESCRIPTOR : UInt8Array    // WMI 基础设施字段
  TIME_CREATED        : UInt64        // 100 ns 粒度文件时间
  Active              : Boolean
  InstanceName        : String
  ```
- 事件订阅:用 `__EventFilter` + `__EventConsumer` + `__FilterToConsumerBinding` 三条 WMI 关联对象,或直接用 `IWbemServices::ExecNotificationQueryAsync("root\\WMI", "WQL", "SELECT * FROM LENOVO_UTILITY_EVENT")`。推荐后者(实现简单)。

### 2.2 `LENOVO_GAMEZONE_DATA` — GameZone / 性能 / 风扇 / 温度 / 超频 / 背光

> 主数据类;实例名通常为 `ACPI\VEN_VPC&DEV_2004\GMZN_0`。包含 40+ 方法。

| 方法 | 参数 | 语义 | `Data` 常见取值 |
|---|---|---|---|
| `IsSupportGpuOC -> Boolean` | `[out] Data` | 是否支持 GPU 超频 | `1` 支持 / `0` 不支持 |
| `IsSupportCpuOC -> Boolean` | `[out] Data` | 是否支持 CPU 超频 | 同上 |
| `IsBIOSSupportOC -> Boolean` | `[out] Data` | BIOS 是否允许 OC | 同上 |
| `SetBIOSOC -> Boolean` | `[in] Data` | 切换 OC BIOS 开关 | `1` 开 / `0` 关 |
| `GetVersion -> Boolean` | `[out] Data` | GameZone 驱动版本 | 版本整数 |
| `IsSupportFanCooling -> Boolean` | `[out] Data` | 是否支持风扇冷却切换 | 同上 |
| `SetFanCooling -> Boolean` | `[in] Data` | 风扇冷却模式 | `1`=高性能 / `2`=静音 [推断] |
| `GetFanCoolingStatus -> Boolean` | `[out] Data` | 当前风扇冷却模式 | 同上 |
| `GetCPUTemp -> Boolean` | `[out] Data` | CPU 温度 (°C) | 数值 |
| `GetGPUTemp -> Boolean` | `[out] Data` | GPU 温度 (°C) | 数值 |
| `IsSupportDisableWinKey -> Boolean` | `[out] Data` | 是否支持禁用 Win 键 | 同上 |
| `SetWinKeyStatus -> Boolean` | `[in] Data` | Win 键状态 | `1`=禁用 / `0`=启用 |
| `GetWinKeyStatus -> Boolean` | `[out] Data` | 当前 Win 键状态 | 同上 |
| `IsSupportDisableTP -> Boolean` | `[out] Data` | 是否支持禁用触控板 | 同上 |
| `SetTPStatus -> Boolean` | `[in] Data` | 触控板开关 | `1`=禁用 / `0`=启用 |
| `GetTPStatus -> Boolean` | `[out] Data` | 触控板状态 | 同上 |
| `GetKeyboardfeaturelist -> Boolean` | `[out] Data` | 键盘特性位图 | 位图 |
| `GetMemoryOCInfo -> Boolean` | `[out] Data` | 内存 OC 支持信息 | 位图 [推断] |
| `IsSupportWaterCooling -> Boolean` | `[out] Data` | 是否支持水冷 (Legion 特有) | 同上 |
| `SetWaterCoolingStatus -> Boolean` | `[in] Data` | 水冷泵开关 | `1`/`0` |
| `IsSupportLightingFeature -> Boolean` | `[out] Data` | 是否支持背光 | 同上 |
| `SetKeyboardLight -> Boolean` | `[in] Data` | 键盘背光模式/亮度 | 见 `LENOVO_LIGHTING_DATA` |
| `GetKeyboardLight -> Boolean` | `[out] Data` | 当前背光 | 同上 |
| `GetMacrokeyScancode -> Boolean` | `[in] idx [out] scancode` | 宏键 scancode | 数值 |
| `GetMacrokeyCount -> Boolean` | `[out] Data` | 宏键数量 | 数值 |
| `IsSupportGSync -> Boolean` | `[out] Data` | G-Sync 支持 | 同上 |
| `SetGSyncStatus -> Boolean` | `[in] Data` | G-Sync 开关 | `1`/`0` |
| `GetGSyncStatus -> Boolean` | `[out] Data` | G-Sync 状态 | `1`/`0` |
| `IsSupportSmartFan -> Boolean` | `[out] Data` | 智能风扇支持 | 同上 |
| `SetSmartFanMode -> Boolean` | `[in] Data` | 智能风扇模式 | `1`=静音 / `2`=均衡 / `3`=性能 / `4`=极致 [推断] |
| `GetSmartFanMode -> Boolean` | `[out] Data` | 当前智能风扇模式 | 同上 |
| `GetSmartFanSetting -> Boolean` | `[out] Data` | 风扇设置 (位图) | 位图 [推断] |
| `GetPowerChargeMode -> Boolean` | `[out] Data` | 充电模式 | `1`=标准 / `2`=快速 / `3`=保养 [推断] |
| `GetProductInfo -> Boolean` | `[out] Data` | 产品信息 | 位图 |
| `IsSupportOD -> Boolean` | `[out] Data` | OverDrive 支持 | 同上 |
| `SetODStatus -> Boolean` | `[in] Data` | OverDrive 开关 | `1`/`0` |
| `GetODStatus -> Boolean` | `[out] Data` | OverDrive 状态 | `1`/`0` |
| `SetLightControlOwner -> Boolean` | `[in] Data` | 背光控制权切换(多应用) | 见 04 文档 |
| `SetDDSControlOwner -> Boolean` | `[in] Data` | DDS(显示色彩)控制权 | 同上 |
| `IsRestoreOCValue -> Boolean` | `[in] idx [out] Data` | OC 值是否可恢复 | 同上 |
| `GetThermalMode -> Boolean` | `[out] Data` | 热模式 (静音/均衡/性能/极致) | `1`/`2`/`3`/`4` [推断] |
| `GetBIOSOCMode -> Boolean` | `[out] Data` | BIOS OC 模式 | 数值 |
| `GetHardwareInfoSupportVersion -> Boolean` | `[out] Data` | 硬件信息支持版本 | 数值 |
| `GetCpuFrequency -> Boolean` | `[out] Data` | 当前 CPU 频率 (MHz) | 数值 |
| `IsACFitForOC -> Boolean` | `[out] Data` | AC 适配器是否支持 OC | `1`=是 |
| `IsSupportIGPUMode -> Boolean` | `[out] Data` | 独立/集显切换支持 | 同上 |
| `GetIGPUModeStatus -> Boolean` | `[out] Data` | 当前 iGPU 模式 | 数值 |
| `SetIGPUModeStatus -> Boolean` | `[in] mode [out] Data` | 切 iGPU 模式 | 见 `LENOVO_FEATURE_STATUS_DATA` |
| `NotifyDGPUStatus -> Boolean` | `[in] status [out] Data` | 上报 dGPU 状态 | 状态值 |
| `IsChangedYLog -> Boolean` | `[out] Data` | Y 日志是否变更 [推断:内部调试] | 同上 |
| `GetDGPUHWId -> Boolean` | `[out] Data:String` | dGPU 硬件 ID 字符串 | 字符串 |

### 2.3 `LENOVO_OTHER_METHOD` — 通用特性读写

**通用万能接口,几乎所有 Lenovo 内部能力都可以经此通道访问。** `IDs` 是 32 位功能码,通过 `LENOVO_CAPABILITY_DATA_00/01/02` 或 `LENOVO_DISCRETE_DATA` 反查。

| 方法 | 签名 | 语义 |
|---|---|---|
| `GetFeatureValue -> Boolean` | `[in] IDs:u32 [out] value:u32` | 读取某功能当前值 |
| `SetFeatureValue -> Boolean` | `[in] IDs:u32 [in] value:u32` | 写入某功能值 |
| `GetDataByCommand -> Boolean` | `[in] Command:u32 [in] IDs:u32 [out] Data:u32[] WmiSizeIs DataSize [out] DataSize:u32` | 命令式批量读取 |
| `GetDataByPackage -> Boolean` | `[in,Max] Input:u8[] [out,WmiSizeIs] Data:u8[] [out] DataSize:u32` | 包式读写 (二进制协议) |

> **实现者注意**: `GetDataByPackage` 是 Lenovo 内部最常用的通道;`Input` 是自定义二进制协议,格式未知 [推断:头 4 字节魔数 + 字段],需要具体功能文档给出 payload 结构。优先用 `GetFeatureValue`/`SetFeatureValue`(单值语义清晰)。

### 2.4 `LENOVO_UTILITY_DATA` — 通用杂项功能

| 方法 | 签名 | 语义 |
|---|---|---|
| `GetIfSupportOrVersion -> Boolean` | `[in] datatype:u32 [out] Data:u32` | 查询某功能是否支持 / 版本 |
| `SetFeature -> Boolean` | `[in] featuretype:u32 [out] Data:u32` | 切换某功能开关 |
| `SetFeatureEx -> Boolean` | `[in] IDs:u32 [in] Value:u32 [out] Ret:u32` | 带 ID 的写操作,`Ret` 是状态码 |

已知 `datatype` 取值(样本中通过 `GetIfSupportOrVersion` 观察到,具体语义见 `ludp.dll` [推断]):

| datatype | Data 返回值 | 推断语义 |
|---|---|---|
| `1`  | `3`  | 键盘背光三档 (0/1/2 + 关) |
| `18` | `24` | 未确认 [推断:热键相关] |
| `19` | `25` | 未确认 [推断:热键相关] |
| `26` | `27` | 未确认 [推断:面板相关] |
| `29` | `32` | 未确认 [推断:音频相关] |
| `38` | `36` | 未确认 [推断:蓝牙相关] |

### 2.5 `LENOVO_FAN_METHOD` / `LENOVO_FAN_TABLE_DATA` / `LENOVO_FAN_TEST_DATA` / `LENOVO_FAN_MAX_SPEED_DATA` — 风扇

| 类 | 关键字段/方法 |
|---|---|
| `LENOVO_FAN_METHOD.Fan_Get_Table` | `[in] FanID:u8 [in] SensorID:u8 [out] FanTable:u32[](WmiSizeIs FanTableSize) [out] FanTableSize:u32 [out] SensorTable:u32[](WmiSizeIs) [out] SensorTableSize:u32` |
| `LENOVO_FAN_METHOD.Fan_Set_Table` | `[in,Max] FanTable:u8[]` — 自定义二进制格式的风扇曲线表 |
| `LENOVO_FAN_TABLE_DATA` | `Fan_Id, FanSpeedStep, FanTable_Data(u16[]), FanTable_Len, Sensor_ID, SensorTable_Data(u16[]), SensorTable_Len, SensorTemperatureStep, MaxSensorTemperature, MinSensorTemperature, CurrentFanMaxSpeed, CurrentFanMinSpeed, DesignMaxFanSpeedNumber, Mode, Start/EndOnlyUpwardAdjustNumber` |
| `LENOVO_FAN_TEST_DATA` | `NumOfFans, FanId[], FanMaxSpeed[], FanMinSpeed[]` — 风扇拓扑 |
| `LENOVO_FAN_MAX_SPEED_DATA` | `Fan_Id, Fan_CurrentMaxSpeed, Fan_DefaultMaxSpeed, Fan_Flag` — 单风扇最大速度 |

> 风扇曲线表(`FanTable_Data`)是温度-转速查表:长度 `FanTable_Len`,每行含 (SensorTemperature, FanSpeed);步长 `SensorTemperatureStep` / `FanSpeedStep`。

### 2.6 `LENOVO_PANEL_METHOD` — 面板/显示

| 方法 | 参数 | 语义 |
|---|---|---|
| `Panel_Get_Support_Status` | `[out] Support_Status` | 面板支持能力 |
| `Panel_Get_Status` | `[out] Status` | 当前面板状态 |
| `Panel_Set_Status` | `[in] Status` | 设置面板状态 (开/关/切换) |
| `Panel_Get_Low_Latency_Mode` | `[out] mode` | 低延迟模式 |
| `Panel_Set_Low_Latency_Mode` | `[in] mode` | 设置低延迟模式 |
| `Panel_Get_PIP_Info` | `[out] PosX,PosY,SizeX,SizeY` | 画中画 |
| `Panel_Set_PIP_Info` | `[in] ...` | 设置画中画 |
| `Panel_Get_Game_Aid_FPS_Display_Pos` | `[out] PosX,PosY` | 游戏辅助 FPS 位置 |
| `Panel_Set_Game_Aid_FPS_Display_Pos` | `[in] PosX,PosY` | 同上 |
| `Panel_Get_Game_Aid_FPS` | `[out] AvgFPS,CurrentFPS,MaxFPS,MinFPS` | FPS 显示 |
| `Panel_Get_Game_Aid_Sight_Mode` | `[out] mode` | 瞄准辅助 |
| `Panel_Set_Game_Aid_Sight_Mode` | `[in] mode` | 同上 |
| `Panel_Get_Game_Aid_Timer_Info` | `[out] Clear,End,Start` | 计时器 |
| `Panel_Set_Game_Aid_Timer_Info` | `[in] ...` | 同上 |
| `Panel_Get_Game_Aid_Countdown_Info` | `[out] time` | 倒计时 |
| `Panel_Set_Game_Aid_Countdown_Info` | `[in] time` | 同上 |
| `Panel_Get_Display_Mode` | `[out] mode` | 显示模式 (sRGB / DCI-P3 / Adobe RGB) [推断] |
| `Panel_Set_Display_Mode` | `[in] mode` | 同上 |
| `Panel_Get_Gamut_Switch` | `[out] mode` | 色域切换 |
| `Panel_Set_Gamut_Switch` | `[in] mode` | 同上 |
| `Panel_Get_MPRT` | `[out] PosX,PosY,SizeX,SizeY` | MPRT 影闭 |
| `Panel_Set_MPRT` | `[in] ...` | 同上 |

`LENOVO_INTERNAL_PANEL_REFRESH_RATE_DATA`:只读属性类,含 `MinimumRefreshRate`, `MaximumRefreshRate`, `DefaultRefreshRate`, `Mode`, `InternalPanelHwID`。

### 2.7 `LENOVO_CPU_METHOD` / `LENOVO_GPU_METHOD` / `LENOVO_MEMORY_METHOD` / 超频数据类

| 方法 | 参数 | 语义 |
|---|---|---|
| `LENOVO_CPU_METHOD.CPU_Set_OC_Data -> Boolean` | `[in] mode:u8 [in] TuneID:u32 [in] value:u32` | 写 CPU 超频参数 |
| `LENOVO_MEMORY_METHOD.MEM_Get_OC_Status` | `[in] mode:u8 [out] Status:u8` | 读内存 OC 状态 |
| `LENOVO_MEMORY_METHOD.MEM_Set_OC_Status` | `[in] mode:u8 [in] Status:u8` | 写内存 OC 状态 |
| `LENOVO_MEMORY_METHOD.MEM_Set_OC_Data` | `[in,Max] MEM_OCData:u8[]` | 二进制超频参数包 |

**数据类**(`LENOVO_CPU_OVERCLOCKING_DATA` / `LENOVO_GPU_OVERCLOCKING_DATA` / `LENOVO_MEMORY_OC_DATA` / `LENOVO_GAMEZONE_CPU_OC_DATA` / `LENOVO_GAMEZONE_GPU_OC_DATA`):全部为只读属性类,提供能力声明:

- 公共字段:`Capability, DefaultValue, Interval, MaxValue, MinValue, ScaleValue, OCMinOffset, OCMaxOffset, OCOffsetFreq, OCOffsetScale, Tuneid, OCOrderid, NOCOrderid, PStateID`。
- `LENOVO_MEMORY_OC_DATA` 特化:`MEM_OC_Ability, MEM_OC_XMP_Numbers, MEM_OC_*_Customize_*`(VDD/tCL/tRAS/tRCD/...共 13 个时序字段,`UInt16` 单位)。

### 2.8 `LENOVO_LIGHTING_METHOD` / `LENOVO_LIGHTING_DATA` — 键盘背光

| 方法 | 参数 | 语义 |
|---|---|---|
| `Get_Lighting_Current_Status` | `[in] Lighting_ID:u8 [out] Current_Brightness_Level:u8 [out] Current_State_Type:u8` | 读背光 |
| `Set_Lighting_Current_Status` | `[in] Current_Brightness_Level:u8 [in] Current_State_Type:u8 [in] Lighting_ID:u8` | 写背光 |

`LENOVO_LIGHTING_DATA`:能力声明(`Brightness_Level, Control_Interface, Default_Brightness_Level, Default_State, Lighting_Id, Lighting_Type, State_Type_Num`)。

- `Lighting_Type = 1` 推断 = 键盘背光;`Lighting_Id` 是设备索引(多数机型只有 0)。
- `Current_State_Type = 0` 关 / `1..N-1` 亮度档 / `N` 呼吸 / `N+1` 波等特效 [推断]。
- `Current_Brightness_Level` 只在亮度档生效,取值 `0..Brightness_Level`。

### 2.9 `LENOVO_SR_DATA` — 系统恢复 / 智能上报

| 方法 | 参数 | 语义 |
|---|---|---|
| `GetDataValue` | `[in] datatype [out] Data` | 读取 EC 监控数据 |
| `StartECMonitor` | `[in] datatype [in] value [out] ret` | 开启 EC 传感器监控 |
| `StopECMonitor` | `[in] datatype [in] value [out] ret` | 关闭 EC 传感器监控 |
| `GetCapability` | `[in] datatype [out] Data` | 能力查询 |

### 2.10 `LENOVO_BIOS_ASSISTANT` — 辅助 BIOS 设置(比标准 WMI 更底层)

| 方法 | 参数 | 语义 |
|---|---|---|
| `GetCapabilityValue` | `[out] Data` | 能力标志 |
| `GetValue` | `[in] IndexData [out] Data` | 按索引读 |
| `SetValue` | `[in] IndexData [in] ValueData [out] ReturnData` | 按索引写,`ReturnData` 是状态码 |

### 2.11 标准 `Lenovo_*` BIOS 类

| 类 | 关键方法/字段 |
|---|---|
| `Lenovo_BiosSetting` | 属性类:`CurrentSetting: String`(形如 `CurrentValue=On`) |
| `Lenovo_SetBiosSetting` | `SetBiosSetting(parameter:String) -> (return:String)` |
| `Lenovo_SaveBiosSettings` | `SaveBiosSettings(parameter:String) -> (return:String)` — 保存并重启生效 |
| `Lenovo_DiscardBiosSettings` | 丢弃未保存的更改 |
| `Lenovo_LoadDefaultSettings` | 加载默认设置 |
| `Lenovo_SetFunctionRequest` | `SetFunctionRequest(parameter) -> (return)` — 功能请求 (重启 BIOS 等) |
| `Lenovo_GetBiosSelections` | `GetBiosSelections(Item:String) -> (Selections:String)` — 枚举某项的可选值 |
| `Lenovo_BiosPasswordSettings` | 密码设置能力声明 |
| `Lenovo_SetBiosPassword` | 密码设置 |
| `Lenovo_AssetTag` / `Lenovo_AssetTagWrite` | 资产标签读写 |

> 这些类的 `Active=true` 实例位于 `root\WMI` 命名空间下,通常只有一个实例;`CurrentSetting` 字符串解析:以 `=` 分割,左侧为键,右侧为值。

### 2.12 事件类 (`*_EVENT`) 全表

| 事件类 | 特有字段 | 触发条件 | 主要订阅方(官方) | 实现者用法 |
|---|---|---|---|---|
| `LENOVO_DISPATCHER_EVENT` | `PowerLevel:u32` | 电源模式 / AC 切换 / BIOS 策略切换 | LNVDispatcherService | 订阅 → 知道当前性能档位 |
| `LENOVO_UTILITY_EVENT` | `PressTypeDataVal:u32` | Fn 组合键按下 | LenovoUtilityService | **实现者必须订阅** → 触发自定义热键映射 |
| `LENOVO_GAMEZONE_SMART_FAN_MODE_EVENT` | `mode, version` | 智能风扇模式切换 | Vantage GameZone | 同步 UI |
| `LENOVO_GAMEZONE_SMART_FAN_SETTING_EVENT` | `mode` | 用户保存风扇曲线 | Vantage | 同步 UI |
| `LENOVO_GAMEZONE_KEYLOCK_STATUS_EVENT` | `KeyLockState:u32` | WinKey/TP 锁定状态 | Vantage | 同步 UI |
| `LENOVO_GAMEZONE_THERMAL_MODE_EVENT` | `mode` | 热模式切换 | Vantage / ipfsvc | 同步 UI |
| `LENOVO_GAMEZONE_FAN_COOLING_EVENT` | `EventId` | 风扇档位 | Vantage | 同步 UI |
| `LENOVO_GAMEZONE_POWER_CHARGE_MODE_EVENT` | `mode` | 充电模式切换 | Vantage | 同步 UI |
| `LENOVO_GAMEZONE_LIGHT_PROFILE_CHANGE_EVENT` | `EventId` | 背光主题切换 | UtilityService | 同步 UI |
| `LENOVO_AI_SCENARIO_TYPE_EVENT` | `Type:u32` | AI 场景判定 | LNVDispatcher | 可订阅实现 AI 场景联动 |
| `LENOVO_AI_CHIP_EVENT` | `Status:u32` | NPU/VPU 状态 | LNVDispatcher / ipfsvc | 可选 |
| `LENOVO_REPORT_STATUS_TO_DISPATCHER_EVENT` | `Type, Value` | 下游回报 | LNVDispatcher | 可用 |
| `LENOVO_REPORT_DBDC_DATA` | `Counts, CurrentLimit[], Threshold[], ROS_Power[]` | DBDC 动态功率限制 | ipfsvc / LNVDispatcher | **实现者必须读** → 用于 Panther Lake 调优 |
| `LENOVO_REPORT_REFRESH_RATE_EVENT` | `MaxRefreshRate, MinRefreshRate` | 面板变频 | Vantage / ipfsvc | 同步 UI |
| `LENOVO_REPORT_POWER_CONSUMPTION_CHANGE_EVENT` | `ModeID[], NumbersOfMode, PowerConsumption[]` | 功耗档位变化 | ipfsvc / Vantage | Panther Lake 调优 |
| `LENOVO_REPORT_2D3D_STATUS_EVENT` | `Status` | dGPU 电源状态 | LNVDispatcher / ipfsvc | 可选 |
| `LENOVO_AC_PD_EVENT` | `AC_PD_Status:u16` | AC 适配器插入/拔出 | LNVDispatcher / ipfsvc | 必须订阅 → 触发性能/充电策略切换 |
| `LENOVO_BTKBD_EVENT` | `Status` | 蓝牙键盘连接/断开 | UtilityService | 可选 |
| `LENOVO_LIGHTING_EVENT` | `Key_ID:u8` | 背光键按下 | UtilityService | 可选 |
| `LENOVO_SR_EVENT` | `PostSystemStatus:u32` | 系统 POST 完成 | 系统恢复 | 可选 |
| `LENOVO_SMART_THERMAL_MONITOR_EVENT` | `Status:u32` | 环境/用户感知状态 | LNVDispatcher / ipfsvc | 可选 |

**事件订阅伪代码 (Rust, Windows 后端):**

```rust
// 使用 wmi crate 或 COM IWbemServices
let query = "SELECT * FROM LENOVO_UTILITY_EVENT";
let watcher = wmi_conn.create_event_watcher(query)?;
loop {
    let evt = watcher.next()?; // 阻塞直到下一个事件
    let press = evt["PressTypeDataVal"].clone::<u32>()?;
    dispatch_hotkey(press);
}
```

### 2.13 能力声明数据类

| 类 | 字段 | 用途 |
|---|---|---|
| `LENOVO_CAPABILITY_DATA_00` | `IDs, Capability, DefaultValue, InstanceName` | 简单能力(布尔/枚举) |
| `LENOVO_CAPABILITY_DATA_01` | 同上 + `MaxValue, MinValue, Step` | 带范围的能力 |
| `LENOVO_CAPABILITY_DATA_02` | `IDs, Capability, DataSize, DefaultValue:u8[]` | 二进制能力 |
| `LENOVO_DISCRETE_DATA` | `IDs, Value` | 离散值能力 |
| `LENOVO_FEATURE_STATUS_DATA` | `IDs, Status` | 特性状态快照 |
| `LENOVO_MACHINE_LEARNING_LIST` | `IDs, Capability, ListSize, mode[], processname[], ProfileCount` | ML 进程白名单 |
| `LENOVO_REPORT_DIRECT_BIOS_DATA` | `Name, Setup_ID, Option_num, Option_name[], Option_value[], Min_Value, Max_Value, Step, DependOnID, DependOnValue, RebootFlag, ...` | BIOS 项元数据 |

---

## 3. `AcpiVpc.sys` — 直驱 IOCTL 通道

### 3.1 驱动元信息

| 项 | 值 |
|---|---|
| 文件名 | `AcpiVpc.sys`(别名 `EnergyVpc.sys`),版本 `15.11.30.11` |
| 硬件 ID | `ACPI\VEN_VPC&DEV_2004` |
| 服务名 | 由 `AcpiVpc.inf` 安装,`SERVICE_KERNEL_DRIVER`, `SERVICE_DEMAND_START`, `SERVICE_ERROR_NORMAL` |
| 显示名 | "Lenovo Virtual Power Controller Driver" |
| 构建标签 | `GAMING_Driver-EM_release`（x64 release 构建） |
| 内部设备名 | `\Device\EnergyDrv` |
| 符号链接 | `\BaseNamedObjects\EnergyDrvEvent`(事件同步对象) |
| 用户态设备路径 | `\\.\EnergyDrv`(Windows API `CreateFileW` 用) |
| 自定义 DeviceType | `0x32`(非标准) |

### 3.2 用户态如何打开

```rust
use std::os::windows::io::AsRawHandle;
use winapi::um::winbase::{CREATE_FILE_FLAG_DELETE_ON_CLOSE, FILE_SHARE_READ, FILE_SHARE_WRITE};
use winapi::um::winnt::{FILE_GENERIC_READ, FILE_GENERIC_WRITE};

// 需要管理员(标准 ACL 只允许 SYSTEM / Administrators)
let handle = unsafe {
    CreateFileW(
        "Global\\EnergyDrv\0".as_ptr(),
        FILE_GENERIC_READ | FILE_GENERIC_WRITE,
        FILE_SHARE_READ | FILE_SHARE_WRITE,
        std::ptr::null_mut(),
        OPEN_EXISTING,
        0,
        std::ptr::null_mut(),
    )
};
if handle == INVALID_HANDLE_VALUE {
    let gle = GetLastError();
    return Err(IoctlError::OpenFailed { gle });
}
```

**设备路径别名**:
- `\\.\EnergyDrv`
- `Global\EnergyDrv`(推荐,跨会话)
- `Local\EnergyDrv`

### 3.3 IOCTL 码表

**重要区分**:驱动接受两类 IOCTL:

1. **内部 IOCTL `0x0032C004`** — 由驱动内部 `IofCallDriver` 向下方 ACPI HAL 转发,用户态**永不直接调用**;它由驱动自身在 `IoBuildDeviceIoControlRequest` 中构造。宏展开:
   ```
   CTL_CODE(0x32, 0x0C01, METHOD_BUFFERED, FILE_ANY_ACCESS|FILE_READ_DATA|FILE_WRITE_DATA)
     = (0x32 << 16) | (0x0C01 << 2) | (0 << 14) | (3 << 0)
     = 0x0032C004
   ```

2. **用户态直驱 IOCTL `0x831020xx` 族** — 由 `PowerBattery.dll` 通过 `CDriverLib::DeviceIoControl` 调用,驱动在 `DispatchDeviceControl` 里分发。这是**实现者唯一需要直接调用的通道**。

**已验证 `0x831020xx` 族代码**(2026-08-27 实机只读探测 + 组件行为分析交叉确认,全部经过验证):

| IOCTL | 名称 | 用途 | 输入 | 输出 | 证据 |
|---|---|---|---|---|---|
| `0x831020c0` | 通用 SET | 特性开关写入 | 12B `{cmd:u32, p1:u32, p2:u32}` | — | pcm-cli 组件行为分析,实机写入验证 |
| `0x831020c4` | 通用 GET | 特性状态/能力查询 | 4B `cmd:u32` | 4B 状态 | **实机扫描**:cmd 0-4→0, 5→0x10, 6/7/8→1, 9→err87, 10-15→1, 16/17→0, 18/19→err87, 20→0, 21→1, 22-24→err87;pcm-cli 用 cmd 14 bit0 判定 |
| `0x831020e8` | 通用 GET(变体) | 数值读取 | 4B `cmd:u32` | 4B | BatterySetting.exe 用 cmd 2/8/9;**实机**:cmd2→20160(0x4EC0),其余 cmd 回显输入 |
| `0x831020f8` | `GBMD` | 电池模式入口:子命令 1B;写: `3/5`=养护 on/off(gen1),`0x0d/0x0f`=养护(gen2),`7/8`=快充 on/off;查: `0xFF` | 1B 子命令 | 4B DWORD | Vantage 电源组件行为分析;**实机** `0xFF`→`0x00860004`(bit24=0 无认证充电器能力,bit15-16=0 Inbox 适配器);子命令 0/1/2 返回空 |
| `0x83102120` | 全局电池配置读 | 充电全局配置 | 4B 零 | 20B | WrapPlugin/pcm-cli 组件分析;**实机**→`B7 00…`(首字节 0xB7,位图语义[推断]) |
| `0x8310212c` | 查询 | 未知状态 | 4B(样本用 `0xffff`) | 4B | WrapPlugin;**实机**→`FF FF 00 00` |
| `0x83102130` | 查询(PCManager) | 电池模式/版本 DWORD | 4B | 4B | BatterySetting.exe;**实机** err=2(本机不支持) |
| `0x83102134` | 充电模式写(PCManager) | 充电模式切换 | 4B DWORD `{0,1,9}`(0=常规,1=养护,9=自定义阈值 [推断]) | 32B 结果位图 | BatterySetting 组件行为分析(写后按模式填充 0x01 位图) |
| `0x83102138` | **单电池信息读** | 83B 电池详情结构 | 4B 电池索引 | 83B | WrapPlugin 组件分析;**实机完整解析**见下表 |
| `0x8310213c` | 特性写(变体) | 模式写 | 4B `{0x101 或 1}` | 4B | WrapPlugin 组件行为分析(参数 1→0x101,3→1) |
| `0x83102150` | 查询 | 4B 状态 | 4B | 4B | pcm-cli/WrapPlugin 组件分析 |
| `0x8310215c` | `GAPD` | **适配器识别**(PID/VID/功率) | 4B 零 | 10B: `u16 PID, u16 VID, u16 SystemPowerW, u16 CurrentPowerW` | IdeaNotebookAddin AdapterInformation 接口规格;**实机** err=87(未接认证充电器,与 GBMD bit24=0 一致) |

`0x83102138` 83B 电池结构实机解析(Sunwoda 15.6V 电池):

| 偏移 | 类型 | 本机值 | 语义 |
|---|---|---|---|
| 0x00 | u16 | 9990 | 容量字段 1(设计容量,单位[推断]) |
| 0x02 | u16 | 9645 | 容量字段 2(满充容量[推断]) |
| 0x04 | u16 | 9645 | 容量字段 3(剩余/当前[推断]) |
| 0x06 | u32 | 0xFFFFFFFF | 缺省哨兵(字段不存在) |
| 0x0A | u16 | 17752 | [推断] 电池状态/告警 |
| 0x0E | u16 | 3061 | [推断] 温度(0.1K→33.4℃) |
| 0x10 | u16 | 23446 | [推断] 电流/功率(mA,充电) |
| 0x12 | u16 | 23749 | [推断] 电流/功率备用 |
| 0x14 | u16 | 15600 | **标称电压 mV**(15.6V,与 Lion 电池一致,锚定字段) |
| 0x16 | char[] | `"Lion"` | 化学类型 |
| 0x28 | char[] | `"Sunwoda"` | 制造商 |
| 0x34 | char[] | `"W1LX5CN0294"` | 序列号 |
| 尾 7B | — | `07 00 48 0C 00 01 09` | [推断] 循环计数/日期/阈值标志 |

> 双电池机型按索引 0/1 枚举(本机 idx0/idx1 同电池,idx2 全零=不存在)。

**错误模型**:`DeviceIoControl` 返回 FALSE 且 `GetLastError=87` → 该 cmd/功能本机不支持;`=2` → 驱动拒绝路径;支持但关闭 → 返回 0。**实现者必须用探测而非假设**。

**IOCTL 编码推导**:全部 `0x831020xx` 都是 `CTL_CODE(DeviceType=0x2032 [自定义], Function=0x7fxx, METHOD_BUFFERED, FILE_ANY_ACCESS)` 的变体(具体因版本而异)。实现者应直接以字面量常量使用,不重新推导。

### 3.4 `0x831020f8` (GBMD) 精确调用模式(实机验证)

**输入缓冲区是 1 字节子命令,不是 4 字节**(2026-08-27 实机 + Vantage 组件行为分析确认):

```rust
// 已验证模式:写入 1 字节子命令,读回 4 字节 DWORD
fn gbmd(handle: HANDLE, subcmd: u8) -> Result<u32, IoctlError> {
    let input = [subcmd];          // 1 字节
    let mut out = [0u8; 4];        // 4 字节
    // DeviceIoControl(handle, 0x831020f8, &input, 1, &mut out, 4, ...)
}
```

子命令表(Vantage 电源组件行为分析 + 实机探测):

| 子命令 | 方向 | 语义 | 证据 |
|---|---|---|---|
| `0xFF` | 查询 | 返回状态 DWORD;bit24=认证充电器能力,bit15-16=适配器类型(0=Inbox,1=Lenovo,2=Unknown,3=SlowCharger) | `AdapterInformation.cs`;实机→`0x00860004` |
| `3` / `5` | 写 | 养护(Conservation)开 / 关(gen1 机型) | PowerBattery 组件行为分析 |
| `0x0d` / `0x0f` | 写 | 养护开 / 关(gen2 机型) | 同上 |
| `7` / `8` | 写 | 快充(Express)开 / 关 | 同上 |
| `0` / `1` / `2` | — | 实机返回成功但 `bytesReturned=0`(非查询子命令,勿用于读) | 实机 |

**已知行为**:
- 调用失败时,官方日志:`"Open rapid charge failed, GLE=%d"` (从 `PowerBattery.dll` 字符串反查)。
- 快充开关是**双态**:open / close,而非连续阈值。
- 适配器详情在 GBMD bit24=1 时跟进 `GAPD`(`0x8310215c`)读取 PID/VID/功率;bit24=0 时 GAPD 返回 err=87(实机确认)。

### 3.5 与 WMI 的等价关系

| 功能 | WMI 方法 | 直驱 IOCTL | 何时选哪条 |
|---|---|---|---|
| 充电模式 (标准/快速/保养) | `LENOVO_GAMEZONE_DATA.GetPowerChargeMode` | `0x831020f8` (GBMD) | 读状态走 WMI;切换快充走直驱(更可靠) |
| 电池信息 (容量/电压/电流) | `Win32_Battery` (标准 WMI) | `AcpiVpc` 内部 `SVCR/SBSL/SHDC` (不直驱) | 永远走标准 WMI 或 sysfs |
| 电池详情(厂商/序列号/化学) | `Lenovo_BatteryInformation` (WBAT) | `0x83102138` (83B 结构) | WMI 仅 3 字段;详情走直驱 |
| 适配器功率 | 无 | `0x8310215c` (GAPD) | 必须直驱 |
| 特性状态查询 | 无 | `0x831020c4` (GET cmd) | 必须直驱 |
| USB 充电开关 | 无 | `0x831020c0`/`0x831020e8` [推断 cmd] | 必须直驱 |
| DGPU 状态 | 无 | 直驱 (代码未在样本中) | 必须直驱 [推断] |

### 3.6 内部协议 (共享内存 + 事件同步)

**用户态与驱动之间的协议**不是普通 `DeviceIoControl`—— AcpiVpc 还维护了一套**事件驱动的共享内存协议**,供 Vantage Power Addin 使用。虽然实现者主要走 `DeviceIoControl`,但理解这一层有助于理解为什么某些调用看似异步。

**共享帧布局**:
```
偏移   大小   含义
0x00    4    Magic: 'AeiI' (0x49696541) 写入时初始值
0x04    4    CmdId: 'VPCR' (0x52435056) 读 / 'VPCW' (0x57435056) 写
0x08    4    Length: 读=4 写=8
0x0C    4    Status: 0 = success, 非 0 = NTSTATUS
0x10   12    Reserved (XMM0 清零)
0x1C    4    回复 Magic: 'AeoB' (0x426f6541)
0x20    ?    数据 payload
```

**驱动内部 8-phase 状态机**(会话分发函数):
```
phase 0: 空闲 → 若 state_a != 0,初始化 (phase=1, 48=0x1A)
phase 1: VPCR 读 → out==0 则 phase=5
phase 2: 位测试 (out & 0x8151550a80000) → FB 置位
phase 3..7: 其他读/写命令 (SBSL/SHDC/SVCR/AeiC)
phase 5: 完成 → KeSetEvent 唤醒用户态
```

**用户态句柄通知**:驱动维护 3 组连接表 (A/B/C),每组最多 64 个用户句柄 (元素大小 `0xB0` 字节);事件发生时用 `KeSetEvent` 唤醒对应句柄:

| 位 | 含义 |
|---|---|
| `0x00000001` | 电池状态变更 |
| `0x00000002` | AC 电源状态变更 |
| `0x00000004` | 热/风扇事件 |
| 其他 | Lenovo 专有事件 |

**Power IRP 处理**(电源分发函数):接收 `IRP_MJ_POWER`,通过 `IoBuildSynchronousFsdRequest` 向下方 ACPI HAL 发 IRP,输入缓冲区 `0x81B` 字节 (`_PSS` 性能状态结构),输出 `0x10058` 字节 (页对齐 64KB + 88 字节)。

### 3.7 驱动会话创建流程

```
DriverEntry
  → 创建 \Device\EnergyDrv
  → 创建符号链接 \BaseNamedObjects\EnergyDrvEvent
  → 挂载到下层设备栈 (IoAttachDeviceToDeviceStack)
  → 首次 VPCR 探测
  → 轮询忙标志 (ctx+0x40)
  → Ready
```

用户态进程通过 `CreateFile` 打开设备 → 驱动在 `IRP_MJ_CREATE` 处理中:
1. `ObReferenceObjectByHandle` 引用用户事件对象。
2. 从 `ExAllocatePoolWithTag(0x200, 'EVET')` 分配 512 字节条目。
3. 把句柄存入 A/B/C 连接表之一,`idx*0xB0 + 0x80` 处。
4. 计数 +1。

### 3.8 EC 通道边界

**`AcpiVpc.sys` 不直接访问 EC I/O 端口 (0x62/0x66)**—— 它完全通过 ACPI 层的 `_BST`/`_BIF`/`_WMI` 等方法与硬件交互。EC 的实际寄存器读写由 ACPI 固件在 `_Region(EC)` 里完成,Windows 内核 API 无法绕过。

---

## 4. `lenovoDriverBus.sys` — 虚拟总线

### 4.1 元信息

| 项 | 值 |
|---|---|
| 框架 | Kmdf 1.15 |
| 服务 | `lenovoDriverBus`, `SERVICE_KERNEL_DRIVER`, `SERVICE_DEMAND_START` |
| INF Class | System `{4d36e97d-e325-11ce-bfc1-08002be10318}` |
| 硬件 ID | `Root\VID_LENOVO_INC_PID_LENOVO_VIRTUAL_BUS_0001` (非 PnP,由注册表/SwDeviceCreate 手动创建) |
| Security | `D:P(A;;GA;;;SY)(A;;GA;;;BA)(A;;GA;;;WD)(A;;GA;;;RC)` — SYSTEM / BA / WD / RC 全 GA |
| PDB | `Z:\readyforassist\drivers\ScreenShareDriver\...` |

### 4.2 数据结构

| 结构名 | 用途 |
|---|---|
| `LFDO_DEVICE_DATA` | 单个 PDO 上下文 (Lenovo Forward Device Object) |
| `FILE_OBJECT_DATA` | 文件对象扩展 |
| `PDO_DEVICE_DATA_COMMON` | PDO 通用数据基类 |
| `SB_REQUEST_CONTEXT` | 请求上下文 |

### 4.3 与用户态关系

**实现者不需要直接对话 `lenovoDriverBus.sys`**。它的作用是:

- 通过 Kmdf 枚举多个虚拟 PDO,每个 PDO 代表一个 Lenovo 软件设备 (屏幕共享、键盘 Fn 键 HID、音频切换、摄像头、MagicBay LTE 等)。
- 上层软件 (MagiCenter / Vantage) 通过标准 WDF 设备接口 `SetupDiEnumDeviceInfo` + `SetupDiGetDeviceInterfaceDetail` 枚举这些 PDO。
- 硬件访问仍由 `AcpiVpc.sys` 完成,上层通过共享事件 `\BaseNamedObjects\EnergyDrvEvent` 与 AcpiVpc 同步。

**衍生设备**:`lenovoDriverHid.inf` 在 `lenovoDriverBus` 基础上枚举 HID PDO,`VID_LENOVO_INC_PID_LENOVO_DRIVER_HID_0001`,接口 GUID `{63384FBF-F398-46AB-86C9-5CCDF1EC4917}`,使用 UMDF 2.15 (`lenovoDriverHid.dll` + `Lenovo.CertificateValidation.Native.dll`)。

**实现者注意**:如果需要识别联想专有 HID 设备,可通过 `SetupDiGetClassDevs` 过滤此接口 GUID。

---

## 5. `LnvDrvFdn.sys` — 过滤驱动 (防御性,不直接影响功能)

### 5.1 元信息

| 项 | 值 |
|---|---|
| 文件名 | `LnvDrvFdn.sys` |
| FileVersion | `1.0.0.5` |
| FileDescription | "LDF Filter Driver" (LDF = Lenovo Defense Framework) |
| PDB | `D:\1_Lpcm\02 Defense\LDF\LDF\x64\Release\LnvDrvFdn.pdb` |
| 框架 | FLT (FltMgr.sys) |
| 导入 | `FltRegisterFilter`, `FltStartFiltering`, `FltCreateCommunicationPort`, `FltSendMessage`, `BCryptOpenAlgorithmProvider`, `BCryptHashData`, `BCryptVerifySignature`, `BCryptImportKeyPair` |

### 5.2 职责

- 作为 FLT 文件/磁盘过滤驱动,挂载在文件卷上层。
- 拦截 `IRP_MJ_CREATE` / `IRP_MJ_WRITE` / `IRP_MJ_SET_INFORMATION` 对 Lenovo 保护目录 (`System32/drivers/` 下的 Lenovo 驱动) 的非法修改。
- 使用 `PsReferenceProcessFilePointer` 进行进程文件指针跟踪。
- 使用 `BCrypt*` API 做驱动签名校验 (SHA256 + RSA)。
- 拦截 `WerFault.exe` / `WerFaultSecure.exe` / `WerMgr.exe` (Windows Error Reporting)。

### 5.3 通信端口

`FltCreateCommunicationPort` / `FltSendMessage` — 用户态代理(可能是 `lnvscenter.sys`)通过命名端口与驱动通信,报告/查询驱动完整性状态。**实现者无需对接**。

---

## 6. 用户态服务与 IPC 通道

### 6.1 `LNVDispatcherService.exe` — Lenovo 进程调度器

| 项 | 值 |
|---|---|
| 设备 ID | `ACPI\IDEA200C` |
| INF | `LNVProcessManagement.inf`, DriverVer `2025-12-16, 3.2.0.19` |
| 主进程 | `LNVDispatcherService.exe` (1.3 MB) |
| 服务名 | `LenovoProcessManagement`, `SERVICE_OWN_PROCESS`, `SERVICE_AUTO_START` |
| 关键 DLL | `LenovoIPF.dll` (DPTF 对接), `LnvMSRIO.sys` (MSR 读写), `LnvEvtProvider.dll` (WMI 事件), `LnvMSRIO.sys` (MSR Minifilter) |

**订阅的 WMI 事件**:
```
SELECT * FROM LENOVO_DISPATCHER_EVENT
SELECT * FROM LENOVO_UTILITY_EVENT
SELECT * FROM Lenovo_AI_Scenario_TYPE_Event
```

**`PowerLevel` 编码** (`LENOVO_DISPATCHER_EVENT`):

| 值 | 含义 | 对应 Windows 电源计划 |
|---|---|---|
| `0` | NotDefined | — |
| `1` | High Performance | Max Performance / Balanced (AC) |
| `2` | PowerSaving | Balanced (DC) / Better Battery / Max Power Savings |

**IPC 接口**:

- **Named Pipes**:服务内部暴露多个 named pipe(字符串中出现 `CDispatchCommand::ConnectNamedPipe`, `CNamedPipeServer::PipeRead`, `WriteCurveData:CreateNamedPipe`)[推断] 名称约定为 `\\.\pipe\LenovoDispatcher.*`。Vantage / PCManager 通过它下发策略、查询 DBDC 曲线数据。真实 pipe 名需从 `LNVDispatcherService.exe` PE 里进一步定位 [推断]。
- **WMI 双向**:订阅 3 个 `_EVENT`;通过 `LENOVO_REPORT_STATUS_TO_DISPATCHER_EVENT` 接收下游回报。
- **WMI 方法**:通过 `LENOVO_OTHER_METHOD.GetFeatureValue/SetFeatureValue` 与 `LenovoIPF.dll` 组合完成 DPTF 交互。
- **MSR 直接读写**:经 `LnvMSRIO.sys` 读写 CPU MSR (DYTC、EPP、AC/DC 电源切换等)。
- **Windows Energy QoS (EQoS)**:直接调 `SetProcessInformation` / `PROCESS_POWER_THROTTLING_EXECUTION_SPEED (0x01)` / `DISK (0x02)` / `NETWORK (0x04)` 对非前台进程做节能节流。

**DBDC (Dynamic Battery Discharge Control)**:`DBDCWatch` 线程订阅 `LENOVO_REPORT_DBDC_DATA`,根据电池温度/功率限制动态下压 TDP。样本实测:

```
Counts=3, CurrentLimit=[7500,5000,4500], Threshold=[100,40,20], ROS_Power=[0,0,0]
```

含义:3 档功率限制,阈值分别是 100/40/20 [推断:电池温度],超限则下压到对应电流限制 (mA)。

**调度算法关键信号**(从 组件字符串资源):
```
LNVdispatcher[ECOQOS]:SetPowerThrottling PASS for PID %d
LNVdispatcher[ECOQOS]:PROCESS_POWER_THROTTLING_StateMask %d
LNVdispatcher[Affinity_QOS]:SetProcessAffinityMask %x %x
LNV_Dispatcher [UpdateACEPP]: EPP Change From %d to %d
LNV_Dispatcher [UpdateDefaultACEPP]: DefaultEPP From %d to %d
Notify BIOS:EnableDispatcherActive enable %d
```

### 6.2 `LenovoUtilityService.exe` — Fn 与功能键

| 项 | 值 |
|---|---|
| 设备 ID | `ACPI\LHK2019` |
| INF | `LenovoFnAndFunctionKeys.inf`, DriverVer `2026-03-05, 2.1.2602.8` |
| 主进程 | `LenovoUtilityService.exe` (199 KB) |
| 服务名 | `LenovoFnAndfunctionKeys`, `SERVICE_OWN_PROCESS`, `SERVICE_AUTO_START` |
| 关键 DLL | `ludp.dll` (Lenovo Utility Driver Protocol), `spkvol.dll` (音量), `mmstate.dll` (麦克风/静音) |

**热键事件流**:
```
硬件 Fn+F? / 专用媒体键
  → ACPI\VPC2004 产生 SCI
  → ACPI\LHK2019 暴露 WMI 事件 LENOVO_UTILITY_EVENT(PressTypeDataVal)
  → LnvEvtProvider.dll 打包
  → 订阅方:LenovoUtilityService / LNVDispatcherService
```

**热键动作映射 [推断]**:

| 键 | 动作 | 实现路径 |
|---|---|---|
| Fn+Space | 键盘背光 +/-/Cycle | `LENOVO_LIGHTING_METHOD` |
| Fn+F1 | 麦克风静音 | `mmstate.dll` |
| Fn+F2/F3 | 亮度 -/+ | `LENOVO_PANEL_METHOD.Panel_Set_Brightness` [推断] |
| Fn+F4 | 热点/QR | 显示 `QRCode.html` |
| Fn+F5/F6/F7 | 音量 -/+ 静音 | `spkvol.dll` |
| Fn+F8/F9/F10 | 媒体键 Play/Next/Prev | `SendInput` |
| Fn+F11 | 显示切换 | `LENOVO_PANEL_METHOD.Panel_Set_State` |
| Fn+F12 | GameZone 启动 | `LENOVO_GAMEZONE_DATA` |
| Fn+CapsLock | BIOS Setup 热启动 | 硬件级直通 |
| Fn+Ctrl | Fn/Ctrl 互换 | `Lenovo_BiosSetting.FnAndCtrlKeySwap` |
| Fn+Win | Win 键禁用 | `LENOVO_GAMEZONE_DATA.SetWinKeyStatus` |

### 6.3 `ipfsvc.exe` — Intel DPTF / IPF

| 项 | 值 |
|---|---|
| 包 | `dtt_sw.inf_amd64_27c1316661666cc3` |
| 主进程 | `ipfsvc.exe` (660 KB) |
| 显示名 [推断] | "Intel Platform Framework Service" |

**Named Pipe 接口**:
- `\\.\pipe\ipfsrv.public` — 面向外部应用 / 服务 (LenovoIPF.dll / Vantage 等)
- `\\.\pipe\ipfsrv.private` [推断] — 内部策略模块间

**协议**:通过 `IpcServer.dll` 收发 `ESIF_DATA_XML`(Intel ESIF XML Schema,内含 DPTF policy 表)。

**注册 ESIF 事件**:139 个 `ESIF_EVENT_*`,涵盖电源、电池、性能、DPTF 内部表变更、外部设备、传感器等。

**策略配置文件** (INF `configuration/`):

| 文件 | 对应策略 |
|---|---|
| `dtt.config` | DTT 顶层 |
| `epo.config` | Energy Performance Optimizer (EPO) |
| `itm.config` / `itm3.config` | Intelligent Thermal Management |
| `oppboost.config` | Opportunistic Boost |
| `rfim.config` | RFIM |
| `apo.config` | Application Performance Optimization |
| `ap.config` | Adaptive Performance |
| `vs2.config` | Virtual Sensor v2 |
| `systemconfiguration.config` | System Configuration |

**运行时路径 [推断]**:
- `C:\Windows\System32\driverStore\DPTF\config\*`
- `C:\Windows\Temp\DPTF*.xml` (DPTF 运行时导出)
- `C:\ProgramData\Intel\DPTF\` (持久化)

### 6.4 `lnvsst` (Smart Sense Technology)

| 项 | 值 |
|---|---|
| 设备 ID | `ACPI\IDEA2002` |
| 服务 | `SmartSense`, `AUTO_START` |
| 组件 | `SmartSense.exe`(服务), `SmartSenseController.exe`(WMI Provider 宿主), `UserSSCtrl.exe`(UI agent), `LnvSST.dll` (UMDF 2.15.0) |
| 事件 | `LENOVO_SMART_THERMAL_MONITOR_EVENT.Status` |

**职责**:用户在场检测、环境光/温度/运动传感器采集、上报给 Dispatcher / DTT。

---

## 7. 权限 / 服务依赖 / 会话要求

### 7.1 权限矩阵

| 操作 | 所需权限 | 备注 |
|---|---|---|
| WMI 方法类调用 (`LENOVO_*_METHOD`, `Lenovo_SetBiosSetting`) | 管理员或 SYSTEM | `root\WMI` 命名空间 ACL 限制 |
| WMI 事件订阅 | 一般用户即可 | WMI 事件推送不要求特权 |
| 打开 `\\.\EnergyDrv` | 管理员 (BA/SYSTEM) | 驱动 SD 只允许 BA/SY |
| `DeviceIoControl` (0x831020xx) | 管理员 (随设备句柄) | 同上 |
| 连接 `\\.\pipe\ipfsrv.public` | 管理员 | ipfsvc 校验 SDK/Client 版本 |
| 连接 `\\.\pipe\LenovoDispatcher.*` | 管理员 [推断] | 服务内部校验 |
| 用户态 MSR 读写 (通过 `LnvMSRIO.sys`) | 管理员 | 需要服务活着 |
| 直接调用 `SetProcessInformation` (EQoS) | 管理员 (对他人进程) | 自己进程不需要 |
| `lenovoDriverBus` PDO 枚举 | SYSTEM / BA / WD / RC | 从安全描述符分析 |

### 7.2 服务依赖

| 想做的事 | 最低需要 |
|---|---|
| 读取电池信息 | 无(标准 WMI `Win32_Battery`) |
| 读取性能模式 | 无(WMI `LENOVO_GAMEZONE_DATA.GetThermalMode`) |
| 切换性能模式 | 无(WMI 直抵 EC) |
| 快充 / 适配器 / USB 充电 | AcpiVpc.sys 加载 |
| 风扇曲线读写 | 无(WMI 直抵 EC) |
| 键盘背光 | 无(WMI 直抵 EC) |
| BIOS 设置读写 | 无(标准 WMI `Lenovo_*`) |
| 面板刷新率/色彩 | 无(WMI 直抵 EC) |
| Panther Lake DPTF/EPP/TGP | `ipfsvc.exe` 服务运行 |
| 前台进程调度 | `LNVDispatcherService.exe` 运行(或自行调 EQoS) |
| 热键订阅 | 无(ACPI 直推 WMI 事件) |
| 热键动作(音量/亮度 OSD) | 可选 LenovoUtilityService |
| AI 场景联动 | 可选 LNVDispatcher / ipfsvc |

### 7.3 会话模型

- **Session 0**:系统服务(所有 `_SERVICE` 服务)运行在 Session 0。
- **Session 1+**:用户登录会话。
- WMI 方法跨会话可见(命名空间是全局的);事件订阅需要在目标会话中启动 watcher 才能收到 UI 侧事件。
- `\\.\EnergyDrv` 设备句柄**跨会话共享**(符号链接在 `Global` 命名空间)。
- Named Pipe 会话隔离:每个 pipe 通常**每个会话一个实例**;Service 侧 pipe 在 Session 0,用户态客户端需要跨会话连接。

---

## 8. 错误码与失败语义

### 8.1 WMI 方法失败

| 返回值 | 含义 |
|---|---|
| `Boolean=true, Data=0` | 成功 |
| `Boolean=true, Data!=0` | 方法执行了,但返回私有状态码;需查表(见各功能文档) |
| `Boolean=false` | 固件/驱动拒绝;可能是:功能不支持、参数越界、权限不足 |
| WMI 调用本身异常 | `HRESULT` 错误:如 `WBEM_E_NOT_FOUND (0x80041002)` 类不存在;`WBEM_E_ACCESS_DENIED (0x80041003)` 权限不足;`WBEM_E_INVALID_PARAMETER (0x80041008)` |

### 8.2 `DeviceIoControl` 失败 (直驱 IOCTL)

当 `DeviceIoControl` 返回 `FALSE` 时,必须调用 `GetLastError` 判断:

| GetLastError | 含义 |
|---|---|
| `ERROR_SUCCESS (0)` | 不应出现在失败路径 |
| `ERROR_INVALID_FUNCTION (1)` | IOCTL 码不被驱动识别;驱动版本太老 |
| `ERROR_ACCESS_DENIED (5)` | 权限不足 |
| `ERROR_INVALID_PARAMETER (87)` | 输入/输出缓冲区大小不对 |
| `ERROR_NOT_READY (21)` | 设备未就绪 (如电池未插入) |
| `ERROR_IO_DEVICE (32)` | 设备故障;驱动或 EC 异常 |
| `STATUS_DEVICE_DATA_ERROR (0xC0000001)` | 回复 Magic 校验失败 (`AeoB` 不对) |
| `STATUS_IO_DEVICE_ERROR (0xC014000F)` | 状态字校验失败 (Status Word != 0) |
| `STATUS_NO_MEMORY (0xC000009A)` | 驱动内部分配失败 (`ExAllocatePoolWithTag` 返回 NULL) |
| `ERROR_OPERATION_ABORTED (995)` | 调用被取消 |
| `ERROR_HANDLE_EOF (38)` | 设备已卸载 |

**官方错误日志字符串**:
- `"Open rapid charge failed, GLE=%d"` — 快充打开失败,直接输出 GLE。

### 8.3 驱动内部协议失败

| 校验点 | 失败码 |
|---|---|
| 回复 Magic != `AeoB` (0x426f6541) | `STATUS_DEVICE_DATA_ERROR (0xC0000001)` |
| OutputBuffer[0x08..0x0B] != 0 | `STATUS_DEVICE_DATA_ERROR` |
| OutputBuffer[0x05..0x06] != 0 | `STATUS_IO_DEVICE_ERROR (0xC014000F)` |
| `ExAllocatePoolWithTag` 返回 NULL | `STATUS_NO_MEMORY (0xC000009A)` |

### 8.4 幂等与重试建议

- **读操作幂等**:任何 `Get*` 方法/IOCTL 可安全重试。
- **写操作非幂等**:任何 `Set*` 方法**先读后写**,避免覆盖用户修改。
- **ACPI 命令有内部重试**:驱动 8-phase 状态机对 phase 1/2/3 都有循环重试逻辑,直到 `ctx+0x48` 或 `ctx+0x4C` 上限。上层无需对直驱 IOCTL 做短重试。
- **建议重试策略**:IOCTL 失败 → 若 GLE 是 `ERROR_IO_DEVICE` / `STATUS_DEVICE_DATA_ERROR`,等待 100ms 后重试最多 3 次;其他错误立即返回。

---

## 9. 附录 A 引用索引

本文件引用的常量与类,完整定义在:

| 参考 | 文件 |
|---|---|
| WMI 类全量签名 | `A-wmi-reference.md`(从 `目标机 WMI 仓库实机采集` 提取) |
| 证据索引 | `B-evidence.md` |
| 通道层 API | `00-cleanroom-charter.md` §5.3 |
| 电池/电源详细 | `02-power-battery.md` |
| 散热/性能详细 | `03-thermal-performance.md` |
| 外设详细 | `04-peripherals.md` |
| BIOS 详细 | `05-bios-settings.md` |
| Panther Lake 调优 | `07-pantherlake-tuning.md` |
| Linux 后端 | `09-linux-backend.md` |

---

## 10. 证据索引

| 结论 | 证据 |
|---|---|
| `\\.\EnergyDrv` 设备路径 | AcpiVpc 驱动数据段(UTF16 常量);内部档案 §2.1 |
| `\BaseNamedObjects\EnergyDrvEvent` 符号链接 | AcpiVpc 驱动数据段;同上 |
| 内部 IOCTL `0x0032C004` | AcpiVpc 会话相关两处分发函数均载入常量 `0x32c004` |
| 共享帧 Magic `AeiC`/`AeoB` | 会话函数内联常量 `0x43696541` (AeiC);回复校验 `0x426f6541` (AeoB) |
| VPCR/VPCW tag | 会话函数内联常量 `0x52435056` (VPCR) / `0x57435056` (VPCW) |
| 连接表基址/大小/上限 | 连接表管理函数内联常量: `0x5090/0x7c90/0xa890`, `0xb0` 元素大小, `0x3f` 上限 |
| 分配标签 'EVET' | 连接表管理函数内联常量 `0x54455645` |
| 位掩码偏移 `0xEC/0xF0/0xF4` | 事件标志函数对偏移 `0xEC` 做 OR 置位 |
| 位测试表 `0x8151550a80000` | 会话分发函数位测试逻辑 |
| 直驱 IOCTL `0x831020f8` (GBMD) | PowerBattery 组件行为分析报告:快充/养护六条路径的 DeviceIoControl 调用点全部使用 `0x831020f8` |
| 快充失败日志字符串 | PowerBattery 组件日志字符串: `Open_rapid_charge_failed__GLE_d` |
| GBMD tag `0x444d4247` | AcpiVpc GBMD 处理函数内联常量 `0x444d4247` ('GBMD') |
| CFG 命令 `'_CFG'` | 配置处理函数内联常量 `0x4746435F` |
| `SHDC/SBSL/SVCR` 命令 | 三个处理函数分别内联常量 `53484453`/`5342534C`/`53564352` |
| Power IRP 缓冲 0x81B/0x10058 | 电源分发函数 |
| `lenovoDriverBus` 硬件 ID / Kmdf 1.15 / Security SD | `pnf_str.txt`(INF 数据分析), `bus_str.json` |
| `LnvDrvFdn` 框架 (FLT) 与 BCrypt 调用 | LnvDrvFdn 组件字符串资源 |
| WMI 类签名 | `目标机 WMI 仓库实机采集`(全量 689 行) |
| 服务元信息 (LNVDispatcher / LenovoUtility / ipfsvc / lnvsst) | `Lenovo 系统服务组件内部接口说明 |
| `ipfsvc` Named Pipe `\\.\pipe\ipfsrv.public/private` | `Lenovo 系统服务组件内部接口说明 组件字符串资源 |
| DBDC 样本数据 | `Lenovo 系统服务组件内部接口说明 |
| WMI 事件全景表 | `Lenovo 系统服务组件内部接口说明 |
| `lenovoDriverHid` 接口 GUID `{63384FBF-...}` | `hal-drivers.md` §5.3 |
| 热键动作映射 | `Lenovo 系统服务组件内部接口说明 |
| Lenovo EQoS 使用 | `Lenovo 系统服务组件内部接口说明 组件字符串资源 |

**[推断] 标记条目**(本文档中出现 `[推断]` 或 "未确认" 的所有内容):

1. `0x831020f4 GAPD` / `0x831020e8 USBCHARGE` 等码的精确语义
2. `0x83102120/12c/138/14b/14c/150/15c` 电池相关码的语义
3. LNVDispatcherService Named Pipe 真实名称
4. GBMD 输入/输出 payload 结构
5. `SetFanCooling` `Data` 取值映射(1=高性能/2=静音)
6. `SetSmartFanMode` `Data` 取值映射
7. `GetPowerChargeMode` `Data` 取值映射
8. `GetThermalMode` `Data` 取值映射
9. `Panel_Get_Display_Mode` / `Panel_Get_Gamut_Switch` 的枚举值
10. `LENOVO_UTILITY_DATA` datatype→语义 映射
11. `LENOVO_UTILITY_EVENT.PressTypeDataVal` 编码表
12. `ipfsvc` 运行时 XML 精确路径
13. `LENOVO_LIGHTING_DATA.Current_State_Type` 特效枚举
14. `GetDataByPackage` Input 二进制协议格式

---

*本文档结束。Windows 后端实现者仅凭本文档 + `A-wmi-reference.md` 即可实现全部硬件通道层代码,无需接触任何二进制。*
