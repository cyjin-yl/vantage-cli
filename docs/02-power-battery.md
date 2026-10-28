# 02 · 电源与电池管理 — 功能与接口规范

> 读者:实现者。本文档自足描述「电源与电池」域的全部可观测行为、通道、常量与状态机。
> 样本:Lenovo 21VG (ThinkBook 14+ 2026, Panther Lake) 上的
> Lenovo Vantage 10.2606.12.0 / VantageService 5.1.2607.5 / PCManager 5.1.200.8201,
> ACPI 驱动 AcpiVpc.sys (EnergyVpc 15.11.30.11)。
> 交叉引用:通道层细节见 `01-hal-interfaces.md`;WMI 类签名索引见 `A-wmi-reference.md`。

## 1. 范围与 CLI 子命令签名

本域覆盖两套官方软件的下列功能:

| 功能 | Vantage | PCManager | CLI 子命令 |
|---|---|---|---|
| 充电模式三态(常态/养护/快充) | ✔ | ✔(另有自定义阈值/极限续航) | `battery charge-mode {normal|conservation|rapid}` |
| 自定义充电阈值(起充/停充百分比) | ✗ | ✔ | `battery thresholds <start%> <stop%>` |
| 极限续航模式(60% 停充+节流) | ✗ | ✔ | `battery extreme-life {on|off}` |
| 夜间充电 | ✔ | ✗ | `battery night-charge {on|off}` |
| 临时充电模式查询 | ✔ | ✗ | `battery temporary-mode` |
| 电池信息(健康/循环/固件) | ✔ | ✔ | `battery status [--json]` |
| 适配器检测与功率识别 | ✔ | ✔ | `battery adapter` |
| AlwaysOnUSB(AC 下 USB 对外供电) | ✔ | ✔(BIOS 层) | `usb always-on {on|off}` |
| 电池模式下 USB 对外供电 | ✔ | ✔(BIOS 层) | `usb charge-on-battery {on|off}` |
| 电源计划切换/高级电源设置读写 | ✔ | ✔ | `power scheme {list,get,set,apply}` |
| 一键省电 | ✗ | ✔ | `power saver-once` |
| 电源/适配器/电池事件监听 | ✔ | ✔ | `battery watch` |

净室排除:Vantage 的电池 widget UI、电池类广告与遥测上报。

---

## 2. 通道总览(优先级从高到低)

| # | 通道 | 承担者(官方栈) | 用途 |
|---|---|---|---|
| 1 | `\\.\EnergyDrv`(AcpiVpc.sys)DeviceIoControl | `PowerBattery.dll!CDriverLib` | 一切下发型动作:充电模式、快充、USB 供电、适配器信息 |
| 2 | `root\WMI` `Lenovo_BatteryInformation`(实例 `ACPI\PNP0C14\WBAT_0..5`) | PCManager `BatterySetting.exe` | 充电模式/阈值回读,`CurrentSetting` 属性 |
| 3 | `\\.\BatteryDriver` + `IOCTL_BATTERY_QUERY_INFORMATION` | `CBatteryDriver` | OS 层电池信息(与 Windows 电池子系统同底层) |
| 4 | SMBus(经 EnergyDrv 转发到 Smart Battery) | `CBatteryInformation` | 电池身份、循环数、固件版本、健康度 |
| 5 | Windows Power API(`PowerEnumerate`/`PowerRead/Write*ValueIndex`/`PowerSetActiveScheme`) | `CPowerScheme` / PCManager | 电源计划与高级电源设置 |
| 6 | `root\WMI` 事件类订阅 | Addin 事件泵 | 适配器/功耗/电池事件 |
| 7 | 注册表 | 各层 | 用户选择持久化(见 §3.4) |
| 8 | Windows SCM(`OpenServiceW`/`ControlService`) | `CIntelligentCooling` 等 | 性能模式(见 03 文档,本域仅引用) |

**实现建议**:写入一律走通道 1(EnergyDrv IOCTL,语义最精确、已被逐项验证);回读优先通道 2/3(WMI/OS,不持有驱动句柄);通道 1 需要本地管理员权限打开设备句柄。

---

## 3. 充电模式三态(Normal / Conservation / Rapid)

### 3.1 语义

三态**互斥**,由 EC 固件执行:

| 模式 | 枚举值(`BatteryChargeModeType`) | EC 行为 | 适用场景 |
|---|---|---|---|
| Normal | 0 | 充电至 100%,常规电流 | 默认 |
| Storage(养护/Conservation) | 1 | 充电截止电压由 ~4.2V 降至 ~4.0V,上限约 55%(官方文档口径 55–60%) | 长期插电使用,延长电池寿命 |
| Quick(快充/Rapid/Express) | 2 | 进入快充电流档(约 20W 上限提升),30 分钟级快速补电 | 临时应急 |

> Storage80 变体:固件 EM Spec 版本 ≥ `EM_SPEC_VERSION_STORAGE_80` 阈值时,合约层会额外置位
> `ChargeModeStorage80`(上限 80%)。**目标机 21VG 该能力位为 0**,实现时按能力探测结果决定是否暴露。

### 3.2 底层下发:GBMD 历史子命令资料(非目标机写入许可)

下发统一经 `\\.\EnergyDrv` 的通用助手(官方内部名 `CDriverLib::DeviceIoControl`):
`DeviceIoControl(hDrv, dwIoControlCode, inBuf, inSize, outBuf, outSize, &ret, NULL)`,同步调用。

**核心命令字 `IOCTL 0x831020f8`(内部名 GBMD, Get/Set Battery Mode)**:
- 输入缓冲区:**1 字节子命令**;输出缓冲区:**4 字节状态字**(0 = 成功)。
- DeviceType `0x8310`(厂商自定),METHOD_BUFFERED,FILE_ANY_ACCESS。

| 子命令字节 | 语义 | 验证来源(PowerBattery 组件行为分析) |
|---|---|---|
| `0x03` | 历史资料称 gen1 养护开;**目标 21VG 存在运输模式风险，禁止发送** | `CStorageMode::OpenFeature` 120 |
| `0x05` | 养护模式 **关**(gen1) | `CStorageMode::CloseFeature` 2d0 |
| `0x0d`(13) | 养护模式 **开**,gen2 变体 | 历史组件资料;目标机仅确认请求被接受，未证实实际生效 |
| `0x0f`(15) | 养护模式 **关**,gen2 变体 | `CStorageMode::CloseFeature`,同上条件连发 |
| `0x07` | 快充(Rapid/Express)**开** | `CExpressMode::OpenFeature` be0 |
| `0x08` | 快充 **关** | `CExpressMode::CloseFeature` d00 |
| `0xff`(255) | **查询特性支持**(养护/快充的 `IsSupport` 与 ARM 变体共用) | `CExpressMode::IsSupport` ac0、`CStorageMode::IsSupportOnARM` f70 |

**目标机安全边界**:历史资料的 gen1/gen2 连发规则不适用于 21VG 验收;
不得发送 `0x03`，也不得把状态字成功当成模式已切换。目标机现有实现因独立回读契约
未验证而拒绝充电模式提交;已测 `0x0d`/`0x0f` 请求被接受但所测回读未变，见 C2。

有已验证读回来源的控制器在写请求报错或读回失败时，连首个写请求也视为可能已生效，
尝试恢复已捕获的前值并核验;恢复失败时保留主错误与恢复错误，状态标为未验证。
由于传输错误未标识请求是否送达，恢复可能额外发出设置前值的写请求;首次读回失败或
dry-run 不进入写入与恢复阶段。
这不放宽目标机写入门禁，也不代表恢复逻辑已在真实固件上验收。错误字节序实验及
其不能证明真实恢复的边界见 C15。

**适配器状态读取(实机闭环,原 U2 已解决)**:`IdeaNotebookAddin ToastUtils\AdapterInformation.cs`
接口规格——适配器状态 = GBMD **子命令 `0xFF`** 的返回 DWORD:`bit24`=认证充电器能力,
`bit15-16`=适配器类型(0=Inbox,1=Lenovo,2=Unknown,3=SlowCharger);实机返回 `0x00860004`
(无认证充电器,Inbox 类型)。bit24=1 时跟进 **GAPD `0x8310215c`**(in=4B 零,out=10B:
`u16 PID, u16 VID, u16 SystemPowerW, u16 CurrentPowerW`);本机未接认证充电器,实机 err=87。
子命令 `0/1/2` 实机返回 `bytesReturned=0`,**不是**查询子命令(原候选排除)。

### 3.3 状态机与读写分离

```
SetChargingMode(mode):
  if IsSupport()==0 且 IsSupportOnARM()==0: 跳过下发(仅写注册表)
  switch mode:
    0 Normal:  Storage.CloseFeature(); Express.CloseFeature()
    1 Storage: Express.CloseFeature(); Storage.OpenFeature()   # 先关对侧
    2 Quick:   Storage.CloseFeature(); Express.OpenFeature()
    3 (内部):  Storage.OpenFeature(); Express.OpenFeature()    # 位拆模式,UI 从不上交
  写注册表 BatteryChargeMode = {"Normal"|"Storage"|"Quick"}  ← 无论下发成败都写
```

**读写分离语义(重要)**:`CloseFeature`/`OpenFeature` 失败只产生日志
(`"Close storage mode failed"` / `"Open rapid charge failed, GLE=%d"` 等),
**不向上层抛错**;UI 展示的是「最后一次成功读到」的值。实现者应区分 `set` 的提交结果与
`status` 的回读结果,并在 `set` 后延迟回读校验(官方在性能模式域的做法:50 ms × 10 轮询,
见 03 文档;充电模式域官方未做回读校验,建议实现者补上)。

**回读 `GetChargingMode()` 返回 bitmask,不是枚举**:

| 返回值 | bit0 (Storage) | bit1 (Express) | 语义 |
|---|---|---|---|
| 0 | 0 | 0 | Normal |
| 1 | 1 | 0 | Storage 开 |
| 2 | 0 | 1 | Quick 开 |
| 3 | 1 | 1 | 异常组合,UI 回显最近一次 |
| `0xFFFFFFFF` | — | — | 读取失败(设备句柄/IO 错误) |

### 3.4 持久化与恢复

| 键 | 路径 | 取值 |
|---|---|---|
| `BatteryChargeMode` | `HKCU\SOFTWARE\Lenovo\VantageService\AddinData\IdeaNotebookAddin` | `"Normal"` / `"Storage"` / `"Quick"` |
| `AdapterToastStatus` | 同上 | 0=提示开,1=提示关 |

恢复语义(`RestoreDefault`/`BatteryChargeModeRestoreDefault`):
1. 优先按注册表恢复,不重新枚举能力:`"Storage"`→`SetChargingMode(1)`,`"Quick"`→`(2)`,其他→`(0)`。
2. 若当前读到 Storage 开且恢复目标为 Normal,下发失败时记日志 `"Cannot restore to normal charging mode."`。

### 3.5 能力探测

`DoesSupportConservationMode()`:
1. SMBIOS 特征列表过滤(特征名 `ConservationMode`):读 Brand/SubBrand/EnclosureType/Family/BIOSVersion/Type,命中 filter 即返回其布尔值。
2. ARM 分支:`GBMD` 子命令 `0xff` 探测。
3. 非 ARM 默认:`CStorageMode` 对象可构造且 EnergyDrv 句柄可打开即支持。

`DoesSupportRapidChargeMode()` 额外条件(任一成立即不支持):
- 特征列表 `QuickCharge` 命中 false;
- **39 Wh 小电池安全策略**:`Is39whBatteryInstalled()` — 电池标称容量 == 39000 mWh
  (`_BATTERY_INFORMATION_EX.DesignCapacity × 10`)时强制禁用快充;
- 非 Lenovo OEM 驱动(EnergyDrv 打不开)。

---

## 4. 自定义充电阈值与极限续航(PCManager 独有)

### 4.1 模式全集

PCManager `BatterySetting.exe` 提供四种充电策略(Vantage 仅有前三态中的两种):

| 模式 | 内部类 | 语义 |
|---|---|---|
| 快速充电 | `CQuickChargingMode`/`CQuickChargingHelper` | 无上限,最大电流(= §3 的 Quick) |
| 常规(自定义阈值) | `CRegularValuePowerSetting`/`CRegularValueItem` | 用户设定 (start%, stop%),低于 start 起充、到达 stop 停充 |
| 养护 | `CStorageMode`/`CStorageChargingHelper` | 停充约 50%±5%(= §3 的 Storage) |
| 极限续航 | `CExtremeBatteryLife`/`Group`/`Item` | 停充约 60%,同时打包 CPU 节流等省电动作 |

### 4.2 阈值读写(2026-08-27 实机验证更新)

- **回读**:实机确认 `Lenovo_BatteryInformation`(WBAT_0..5)是**纯只读数据类**(无方法,
  仅 BatMaker/HwId/MfgDate 三串)。阈值与电池详情回读走 `\\.\EnergyDrv`:
  - `0x83102138`(in=4B 电池索引,out=83B):完整电池结构,实机解析见 01 文档 §3.3。
  - `0x83102120`(in=4B 零,out=20B):全局充电配置,实机返回 `B7 00 …`(位图语义 [推断])。
- **下发链(组件分析确认)**:`ThinkPowerPlugin.dll!SetChargeThreshold()`(无参,
  先 `GetPrivateProfileIntW` 读 `C:\config.ini`,节内键名 `ChargeStartPercentage`/`ChargeStopPercentage`/
  `ChargeStartControl`/`ChargeStopControl`)→ 插件宿主回调(vtable 0x102f17bc)→
  `LenovoPcManagerService` → EnergyDrv。`WrapPlugin.dll!SetChargeThreshold`[197] 同链。
- **终端写路径实机否定结论(2026-08-27 第二次实机验证)**:
  - `ThinkPowerPlugin` 的阈值写最终落在 `ioctl 0x24058`(16B 入/64KB-2 出,generic dispatch),
    目标设备接口由宿主进程注册——该设备类属旧 ThinkPad 电源栈,**本机不存在**,路径休眠;
    本机 `C:\config.ini` 亦不存在(未启用过自定义阈值)。
  - 通用 SET `0x831020c0` `{6,1,0}`/`{6,1,1}` 实机写入 err=0 但 GET cmd 5-8、
    GBMD `0xFF`、`0x83102120` 均无变化——cmd 6 对本机为无效应答(休眠遗留)。
  - **结论:本机尚未找到经实际生效和独立回读验证的任意百分比阈值写通道**。
    历史资料中 `0x0d/0x0f`、`7/8`、`5` 及 `0x83102134` 的模式不能仅凭请求被接受就
    判定为可用;`0x03` 有运输模式风险，禁止发送。未验证的通道不暴露为成功操作。
- 用户配置经 XML 序列化(TiXmlDocument),并另有加密二进制 `cfg.data`(AES 密文特征,
  位于 `ProgramData\devicecenter\cfg.data`)保存自定义阈值;实现者无需兼容该格式,自行持久化即可。
- 参考工具:`pcm-cli.exe --charging-mode`(PCManager 自带 x86 CLI,含
  `CBatteryChargingHelperEx`/`CNightChargingHelper`;注意其输出走 `WriteConsole`,重定向下无输出)。

### 4.3 一键省电(`power saver-once`)

`COneKeyPowerSaverFactory` 打包执行:CPU 降频(经 Power API 写处理器上限)+ 关 DGPU +
降亮度 + 收缩后台工作集(后两者由 ResScheduler 完成,见 03 文档 §6)。无独立硬件命令,
是上述原语的组合;实现者可直接编排。

---

## 5. 夜间充电与临时充电模式

| 功能 | 语义 | 接口 |
|---|---|---|
| 夜间充电 `GetNightChargeMode()` | 返回 bitmask:bit0=On, bit1=Off | `SetNightChargeMode(1)`=On,`(2)`=Off;`RestoreNightChargeMode` 若支持则恢复为 Off |
| 临时模式 `GetTemporaryMode()` | 返回 bitmask:bit0=Permanent, bit1=PlugIn, bit2=DynamicLifeSpan;返回值 ∈ {2,4} 时 `IsTemporaryChargeMode=true` | 只读探测 |

能力探测:`DoesSupportNightChargeMode()` → `IOCTL 0x83102150`(已验证,见 §9 表)。

---

## 6. AlwaysOnUSB / USB 对外供电

两组独立开关,均为「运行时覆盖层」,重启后回归 BIOS 默认值:

| 开关 | 合约枚举 | 取值 |
|---|---|---|
| AC 下 USB 供电(`CUSBCharger`) | `SupportedUsbChargingModeStatusType` | 1=AlwaysOnEnabled, 2=AlwaysOnDisabled |
| 电池下 USB 供电(`CUSBBatteryCharger`) | `SupportedUsbChargingInBatteryModeStatusType` | 1=ChargingInBatteryModeEnabled, 2=Disabled |

- 下发:`CUSBBatteryCharger::OpenOrClose` → **IOCTL `0x831020e8`**(已验证);
  `CUSBCharger` 走同族命令字(确切值 [推断],同 01 文档 IOCTL 表)。
- 回读:组件接口表偏移 `+0x32` 的 `GetStatus`,返回 1=开 / 2=关 / 其他=未知。
- BIOS 层默认:`Lenovo_BiosSetting` 中 `AlwaysOnUSB,Enable` / `ChargeInBatteryMode,Disable`
  (见 05 文档)。`SetUSBChargingMode` 语义:两个参数分别映射两组开关,两者同时为 null 视为非法请求。
- 能力探测:`CUSBCharger::IsSupport()` / `CUSBBatteryCharger::IsSupport()`,不支持时隐藏对应选项。

---

## 7. 电池信息读取(`battery status`)

### 7.1 `_BATTERY_INFORMATION_EX` 结构(字段偏移,值 0xFFFF = 不支持)

下表为通用草图，不是 21VG 已验证的 EnergyDrv 83 字节布局。针对目标机，`docs/01` §3.3
的 `0x14` 是标称/设计电压，不是瞬时端电压；`0x0e` 温度和 `0x10` 电流/功率仍属推断。
当前实现不把这些推断值输出为已验证实时温度或电流。目标机只读差异见
`evidence/target-battery-voltage-discrepancy-20261028.json`。

| 偏移 | 类型 | 字段 | 单位/换算 |
|---|---|---|---|
| 0 | u16 | DesignCapacity | ×10 → mWh |
| 2 | u16 | FullChargeCapacity | ×10 → mWh |
| 4 | u16 | RemainingCapacity | ×10 → mWh |
| 10 | u16 | Voltage | mV |
| 12 | i32 | Current | mA(正=充电,负=放电) |
| 16 | u16 | Temperature | 0.1 K,℃ = t / 10 − 273.15 |
| 18 | u16 | ManufactureDate | BCD:day[4:0] \| month[8:5] \| (year−1980)[15:9] |
| 20 | u16 | FirstUsedDate | 同上 |
| 22 | u16 | DesignVoltage | mV |
| 24 | u16 | RemainingPercent | % |
| 26 | u16 | LifePercent | % |
| 28 | u16 | ChargeStatus | 见 §7.2 |
| 30 | u16 | RemainingTime | min |
| 32 | u16 | ChargeCompletionTime | min |
| 34 | u16 | Wattage | W |
| 36 | u16 | CycleCount | 次 |
| 48 | wstr | ManufactureName | SMBus |
| 56 | wstr | FirmwareVersion | SMBus |
| 72 | wstr | BarCodingNumber | SMBus(16 进制清洗) |
| 80 | wstr | DeviceChemistry | "LION"/"Li-I"→Li_Ion,"LiP"→Li_Polymer |

### 7.1a 当前 Windows 只读输出与来源边界

`battery status --json` 和人类可读输出均保留来源分组，不再混合为一个平坦的 `telemetry`:

| 输出 | 读取来源 | 身份与单位边界 |
|---|---|---|
| `device_telemetry` | EnergyDrv 的设备索引 0 | `0x14` 仅作 `design_voltage_mv`;未证实的瞬时电压、电流和温度为 null |
| `os_battery` | `root\WMI` 电池类 | 状态与静态、满充等可选行只按非空 `InstanceName` 精确关联;歧义拒绝;容量比仅在容量单位可信时计算 |
| `electrical` | Windows 电池类只读查询 | 唯一非短时系统电池的状态电压(mV)、原始带符号速率(mW)及可选温度(0.1 K);不将 mW 推算为 mA |
| `system_power` | `GetSystemPowerStatus` | 全系统聚合状态，不当作某一电池的百分比或厂商适配器读数 |
| `adapter`、`charge_mode` | 各自的厂商接口 | 和 OS/电池类来源分开;无厂商详情时不虚构适配器额定功率或不足功率判定 |

`source_association` 固定为 `unverified`:不同来源之间的电池身份没有被证明。每组有独立
`*_error`，可用结果内的 `source_issues` 保留部分失败;所有主要观察来源均失败时命令失败。
Windows WMI 原始 `ChargeRate`/`DischargeRate` 不标作 mA;`capacity_ratio_percent` 是
满充容量与设计容量的算术比值，不是健康等级。原生电池查询最多等待 2 秒返回给调用者，
但底层 Windows 操作可能继续阻塞;阻塞时不创建更多查询线程。

### 7.2 状态映射

`ChargeStatus`:0=NoActivity,1=Charging,2=Discharging,3=DischargingWithAc,4=Error,5=Detached。
`BatteryHealth`:1=Green(良好),2=Yellow(注意),3=Red(警告),4=Invalid,5=NotInstalled,其他=Error。

### 7.3 Smart Battery V2 健康扩展(`SmartBatteryInfo`)

| 偏移 | 字段 | 值域 |
|---|---|---|
| 0 | IsSmartBatteryV2 | 1/0 |
| 4 | BatteryHealthLevel | 1..N(−1=不支持) |
| 8 | BatteryHealthTip | 建议动作整数(−1=不支持) |
| 12 | PredictedLifeSpan | 天数(−1=不支持) |

相关已验证命令字:`0x83102138`(电池信息 SMBus 读取)、`0x83102120`(固件版本+循环数)、
`0x8310214b`(IsSupportSmartBatteryV2)、`0x8310212c`(GetChargeCompletionTime)。

---

## 8. 适配器检测与功率(`battery adapter`)

`CAdapter` 经 EnergyDrv 读取:

| 方法 | 语义 | 值域 |
|---|---|---|
| `IsACIn()` | AC 是否接入 | 1/0 |
| `GetAdapterStatus()` | 适配器模式 | 0=Full,1=Limited,2=None,3=不支持检测 |
| `GetACAdapterType()` | 接口类型 | 0=USB-C(PD),1=Legacy(方口) |
| `GetAdapterWattage()` | 功率 | W,≤0=未知 |
| `IsLenovoAdapter()` | 认证判定 | 见下 |

**认证/功率识别**:`UpdateAdapterInfo()` → **IOCTL `0x831020f4`(GAPD)** 与
**`0x8310215c`**(均已验证),返回字段:`PID`(USB Vendor ID,0xFFFF=无)、`VID`、
`SystemChargerPower`(W,系统需求)、`CurrentChargerPower`(W,实际)。
`CurrentChargerPower < SystemChargerPower` ⇒ 不足功率判定;任一值缺失时结果为未知。
`battery adapter --json` 分别返回 `vendor_adapter` 和聚合 `system_power` 及各自错误，
不把后者补入厂商适配器字段。
**实机校准(2026-08-27)**:`0x8310215c` GAPD 的精确格式经 `AdapterInformation.cs` 确认——
in=4B 零,out=10B `{u16 PID, u16 VID, u16 SystemPowerW, u16 CurrentChargerPowerW}`;
仅在 GBMD `0xFF` 返回 DWORD 的 bit24=1(认证充电器能力存在)时调用,否则 err=87。
认证判定:GBMD `0xFF` DWORD 的 bit15-16(`AdapterType`)==1 ⇒ Lenovo 认证适配器;
==0 ⇒ Inbox(通用);==3 ⇒ SlowCharger(慢充)。

**EM Spec 版本**:`GetEmSpecVersion()` → **IOCTL `0x8310214c`**(已验证),用于 §3.1 Storage80 判定。

---

## 9. 本域已验证 IOCTL 汇总(证据:PowerBattery 组件行为分析)

设备:`\\.\EnergyDrv`,DeviceType `0x8310`,全部 METHOD_BUFFERED / FILE_ANY_ACCESS。

| IOCTL | 内部名 | 功能 | 验证点(函数) |
|---|---|---|---|
| `0x831020e8` | — | USB 电池下供电开关 | `CUSBBatteryCharger::OpenOrClose` |
| `0x831020f4` | GAPD | 适配器信息 | `CAdapter::UpdateAdapterInfo` |
| `0x831020f8` | GBMD | 通用电池模式(子命令见 §3.2) | `CStorageMode`/`CExpressMode` 全部虚函数 |
| `0x83102120` | — | 电池固件+循环数(SMBus) | `CBatteryInformation::RetrieveAndCacheBatteryFirmwareAndCycleCountFromSMB` |
| `0x8310212c` | — | 充满剩余时间 | `GetChargeCompletionTime` |
| `0x83102138` | — | 电池信息(SMBus) | `RetrieveAndCacheBatteryInformationFromSMB` |
| `0x8310214b` | — | Smart Battery V2 支持探测 | `IsSupportSmartBatteryV2` |
| `0x8310214c` | — | EM Spec 版本 | `GetEmSpecVersion` |
| `0x83102150` | — | 夜间充电支持探测 | `DoesSupportNightChargeMode` |
| `0x8310215c` | GAPD 族 | 适配器信息更新 | `CAdapter::UpdateAdapterInfo` |

调用模式(`CDriverLib::DeviceIoControl`):惰性打开驱动句柄(缓存于对象 +8 字段,
`-1` 表示未打开),打开失败返回 0 并记日志 `"CDriverLib::DeviceIoControl failed to open driver"`;
DeviceIoControl 返回 FALSE 时记 `"… failed, GLE = %d"`(GetLastError)。

---

## 10. 电源计划(`power scheme`)

`CPowerScheme` / PCManager `BatterySetting.exe` 均只使用 Windows Power API,无私有通道:

- 枚举:`PowerEnumerate` / 活动方案 `PowerGetActiveScheme` / 切换 `PowerSetActiveScheme`。
- `power scheme get` / `apply` 优先匹配精确方案 ID;没有精确 ID 时按不区分大小写的
  名称匹配。多个方案名称相同则拒绝，并要求使用 `power scheme list` 返回的精确 ID，
  不按枚举顺序选择一个可能错误的目标。
- 高级设置:`PowerReadACValueIndex`/`PowerReadDCValueIndex`/`PowerWriteACValueIndex`/
  `PowerWriteDCValueIndex` + `PowerReadValueMin/Max/Increment` + `PowerSettingAccessCheck`。
- Windows 设置事务先捕获活动方案 GUID，同一事务的 AC/DC 读取、写入、核验和恢复均绑定
  该 GUID。若观察到活动方案已切换，拒绝或停止事务;恢复原方案内的保存值时不主动
  重新激活已离开的方案。方案激活失败后，若观察到既非前值也非目标的第三个方案，
  不覆盖它。传输失败后恢复仍可能发出写请求。
- 恢复前重新读取原 GUID 的设置：已是前值则不写;仍为本次请求值才尝试恢复前值;
  若是第三个值，保留该值并报告恢复未完成。读取与恢复写入之间仍非原子，不能把该
  判断视为跨进程 compare-and-swap。
- **并发边界**:检查活动 GUID 与调用 `PowerSetActiveScheme` 不是跨进程原子操作;
  Windows 设置或其他软件仍可在两者间切换，也可能并发修改同一方案的值。该实现不
  宣称排除外部竞争或崩溃恢复。模拟测试与目标机原生验收的区别见 C16。
- **PCManager 不新建电源方案**,而是在内置方案上覆盖以下 GUID 域(证据见 §14):
  `GUID_PROCESSOR_THROTTLE`(CPU 上限)、`GUID_VIDEO_SUBGROUP`、
  `GUID_PROCESSOR_IDLE_SUBGROUP`(C-state)、`GUID_DISK_SUBGROUP`、`GUID_SYSTEM_SUBGROUP→PROC_THR_STATE`。
- 事件:`RegisterPowerSettingNotification` 订阅 `GUID_ACDC_POWER_SOURCE` /
  `GUID_PROCESSOR_POWER_SAVING`。

---

## 11. 事件(`battery watch`)

### 11.1 WMI 事件类(root\WMI,intrinsic 订阅)

| 类 | 关键字段 | 触发时机 |
|---|---|---|
| `LENOVO_AC_PD_EVENT` | `AC_PD_Status: UInt16` | 适配器插/拔/功率切换/快充切换;取值由 EC 定义(0/1/2 ≈ 未插/标准/快速 [推断]) |
| `LENOVO_REPORT_POWER_CONSUMPTION_CHANGE_EVENT` | `ModeID[]`,`PowerConsumption[]`,`NumbersOfMode` | 性能模式切换、~30 s 周期、功耗显著变化 |
| `LENOVO_DISPATCHER_EVENT` | `PowerLevel` | Dispatcher 模式切换 |
| `LENOVO_REPORT_STATUS_TO_DISPATCHER_EVENT` | `Type`,`Value` | 通用状态回拨 |
| `LENOVO_GAMEZONE_POWER_CHARGE_MODE_EVENT` | — | 充电模式变化(CHRG 参与者联动) |

### 11.2 EnergyDrv 私有事件(`LnvVpcEventMonitor`)

14 种事件类型(索引 0..13),经驱动事件机制到达;与电源相关的位掩码:

| 位 | 事件 |
|---|---|
| 0x2 | AdapterStatus(插/拔/变化) |
| 0x8 | AdapterStatus(备用标记,INFO 路径) |
| 0x20 | BatteryOverTemp / TouchpadStatus |
| 0x400 | TouchpadStatus(GETEVENT3 路径) |
| 0x1000 | KeyboardBacklightStatus(GETEVENT3 路径) |

Linux 替代:upower D-Bus + udev + `/sys/class/power_supply` uevent(见 09 文档 §9)。

---

## 12. 错误模型与边界情况

1. **EnergyDrv 打不开**(未装驱动/权限不足):所有下发型命令返回「通道不可用」错误;
   回读降级到 WMI/OS 路径。
2. **读写分离**:官方 `set` 不保证生效,实现者应在 `set` 后回读并在 CLI 输出「请求值/实际值」。
3. **39 Wh 电池**:快充必须按 §3.5 禁用,忽略用户请求并说明原因(安全策略)。
4. **三态互斥**:任何 `charge-mode` 切换都必须先关对侧再开目标侧(§3.3 顺序)。
5. **运行时覆盖 vs BIOS 默认**:USB 供电开关重启后回 BIOS 值;CLI 应同时提供
   `--persistent`(写 BIOS 设置,见 05 文档)选项。
6. **并发**:Vantage 与 PCManager 写同一 EC 寄存器会互相覆盖;实现无需兼容二者并存,
   但应容忍外部修改(回读为准,不缓存)。

---

## 13. CLI 行为契约(摘要)

```
battery status [--json]        # §7 来源分组、各自错误及当前 charge-mode(回读 bitmask)
battery charge-mode MODE       # normal|conservation|rapid;先探测支持,§3.3 序列下发,回读校验
battery thresholds S E         # PCManager 语义,§4.2;S<E,5≤S≤95,10≤E≤100
battery extreme-life on|off    # §4.1 + §4.3 组合原语
battery night-charge on|off    # §5,先 0x83102150 探测
battery temporary-mode         # §5 bitmask 原样输出
battery adapter [--json]       # §8 全字段
battery watch                  # §11 事件流,Ctrl-C 退出
usb always-on on|off [--persistent]
usb charge-on-battery on|off [--persistent]
power scheme list|get|apply NAME
power scheme set SUBGROUP SETTING AC|DC VALUE   # PowerWrite*ValueIndex 直映射
power saver-once               # §4.3
```

---

## 14. 证据

| 结论 | 证据源 |
|---|---|
| GBMD 子命令 3/5/0x0d/0x0f/7/8/0xff 与调用模式 | PowerBattery 组件行为分析报告(六条写入路径调用点逐一确认)+ 函数表档案 |
| §9 全部 IOCTL→功能映射 | 同上,常量直接出现于 `mov edx, 0x83102xxx` 指令 |
| 三态状态机/读写分离/注册表键/恢复语义 | 内部验证档案 §2-§3(IdeaNotebookAddin BatteryAgent 组件行为分析) |
| 39 Wh 快充禁用 / Storage80 能力位 | `Vantage 电源组件内部接口说明、`Is39whBatteryInstalled()` |
| `_BATTERY_INFORMATION_EX` 偏移表 | `Vantage 电源组件内部接口说明(`BatteryAgent.GetBatteryInformation` 字段偏移) |
| 适配器认证 bit15/16 / GAPD 字段 | `Vantage 电源组件内部接口说明 |
| 充电阈值四模式 / QueryChargeThreshold / cfg.data | `电脑管家电源组件内部接口说明、§7.5-7.6 |
| 电源计划 GUID 覆盖清单 | `电脑管家电源组件内部接口说明(A)、§9 |
| 一键省电动机 | `电脑管家电源组件内部接口说明 |
| WMI 事件类字段 | `目标机 WMI 仓库实机采集`;实例 `目标机 WMI 实例实机采集`(`Lenovo_BatteryInformation` WBAT_0..5) |
| VPC 事件掩码 | `Vantage 电源组件内部接口说明 |
| 阈值下发链(ThinkPowerPlugin→宿主回调→服务)与读通道 0x83102120/0x83102138 | 组件行为分析 + **实机探测**:`目标机实机接口探测记录`;终端写命令字仍未定死(见 §4.2 两条实现路径) |
| GetAdapterStatus = GBMD `0xFF` + GAPD `0x8310215c` 格式 | **实机闭环**:AdapterInformation 接口规格 + 实机 GBMD 0xFF→`0x00860004`;`目标机实机接口探测记录` |
