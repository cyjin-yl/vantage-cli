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
| 极限续航(运行态策略,非已证实的固定停充阈值) | 原始开关映射待绑定 | 旧资料列有此项 | `battery extreme-life {on|off}`（仍 Unsupported,见 §4.4） |
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

历史组件资料以 `BatteryChargeModeType` 描述三态互斥;其名称、命令与目标 21VG
实际固件状态的映射尚未独立验证:

| 模式 | 枚举值 | 历史语义及目标边界 |
|---|---|---|
| Normal | 0 | 充电至 100%,常规电流 |
| Storage(养护/Conservation) | 1 | 旧资料将其描述为限制充电上限;目标机实际门限和电压未验收 |
| Quick(快充/Rapid/Express) | 2 | 旧资料描述较快充电;目标机具体电流和效果未验收 |

联想 [ThinkBook 14/16 Gen 8+ 官方用户指南](https://iknow.lenovo.com.cn/detail/431592)
列出 21VG / ThinkBook 14 G8+ IPH，并将用户可见的“节能模式”描述为保持 75%–80%。
旧资料中的约 50%–60% Storage 阈值与此不同;两种名称是否对应同一底层模式、目标机
`ChargeModeStorage80` 位的语义和实际停充点均未独立验证，不能据此推断本机阈值。
物理门限验收仍未完成；driver-state 切换与恢复闭环按 C2 的具体证据单独裁定。
2026-11-02 只读探测得到 GBMD `0xFF` = `0x00860004`，仅能确认 advertised
bit23 置位。它不等于完整 `DoesSupportStorage80`：原版还要求有效的 EM 版本至少
184（`0xb8`）。旧探测使用 4B 零输入并返回原始 DWORD `0x00000002`，不是原版
`GetEmSpecVersion` 的 DWORD `0x11` 请求，且原始值 bit0 为 0，不能称作版本 2。
该元数据纠正见 §9 与 `docs/evidence/storage80-aiturbo-re-20261102.md`；完整
Storage80 能力保持未知，不能从 bit23、名称或无效版本回复推断支持/不支持或停充点。

### 3.2 底层下发:安装版本绑定的 GBMD 模式契约（非百分比阈值通道）

下发统一经 `\\.\EnergyDrv` 的通用助手(官方内部名 `CDriverLib::DeviceIoControl`):
`DeviceIoControl(hDrv, dwIoControlCode, inBuf, inSize, outBuf, outSize, &ret, NULL)`,同步调用。

**核心命令字 `IOCTL 0x831020f8`(内部名 GBMD, Get/Set Battery Mode)**:
- 输入缓冲区:**1 字节子命令**；`0xff` 查询输出 **4 字节状态掩码**。
  原版写请求也提供 4B 输出缓冲，但目标观察的写回复为 0B，不把写缓冲当状态字解码；
  实际状态由另一次 `0xff` 查询确定。
- DeviceType `0x8310`(厂商自定),METHOD_BUFFERED,FILE_ANY_ACCESS。

| 子命令字节 | 语义 | 验证来源(PowerBattery 组件行为分析) |
|---|---|---|
| `0x03` | Storage/养护开的基础请求 | `CStorageMode::OpenFeature` RVA `0xc3d0`，先发 `03` |
| `0x05` | Storage/养护关的基础请求 | `CStorageMode::CloseFeature` RVA `0xc580`，先发 `05` |
| `0x0d`(13) | subtype 2 的开追加请求，不是基础请求的替代品 | 仅 `03` 成功且 GBMD bit14 置位时追加 |
| `0x0f`(15) | subtype 2 的关追加请求，不是基础请求的替代品 | 仅 `05` 成功且 GBMD bit14 置位时追加 |
| `0x07` | 快充(Rapid/Express)**开** | `CExpressMode::OpenFeature` |
| `0x08` | 快充 **关** | `CExpressMode::CloseFeature` |
| `0xff`(255) | 查询状态及能力位掩码 | Storage 状态 bit5、Express 状态 bit2、Express 支持 bit17、subtype 2 bit14；bit23 仅 advertised Storage80 位 |

**分发与安全边界**：PowerBattery.dll 1.0.13.114 构造函数（RVA `0x9790`）按
成功的 `0xff` 回复 bit14 选择 subtype 1/2；目标 `0x00860004` 的 bit14 为 0，
对应仅发 `03/05`。此前单发 `0d/0f` 被接受但状态未变，不能拿来否定该基础请求链。
旧版“21VG 的 `03` 是运输模式风险，禁止发送”没有绑定到此路径的关机/固件证据；
安装 DLL 的 Storage 方法确实先发 `03`。目标原生 consumer 观察到
`0x00860004 → [03] → 0x00860022 → [05] → 0x00860000`，未观察到关机，
随后生产 CLI 恢复 Rapid 并另读 `0x00860004`。这纠正了选择子身份，不证明任意固件/
电源状态下普遍安全，更不证明物理 75%–80% 停充点。生产 CLI 的模式/自动恢复闭环
另见 `docs/evidence/conservation-cli-native-20261102.md`、C2；原始 consumer 的
`05` 不等于完整恢复原 Rapid，不能冒充自动恢复成功。

有已验证读回来源的控制器在写请求报错或读回失败时，连首个写请求也视为可能已生效，
尝试恢复已捕获的前值并核验;恢复失败时保留主错误与恢复错误，状态标为未验证。
由于传输错误未标识请求是否送达，恢复可能额外发出设置前值的写请求;首次读回失败或
dry-run 不进入写入与恢复阶段。
成功模式切换/自动恢复的具体观察不代表真实固件失败、故障注入或崩溃恢复已验收。
错误字节序实验及其不能证明真实恢复的边界见 C15。

**适配器状态读取(实机闭环,原 U2 已解决)**:`IdeaNotebookAddin ToastUtils\AdapterInformation.cs`
接口规格——适配器状态 = GBMD **子命令 `0xFF`** 的返回 DWORD:`bit24`=认证充电器能力,
`bit15-16`=适配器类型(0=Inbox,1=Lenovo,2=Unknown,3=SlowCharger);实机返回 `0x00860004`
(无认证充电器,Inbox 类型)。bit24=1 时跟进 **GAPD `0x8310215c`**(in=4B 零,out=10B:
`u16 PID, u16 VID, u16 SystemPowerW, u16 CurrentPowerW`);本机未接认证充电器,实机 err=87。
子命令 `0/1/2` 实机返回 `bytesReturned=0`,**不是**查询子命令(原候选排除)。

### 3.3 状态机与读写分离

以下为安装 DLL 的 `SetChargingMode`（RVA `0x9c30`）路径，不是旧资料推断：

```
SetChargingMode(mode):
  0 Normal:  对支持且开启的 Storage、Express 依次 CloseFeature()
  1 Storage: Storage.OpenFeature(); 成功后若 Express 支持且开启则 CloseFeature()
  2 Quick:   Express.OpenFeature(); 成功后若 Storage 支持且开启则 CloseFeature()
  其他 mode: 不进入上述写分支
  以状态读取最多轮询 10 次，不匹配则 Sleep(50ms)
  仅读回等于目标 mode 时写 BatteryChargeMode 注册表缓存
  返回最后观察到的 mode
```

`CloseFeature`/`OpenFeature` 记录日志并返回布尔结果；写请求成功不等于模式已生效。
原版实际有充电模式回读轮询（`0x9fde..0xa085`），撤回旧文档“充电域未校验、无论成功
失败都写缓存”的描述。客户端仍须传播请求/短回复错误、明确核对请求值和实际值；
不能复制原版状态读取失败可能回落到 Normal 的行为而宣称验证成功。

**native `GetChargingMode()`（RVA `0x9bc0`）先查 Storage，再查 Express**：

| 返回值 | bit0 (Storage) | bit1 (Express) | 语义 |
|---|---|---|---|
| 0 | 0 | 0 | Normal |
| 1 | 1 | 0 | Storage 开 |
| 2 | 0 | 1 | Quick 开 |
| 两位都开 | 1 | 1 | native 先命中 Storage，返回 1；托管映射接受 bitmask 3 不证明 native 会返回 3 |
| 状态查询失败 | — | — | native 不传播独立错误，可能回落为 0；客户端必须保持查询错误，不能据此确认 Normal |

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
| 养护 | `CStorageMode`/`CStorageChargingHelper` | 旧资料描述约 50%±5%;与目标机用户可见“节能模式”是否等价未证实 |
| 极限续航 | `CExtremeBatteryLife`/`Group`/`Item`（旧资料名） | 不再把“60% 停充”作为已恢复契约；AiTurbo 的独立运行态策略与原始 UI 映射见 §4.4 |

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
    判定为可用；`03/05` 模式路径及旧运输模式警告的纠正见 §3.2，它们不是任意百分比
    阈值接口。未验证的阈值通道不暴露为成功操作。
- 用户配置经 XML 序列化(TiXmlDocument),并另有加密二进制 `cfg.data`(AES 密文特征,
  位于 `ProgramData\devicecenter\cfg.data`)保存自定义阈值;实现者无需兼容该格式,自行持久化即可。
- 参考工具:`pcm-cli.exe --charging-mode`(PCManager 自带 x86 CLI,含
  `CBatteryChargingHelperEx`/`CNightChargingHelper`;注意其输出走 `WriteConsole`,重定向下无输出)。

### 4.3 一键省电(`power saver-once`)

`COneKeyPowerSaverFactory` 打包执行:CPU 降频(经 Power API 写处理器上限)+ 关 DGPU +
降亮度 + 收缩后台工作集(后两者由 ResScheduler 完成,见 03 文档 §6)。无独立硬件命令,
是上述原语的组合;实现者可直接编排。

内置屏的独立 `panel brightness` 原语与安全边界见 04 §3.0。

**实现记录(2026-11-03,TB14P-EZRA):** `power saver-once` 已实现为编排器,逐步
执行已验证的原语:

1. **CPU 上限**:对活动电源方案写 `PROCTHROTTLEMAX` DC=60%。本机型只枚举一个
   "平衡"方案,没有独立的 "Power saver" 方案可激活,因此策略直接写入当前活动
   方案(与原版的"写活动方案处理器上限"语义一致)。
2. **EPP**:`PERFEPP` AC/DC → 100%(最大能效)。
3. **睿频**:`PERFBOOSTMODE` AC/DC → disabled。
4. **亮度**:内置屏 → 30%(仅当当前值更高时)。

`--dry-run` 报告每步将执行的动作;`--auto-restore` 在提交后逐项回放捕获的原值
(升序为 boost→epp→brightness→throttle,后写入先恢复)。原版的关 dGPU 与收缩
工作集两腿在本机型无对应原语,如实记为 skipped,不作冒充。

**目标机怪癖(已在代码内处理):** `PROCTHROTTLEMAX`/`PERFEPP`/`PERFBOOSTMODE`
的 AC/DC 索引可读写,但 `PowerReadValue{Min,Max,Increment}` 返回
`ERROR_FILE_NOT_FOUND`。写路径在 range 缺失时以调用方记录的文档语义
(百分比 0..=100)约束值,不再硬拒绝。

### 4.4 极限续航：原版组合策略,不是一次性省电原语

精确版本及完整静态证据见
[extreme-life-re-20261102.md](evidence/extreme-life-re-20261102.md)，
完整进程目录/分组/证书标量规则见其配套 JSON。`battery extreme-life` 仍为 Unsupported。

- BatteryLife 捕获当前活动电源方案，改变其三个 **DC** 索引：无线最大节能 3、
  桌面幻灯片暂停 1、PCIe 链路最大节能 2，再重应用同一 GUID；不存在待提取的固定
  “极限续航方案 GUID”。恢复 JSON 位于 **HKCU** 的 `PowerSchemeSetting`。
- HKCU `Enable` 与 `EnableEcoQos` 是两个独立输入；真实单个 UI 开关怎样写入它们，
  尚须目标匹配的设置前端绑定，不能猜成两者同时开/关。激活还取决于版本化 ITS
  来源和 AC/DC，不可替换为 Windows overlay 枚举。
- 完整策略包括进程关系选择、Job CPU 硬上限、EcoQoS/优先级、最小化窗口、关闭弹窗
  和终止指定后台进程。原版 `off` 不会重新启动已终止进程或恢复关闭窗口。
- 单次 Job 限流和电源 API 可公开实现，但完整父子 job 迁移使用同步内核进程创建
  回调，仍需替代项目拥有且适当签名的执行器及用户会话守护；不准依赖联想交付的
  驱动/服务或以轮询、通用 ProcessPowerThrottling、`power saver-once` 冒充完整等价。
- `WinJobList` 已定位的操作仅为停机时删除 HKCU 键，未建立白/黑名单值结构；
  不得为它编造配置。原生状态/效果验收未执行，也未证明任何 60% 停充或续航增益。

---

## 5. 夜间充电与临时充电模式

| 功能 | 语义 | 接口 |
|---|---|---|
| 夜间充电 `GetNightChargeMode()` | 返回 bitmask:bit0=On, bit1=Off | `SetNightChargeMode(1)`=On,`(2)`=Off;`RestoreNightChargeMode` 若支持则恢复为 Off |
| 临时模式 `GetTemporaryMode()` | 返回 bitmask:bit0=Permanent, bit1=PlugIn, bit2=DynamicLifeSpan;返回值 ∈ {2,4} 时 `IsTemporaryChargeMode=true` | 只读探测 |

能力探测:`DoesSupportNightChargeMode()` → `IOCTL 0x83102150`(已验证,见 §9 表)。

---

## 6. AlwaysOnUSB / USB 对外供电

历史组件资料描述两组运行时覆盖开关及重启后回归 BIOS 设置;该分层和实际效果尚未在
目标 21VG 上验收:

| 开关 | 合约枚举 | 取值 |
|---|---|---|
| AC 下 USB 供电(`CUSBCharger`) | `SupportedUsbChargingModeStatusType` | 1=AlwaysOnEnabled, 2=AlwaysOnDisabled |
| 电池下 USB 供电(`CUSBBatteryCharger`) | `SupportedUsbChargingInBatteryModeStatusType` | 1=ChargingInBatteryModeEnabled, 2=Disabled |

- 历史下发路径:`CUSBBatteryCharger::OpenOrClose` → **IOCTL `0x831020e8`**;
  `CUSBCharger` 走同族命令字(确切值 [推断],同 01 文档 IOCTL 表)。
- 回读:组件接口表偏移 `+0x32` 的 `GetStatus`,返回 1=开 / 2=关 / 其他=未知。
- BIOS 保存设置示例:`Lenovo_BiosSetting` 中 `AlwaysOnUSB` / `ChargeInBatteryMode`
  (见 05 文档);本机只读镜像两项均为 `Enable`，不证明出厂默认或运行时供电。历史
  `SetUSBChargingMode` 资料描述两个参数分别映射两组开关,两者同时为 null 视为非法请求。
- 能力探测:`CUSBCharger::IsSupport()` / `CUSBBatteryCharger::IsSupport()`,不支持时隐藏对应选项。

联想上述 21VG 机型族用户指南称带电池图标的 USB 接口支持关机/睡眠/休眠供电，额定
最大输出 5 V、2 A，并列出固件设置和电脑管家/百应作为开关入口。这是机型族手册，
不是本机端口实际供电测量，也没有提供可验证的 Windows 运行态读写契约。未验证的
运行时控制与 BIOS 写入不得因该手册而开放。

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

**EM Spec 版本**：安装 PowerBattery.dll 的 `GetEmSpecVersion()`（RVA `0x22d0`）发
**IOCTL `0x8310214c`**，输入 **DWORD `0x11`**、输出 4B raw。IOCTL 失败或 raw
bit0 为 0 时 native 返回 `-1`；仅 raw bit0 为 1 时版本为 **`raw >> 4`**。
`DoesSupportStorage80` 还要求该版本至少 **184（`0xb8`）** 与 GBMD bit23 置位。
最小值绑定 `.data` RVA `0x21174`、文件偏移 `0x1ef74` 的 `b8 00 00 00`，不是猜测值。
旧探测的 4B 零输入、raw=`0x00000002` 保留为原始观察，但不是原版请求，也不是已验证
的版本 2；完整版本/Storage80 能力仍未知。修正后的原生形态只读重测因目标传输不可用
待执行，不把读取失败或无效回复等同为能力不存在。

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
- `power scheme get NAME --subgroup GUID --setting GUID` 只读所选方案的 AC/DC 设置索引、
  数值范围及两种电源来源的组策略覆盖状态。两个 GUID 参数必须同时提供;不带它们时
  `get NAME` 仍只返回方案信息。AC/DC 均绑定解析后的同一方案 GUID，但多次读取并非
  原子快照。离散设置可能没有数值范围：此时保留设置值并明确报告范围不可用，不猜测
  边界或步进;其他读取错误继续报告。`PowerSettingAccessCheck` 按设置 GUID 和 AC/DC
  来源查询组策略，而不是按方案查询一般权限;未覆盖不代表当前用户必然能写，也不证明
  硬件功耗、频率或其他物理效果。Linux 不支持此 Windows GUID 设置读取。目标机只读
  对照及未验收边界见 C18。
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

**当前实现**:`vantage battery watch [--interval S]` 以固定间隔轮询电池快照
(`battery status` 的同一观测面),立即输出一次,此后每次读取完成后等待 S 秒;
S 为正整数,默认 1 秒,Ctrl-C 退出。本机未接事件订阅通道,§11.1/§11.2 的
真事件源仅作原始契约档案。

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
4. **三态与恢复**：核对目标三态并保持错误可见；安装 DLL 的实际开/关顺序见 §3.3，
   不能把旧“必先关对侧”的描述当作其反汇编证据。恢复必须还原捕获的原语义模式。
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
battery extreme-life on|off    # §4.4 完整常驻策略尚未实现；不是 §4.3 的同义开关
battery night-charge on|off    # §5,先 0x83102150 探测
battery temporary-mode         # §5 bitmask 原样输出
battery adapter [--json]       # §8 全字段
battery watch [--interval S]   # §11 轮询快照流,默认 1 秒,Ctrl-C 退出
usb always-on on|off [--persistent]
usb charge-on-battery on|off [--persistent]
power scheme list|get|apply NAME
power scheme get NAME --subgroup GUID --setting GUID  # 只读指定方案的 AC/DC 设置与策略状态
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
