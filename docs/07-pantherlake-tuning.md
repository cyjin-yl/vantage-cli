# 07 · Panther Lake 低功耗调优 — 策略与接口规范

> 读者:实现者。目标:在 Lenovo 21VG(ThinkBook 14+ 2026,Intel Panther Lake)上提供
> 比官方 Vantage/PCManager 更细粒度的功耗调优,支持「任意接口、任意形态」的用户策略。
> 交叉引用:性能模式三态与风扇见 `03-thermal-performance.md`;DPTF 硬件通道见
> `01-hal-interfaces.md` §6;Linux 映射见 `09-linux-backend.md` §5;DSL 语法见 `10-config-schema.md`。

## 1. 功耗控制栈全貌(分层职责)

```
┌─ 用户策略层   vantage tune / daemon 自动策略(本工具,替代 Vantage SmartScenario + PCManager AiTurbo)
├─ OS 电源层    Windows Power Scheme(GUID_PROCESSOR_THROTTLE 等)/ Linux intel_pstate EPP
├─ 调度提示层   HWP/EPP + Thread Director(异构调度提示)/ PCManager ResScheduler(进程亲和性)
├─ DPTF/IPF 层  Intel Innovation Platform Framework:IETM 管理器 + 参与者(SEN1-5/CHRG/TPWR)
│               策略由 BIOS 注入,ipfsvc.exe(dptftcs)承载配置服务
├─ EC/固件层    VPC2004 + EC:模式字节(经 GBMD/EC 命令)→ BIOS DPTF 重算;PL1/PL2 最终仲裁者
└─ 硬件层       RAPL(MSR 0x610/0x611/0x614...)、温度传感器(DTS)、电流/电压轨
```

**关键事实(目标机实测枚举)**:

| 设备 | 实例 | 驱动 | 角色 |
|---|---|---|---|
| DTT Updater | `SWC\VID8086_DTTCFG_0001\00` | — | DPTF 配置更新组件 |
| DTT Device | `SWC\VID8086_DTT_0001\00` | WUDFRd | 用户模式 DTT 设备 |
| IPF Manager | `ACPI\INTC10D4\IETM` | ipf_acpi | IETM,策略管理器 |
| 温度参与者 ×5 | `ACPI\INTC10D5\SEN1..SEN5` | ipf_acpi | 板载温度传感器 |
| 充电参与者 | `ACPI\INTC10D5\CHRG` | ipf_acpi | 充电功率节流 |
| 电源参与者 | `ACPI\INTC10D8\TPWR` | ipf_acpi | 平台功率(PL 族) |
| 主板资源 | `ACPI\INTC109D\…` | — | DPTF 共享内存/资源 |
| 服务 | `dptftcs`(Running/Automatic) | ipfsvc.exe | DPTF Telemetry/Config |

电源方案实测:仅「平衡」GUID `381b4222-f694-41f0-9685-ff5bb260df2e`(官方不新增方案,
靠覆盖 GUID 域实现模式差异——见 03 文档 §7)。

## 2. 官方软件的调优路径(分析结论)

### 2.1 Vantage(SmartPerformance / SmartScenario)

- SmartPerformanceAddin 只做系统清理/维护调度,**不直接调功耗**(证据:Vantage SmartPerformance 组件内部接口说明)。
- 真正的模式切换走 ITS/Dispatcher 服务控制消息(03 文档 §2):`SetFn4_ModeData` 把模式号
  (0/1/2)经 AcpiVpc 写入 EC,**EC 驱动 BIOS 重算 DPTF 参与者**——即 Vantage 不直写 DPTF,
  是「EC → BIOS → DPTF」间接路径。

### 2.2 PCManager(GameSettingsPlugin)

三条通道并存(电脑管家电源组件内部接口说明):
- **(A) Power API**:覆盖平衡方案的 GUID 域(PL1 间接、GPU、C-state、磁盘)。
- **(B) Lenovo WMI/EC**:`LENOVO_GAMEZONE_DATA.SetSmartFanMode` 等 + `SetFn4_ModeData` 模式字节。
- **(C) Intel DTT 直调(`_DT` 后缀方法族)**:`SetGpuTDPWithSMFAN_DT`(GPU TDP 上限)、
  `SetGpuTemperatureWithSMFAN_DT`(GPU 温度墙)、`Get/SetSmartFanMode_DT`、
  `GetSupportThermalMode_DT`、`IsSupportFullSpeedMode_DT`。
  **仅 Smart Fan 模式下调用**(方法名含 SMFAN);节能模式不直写 DTT,由 PROC_THR_STATE 间接约束。
  **实机更正(2026-08-27,原 U3)**:`root\WMI` **不存在**任何 Intel/DTT 类(实机全量枚举确认);
  这些 `_DT` 名字是 PCManager 的 Vantage RPC 契约名,真正的 DTT 控制面是
  ACPI `INTC10D4`(IETM)+ `INTC10D5`(参与者)+ `ipfsvc.exe`(ESIF 服务)。
  用户态直达 DTT 的路径是 ESIF IPC(named pipe / TCP localhost,ESIF 客户端协议),
  或经 BIOS 间接(写 EC 模式 → BIOS 重配 DTT 策略)。不要寻找 `Intel_TuningTechnologyService` WMI 类——不存在。

### 2.3 独占与冲突

- 进程级调度(亲和性/优先级/工作集)为 PCManager 独有(ResScheduler,03 文档 §6)。
- Vantage 与 PCManager 写同一 EC 寄存器会互相覆盖——本工具取而代之,不存在该冲突。
- DBDC(电池直充限流):WMI `LENOVO_REPORT_DBDC_DATA` 实例实测
  `CurrentLimit=[7500,5000,4500]`、`Threshold=[100,40,20]`(目标机 WMI 实例实机采集)——
  三档:电量 >40% 限 7500 mA,20-40% 限 5000 mA,<20% 限 4500 mA [推断语义]。

## 3. 用户可编程面(本工具暴露的接口)

### 3.1 PL1/PL2/tau

| 路径 | 接口 | 说明 |
|---|---|---|
| Windows 主路径 | ESIF IPC(`ipfsvc.exe`,named pipe `\\.\pipe\DttServerPipe`)写 TPWR 参与者功率限 | 与 DTT 同层;即时生效,重启后 BIOS 策略恢复。备选:写 EC 模式字节交 BIOS 重配 |
| Windows 备选 | `PowerWriteACValueIndex(GUID_PROCESSOR_THROTTLE, …)` | 百分比语义,间接 |
| Windows 底层(需驱动) | MSR `0x610 PKG_POWER_LIMIT`(PL1/tau/PL2)、`0x611 PKG_POWER_SKU`、`0x614 PKG_POWER_SKU_UNIT` | 需 ring0;**本机实测不可行,见下** |
| Linux | `/sys/class/powercap/intel-rapl:0/constraint_{0,1}_{power_limit_uw,time_window_us}` | 标准 intel_rapl;无需私有驱动 |

> **实机结论 (2026-08-27,ThinkBook 14 G8+ 21VG):MSR 直写路径在本机封闭。**
> 本机启用 VBS/HVCI,微软易受攻击驱动黑名单生效:WinRing0x64 加载被拒
> (StartService err=183)。凡需 ring0 的 MSR 读写在此安全基线下均不可用;
> 且从架构看,DPTF(ipf_acpi)持续按策略重编程 RAPL,偶发直写也会被覆写。
> **实现者不要在 Windows 侧依赖 MSR;PL 控制一律走 ESIF IPC 或 EC 模式字节。**

#### 3.1.1 ESIF IPC 会话协议(2026-08-27 三轮实机+组件行为分析,部分闭环)

**结论:该管道是 IPF-EF 的"会话/状态"通道;随驱动发行的 R0 客户端不含 verb 执行面。
实现者如需与 ipfsvc 会话,优先用 Intel 随驱动发行的客户端库 `IpcClient.dll`;
PL 控制本身应绕过本通道(见 §3.1 表的 EC 模式字节/ITS 路径)。**

已证实的事实:
- 管道 `\\.\pipe\DttServerPipe` 由 `ipfsvc.exe`(Intel IPF Service)创建,字节模式,
  任何本地用户可连接;每客户端会话在服务器侧表现为 `DttServerPipe:<pid>` 实例。
- 消息为 **UTF-8 JSON、NUL 结尾**。服务器模板(组件字符串资源,逐字节确认):
  - 会话建立广播:`{"RoleName": "public", "SessionId": "<uuid>", "Status": "Online"}`
  - 会话终结:`{"SessionId": "<uuid>", "Status": "Offline"}`
  - 错误应答:`{"Exception": "<msg>"}` / `{"Exception": "Unspecified"}`
  - 二进制负载的 JSON 包装格式:`{"bytes":[...],"subtype":...}`
- 服务器对无法识别的首条消息**直接断连、无应答**(实机探测多种候选封套均如此);
  连接后服务器**不会主动问候**(问候仅在会话建立成功后广播)。
- 客户端 ABI(实机 P/Invoke 验证可用):
  - `IpcClient.dll!IpcClientR0Create(std::string jsonConfig)` —— 配置是 **JSON 字符串**,
    键:`PipeName`(默认 `DttServerPipe`)、`MaxSessions`;**必须传 JSON**,裸管道名/URI 触发异常。
  - 返回 `Ipf::IpcClient*`,vtable = `[dtor, SetStatusCallback(std::function), Connect, Disconnect, GetStatus]`。
  - 调 `Connect` 后服务器侧立即出现 `DttServerPipe:<pid>` 会话实例(实机观察确认)。
  - **R0 客户端接口不含 Exec/Transact 方法**;服务器是否接受 JSON 形式的 verb 请求未证实(实机候选封套探测均被断连)。
- Rust 侧可直接 `libloading` 加载 `IpcClient.dll`(DPTF 驱动包随系统安装,
  路径 `%SystemRoot%\System32\DriverStore\FileRepository\dtt_sw.inf_amd64_*\IpcClient.dll`)使用上述 ABI;
  这不构成对 Lenovo 私有组件的依赖(Intel 发行物)。
- DPTF 遥测/日志为 WPP 事件(实机抓取 7k+ 事件均无法离线解码,无 TMF/manifest),
  **不要指望 ETW 通道**。
- [TODO 非阻断] 服务器请求 JSON 的完整键集未穷举;如需完整 wire 协议,
  在测试机上以 `IpcClient.dll` 正常建会话并对管道做流量镜像即可补全。

单位换算:MSR 0x614 power_unit = 2^(-(bits 3:0)) W;sysfs 直接 µW。
安全边界:PL1 ≤ PL2;PL1 低于 ~7W 会显著掉速 [经验值];写 RAPL/MSR 前读默认值记录以便恢复。

### 3.2 EPP / HWP 提示

| 平台 | 接口 |
|---|---|
| Windows | Power Scheme `GUID_PROCESSOR_PERFORMANCE_ENERGY_PERFORMANCE_PREFERENCE`(经 PowerWrite*ValueIndex,0-100) |
| Linux | `/sys/devices/system/cpu/cpu*/cpufreq/energy_performance_preference`(performance/balance-performance/balance-power/power) |
| 全局 Speed Shift | Linux `/sys/devices/system/cpu/intel_pstate/no_turbo`、`hwp_dynamic_boost`;Windows 经电源方案 |

### 3.3 进程级调度(替代 ResScheduler)

原语(Windows):`SetProcessAffinityMask`(P/E 核组,先 `GetLogicalProcessorInformationEx`
枚举 RelationProcessorCore 分组)、`SetThreadPriority`、`SetProcessWorkingSetSizeEx`、
`RegisterPowerSettingNotification` 事件源。与 Thread Director 兼容的保守做法:
**只设亲和性掩码不锁组**,让内核完成具体核选择(与官方一致,电脑管家电源组件内部接口说明)。
进阶(官方未用):EcoQoS `SetProcessInformation(ProcessPowerThrottling)`、`SetThreadInformation`——
本工具可提供 `tune throttle <pid>` 超集能力。

### 3.4 EC 模式字节(与 DPTF 联动)

Fn+Q 三态 = 经 AcpiVpc 写 EC 模式号(0=智能/1=节能/2=野兽 [推断映射,以 03 文档 §2 实测为准]),
BIOS 收到后重算 DPTF 参与者。**本工具的两条路线**:
- 简单:复用 EC 模式字节 + 少量 DTT 覆盖(与官方等效);
- 精细:绕过模式字节,直接写 DTT 参与者 + RAPL,获得任意 PL1/PL2/温度墙组合(EC 可能
  在模式事件时覆写,daemon 需监听并重新施加——见 §6 回退模型)。

## 4. 遥测(`tune telemetry`)

| 指标 | Windows | Linux |
|---|---|---|
| 包功耗/能量 | MSR `0x611` PKG_ENERGY_STATUS(需 ring0);或 DTT 事件 `LENOVO_REPORT_POWER_CONSUMPTION_CHANGE_EVENT`(ModeID[]/PowerConsumption[]) | `/sys/class/powercap/intel-rapl:0/energy_uj` 差分 |
| 核/核群频率 | PDH `\Processor Information`\;`CallNtPowerInformation` | `/proc/cpuinfo`、`cpufreq/scaling_cur_freq` |
| 温度 | DTT SEN1-5 参与者(经 ESIF IPC 读,或 ACPI 热区);`LENOVO_GAMEZONE_DATA.GetCPUTemp/GetGPUTemp` | hwmon(`coretemp`、`acpitz`) |
| C-state 驻留 | PDH `% Cx Time` | `/sys/devices/system/cpu/cpu*/cpuidle/state*/time` |
| 电池充放功率 | `_BATTERY_INFORMATION_EX` Wattage/Current(02 文档 §7) | `/sys/class/power_supply/BAT0/power_now` |
| 充电节流状态 | DTT CHRG 参与者;DBDC 实例 | power_supply `charge_control_*`[若内核支持] |

事件:模式切换经 `LENOVO_DISPATCHER_EVENT`(PowerLevel);AC 切换 `LENOVO_AC_PD_EVENT`(02 文档 §11)。

## 5. 调优配方(profile 集)

> 配方即 10 文档 DSL 的内置 profile;每套列出全部参数面。值域以「目标机实测安全区」标注,
> [推断] 处建议首次使用前 `tune telemetry` 实测校准。

### 5.1 `silent-library`(图书馆静音)

| 参数 | 值 | 通道 |
|---|---|---|
| EC 模式 | 节能(模式字节 1) | AcpiVpc |
| PL1 / PL2 / tau | 9 W / 15 W / 28 s [推断安全区] | DTT TPWR 或 RAPL |
| EPP | balance-power(≈179-220) | pstate / Power GUID |
| 风扇 | Smart Fan 静音档 | `SetSmartFanMode` |
| turbo | off | `no_turbo` / 处理器最大状态 99% |
| 充电 | 养护开(若长期插电) | GBMD 0x03 |
| 面板 | 60 Hz 固定 | §04 面板 |
| 后台 | 非前台进程工作集收缩 + 低优先级 | §3.3 |

### 5.2 `long-battery`(长续航)

EC 模式 1;PL1 12 W / PL2 20 W / tau 28 s;EPP balance-power;turbo on(突发响应);
面板 60 Hz + 动态背光;DGPU 强制 IGP_PRIORITY(若适用,03 文档 §8.2);
后台进程 throttle(EcoQoS);充电养护开;Wi-Fi 节能(系统电源方案无线 GUID=最大节能)。

### 5.3 `balanced`(均衡,默认)

EC 模式 0(智能);PL1 15 W / PL2 30 W / tau 28 s [按出厂 PKG_POWER_SKU 回读校准];
EPP balance-performance;Smart Fan 智能;面板 VRR(Mode=1);不写任何 RAPL(交还 DPTF)。

### 5.4 `performance`(性能)

EC 模式 2(野兽);PL1 25 W / PL2 45 W [推断,需实测散热上限];EPP performance;
风扇 Performance 档;面板 120 Hz;关闭工作集收缩;游戏白名单进程高优先级。

### 5.5 `plugged-max`(插电满血)

EC 模式 2;PL1/PL2 = 出厂 SKU 上限(读 MSR 0x611 回写);tau = 最大值;
EPP performance;风扇全速可选(`fan fullspeed`,03 文档 §3);DBDC 不动(EC 自主)。

## 6. 抽象模型:目标-约束-回退(供 08/10 文档引用)

```
Profile = {
  goal:       { pl1_w?, pl2_w?, tau_s?, epp?, turbo?, fan_mode?, ec_mode?, panel_hz?, ... }
  triggers:   [ on_ac | on_battery | process_match([...]) | temp_above(t) | power_above(w) | time_range(...) ]
  constraints:{ temp_max_c?, fan_rpm_max?, battery_only?, min_dwell_s? }   # 生效前置条件
  fallback:   { on_temp_exceed: profile_ref | on_conflict: restore_factory }
}
```

执行语义(daemon):
1. 触发器求值 → 选出候选 profile(优先级:进程匹配 > 温度 > 电源 > 时间段 > 默认)。
2. 约束检查失败 → 不施加,记事件。
3. 施加顺序:先读并缓存当前值 → 写 EC 模式 → 写 DTT/RAPL → 写 EPP → 写外设。
4. 回退:温度越限立即切 fallback profile;外部覆写检测(轮询 PL1 ≠ 期望值)→
   重新施加并计数,连续 N 次失败 → 放弃并告警(EC 强制仲裁场景)。
5. `min_dwell_s` 防抖(默认 10 s,对标官方 debounce 行为)。

## 7. CLI 契约

```
tune profile list|show NAME
tune profile apply NAME [--dry-run]          # §5 配方;--dry-run 只打印将写入的键值
tune pl1 W [--pl2 W] [--tau S]               # §3.1;持久性说明:重启后回 BIOS
tune epp {performance|balance-performance|balance-power|power|0-255}
tune turbo {on|off}
tune throttle PID... / tune boost PID...     # §3.3 进程级
tune restore                                 # 恢复出厂(读缓存/重读 SKU)
tune telemetry [--json] [--interval S]       # §4 流式
tune watch                                   # 触发器/回退事件流(daemon)
```

## 8. 证据

| 结论 | 证据源 |
|---|---|
| DPTF/IPF 设备枚举与驱动 | `目标机 MagicBay/DPTF 实机枚举数据`(Get-PnpDevice 实测) |
| PCManager 三通道与 `_DT` 方法族 | `电脑管家电源组件内部接口说明`、§8.2(GameSettingsPlugin 组件字符串资源) |
| 节能模式不直写 DTT | `电脑管家电源组件内部接口说明 末、§10 |
| EC 模式字节 → BIOS → DPTF 间接路径 | `电脑管家电源组件内部接口说明(B)、§3.4 |
| ResScheduler 原语与 Thread Director 兼容策略 | `电脑管家电源组件内部接口说明 |
| DBDC 三档限流实例 | `目标机 WMI 实例实机采集`(`LENOVO_REPORT_DBDC_DATA`) |
| 充电参与者 CHRG 联动 | `电脑管家电源组件内部接口说明 |
| 功耗遥测事件字段 | `目标机 WMI 仓库实机采集`(`LENOVO_REPORT_POWER_CONSUMPTION_CHANGE_EVENT`) |
| DTT 控制面位置 | **已实机闭环(2026-08-27)**:`root\WMI` 无 Intel/DTT 类;控制面 = ACPI INTC10D4/INTC10D5 + `ipfsvc.exe` ESIF IPC。`_DT*` 为 PCManager 契约名 |
| PL1/PL2 配方具体瓦数 | [推断] §5——需 `tune telemetry` 在目标机实测散热曲线后校准 |
| MSR 被 EC 周期覆写可能性 | [推断] §3.1——Lenovo 机型普遍存在 EC 仲裁,需实测 |
