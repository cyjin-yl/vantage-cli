# 附录 A · root\WMI Lenovo 类全量参考

> 实机采集自目标机(Lenovo 21VG, ThinkBook 14+ 2026, Panther Lake)的 WMI 仓库
> (`Get-CimClass -Namespace root\WMI` 全量导出,可重跑复现)。语义注释见 01-07 各文档;本附录仅作签名索引。

共 65 个类。命名约定:`*_DATA` = 数据/方法承载类,`*_METHOD` = 方法承载类,
`*_EVENT` = 事件类(经 WMI 事件订阅 `__InstanceCreationEvent` 或 intrinsic 事件接收)。


---

# 数据与方法类


## `Lenovo_AssetTag`

(空类 / 仅作占位)

## `Lenovo_AssetTagElement`

(空类 / 仅作占位)

## `Lenovo_AssetTagWrite`

(空类 / 仅作占位)

## `Lenovo_BatteryInformation`

**不是空类,但也没有容量字段**:`Get-WmiObject -Namespace root/WMI -List` 的属性表只有
`{Active, CurrentSetting, InstanceName}`;实机 6 个实例 `ACPI\PNP0C14\WBAT_0..5` 的
`CurrentSetting` 是身份字符串(`BAT0 BatMaker,…` / `BAT0 HwId,…` / `BAT0 MfgDate,…` 及 `BAT1` 组),
`Active` 恒为 True。**设计容量、满充容量与循环数不在这个类里**——它们只在 `root\WMI` 的
电池类链 `BatteryStaticData` / `BatteryFullChargedCapacity` / `BatteryCycleCount` 上
(`Win32_Battery` 的 `DesignCapacity`/`FullChargeCapacity` 本机为 NULL),见
[evidence/re-battery-health-20261105.md](evidence/re-battery-health-20261105.md)。

> 充电模式/夜间充电/临时模式**没有** WMI 方法类:65 个类中与充电相关的只有只读事件
> `LENOVO_GAMEZONE_POWER_CHARGE_MODE_EVENT` 和本空类;`root\Lenovo` 命名空间不存在。
> 写路径是 `PowerBattery.dll` 的 EnergyDrv IOCTL(见
> [interfaces.md §11](interfaces.md) 与
> [evidence/re-battery-adv-20261105.md](evidence/re-battery-adv-20261105.md))。

## `Lenovo_BIOSElement`

(空类 / 仅作占位)

## `Lenovo_BiosPasswordSettings`

(空类 / 仅作占位)

## `Lenovo_BiosSetting`

(空类 / 仅作占位)

## `LENOVO_BIOS_ASSISTANT`

(空类 / 仅作占位)

## `LENOVO_CAPABILITY_DATA_00`

(空类 / 仅作占位)

## `LENOVO_CAPABILITY_DATA_01`

(空类 / 仅作占位)

## `LENOVO_CAPABILITY_DATA_02`

(空类 / 仅作占位)

## `LENOVO_CPU_METHOD`

(空类 / 仅作占位)

## `LENOVO_CPU_OVERCLOCKING_DATA`

(空类 / 仅作占位)

## `Lenovo_DiscardBiosSettings`

(空类 / 仅作占位)

## `LENOVO_DISCRETE_DATA`

(空类 / 仅作占位)

## `LENOVO_FAN_MAX_SPEED_DATA`

(空类 / 仅作占位)

## `LENOVO_FAN_METHOD`

(空类 / 仅作占位)

## `LENOVO_FAN_TABLE_DATA`

(空类 / 仅作占位)

## `LENOVO_FAN_TEST_DATA`

(空类 / 仅作占位)

## `LENOVO_FEATURE_STATUS_DATA`

(空类 / 仅作占位)

## `Lenovo_FunctionRequest`

(空类 / 仅作占位)

## `LENOVO_GAMEZONE_CPU_OC_DATA`

(空类 / 仅作占位)

## `LENOVO_GAMEZONE_DATA`

(空类 / 仅作占位)

## `LENOVO_GAMEZONE_GPU_OC_DATA`

(空类 / 仅作占位)

## `Lenovo_GetBiosSelections`

(空类 / 仅作占位)

## `LENOVO_GPU_METHOD`

(空类 / 仅作占位)

## `LENOVO_GPU_OVERCLOCKING_DATA`

(空类 / 仅作占位)

## `LENOVO_INTERNAL_PANEL_REFRESH_RATE_DATA`

(空类 / 仅作占位)

## `LENOVO_LIGHTING_DATA`

(空类 / 仅作占位)

## `LENOVO_LIGHTING_METHOD`

(空类 / 仅作占位)

## `Lenovo_LoadDefaultSettings`

(空类 / 仅作占位)

## `LENOVO_MACHINE_LEARNING_LIST`

(空类 / 仅作占位)

## `LENOVO_MEMORY_METHOD`

(空类 / 仅作占位)

## `LENOVO_MEMORY_OC_DATA`

(空类 / 仅作占位)

## `LENOVO_OTHER_METHOD`

(空类 / 仅作占位)

## `LENOVO_PANEL_METHOD`

(空类 / 仅作占位)

## `LENOVO_REPORT_DBDC_DATA`

(空类 / 仅作占位)

## `LENOVO_REPORT_DIRECT_BIOS_DATA`

(空类 / 仅作占位)

## `Lenovo_SaveBiosSettings`

(空类 / 仅作占位)

## `Lenovo_SetBiosPassword`

(空类 / 仅作占位)

## `Lenovo_SetBiosSetting`

(空类 / 仅作占位)

## `Lenovo_SetFunctionRequest`

(空类 / 仅作占位)

## `LENOVO_SR_DATA`

(空类 / 仅作占位)

## `Lenovo_SystemElement`

(空类 / 仅作占位)

## `LENOVO_UTILITY_DATA`

(空类 / 仅作占位)

---

# 事件类


## `LENOVO_AC_PD_EVENT`

(空类 / 仅作占位)

## `LENOVO_AI_CHIP_EVENT`

(空类 / 仅作占位)

## `LENOVO_AI_SCENARIO_TYPE_EVENT`

(空类 / 仅作占位)

## `LENOVO_BTKBD_EVENT`

(空类 / 仅作占位)

## `LENOVO_DISPATCHER_EVENT`

(空类 / 仅作占位)

## `LENOVO_GAMEZONE_FAN_COOLING_EVENT`

(空类 / 仅作占位)

## `LENOVO_GAMEZONE_KEYLOCK_STATUS_EVENT`

(空类 / 仅作占位)

## `LENOVO_GAMEZONE_LIGHT_PROFILE_CHANGE_EVENT`

(空类 / 仅作占位)

## `LENOVO_GAMEZONE_POWER_CHARGE_MODE_EVENT`

(空类 / 仅作占位)

## `LENOVO_GAMEZONE_SMART_FAN_MODE_EVENT`

(空类 / 仅作占位)

## `LENOVO_GAMEZONE_SMART_FAN_SETTING_EVENT`

(空类 / 仅作占位)

## `LENOVO_GAMEZONE_THERMAL_MODE_EVENT`

(空类 / 仅作占位)

## `LENOVO_LIGHTING_EVENT`

(空类 / 仅作占位)

## `LENOVO_REPORT_2D3D_STATUS_EVENT`

(空类 / 仅作占位)

## `LENOVO_REPORT_POWER_CONSUMPTION_CHANGE_EVENT`

(空类 / 仅作占位)

## `LENOVO_REPORT_REFRESH_RATE_EVENT`

(空类 / 仅作占位)

## `LENOVO_REPORT_STATUS_TO_DISPATCHER_EVENT`

(空类 / 仅作占位)

## `LENOVO_SMART_THERMAL_MONITOR_EVENT`

(空类 / 仅作占位)

## `LENOVO_SR_EVENT`

(空类 / 仅作占位)

## `LENOVO_UTILITY_EVENT`

(空类 / 仅作占位)
