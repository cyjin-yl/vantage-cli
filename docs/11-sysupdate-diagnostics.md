# 11 · 系统更新 / 硬件诊断 / 系统信息 — 功能规范(净室取舍版)

> 读者:实现者。覆盖 Vantage 的 SystemUpdate(驱动/BIOS 更新)、HardwareScan(自检)、
> MachineFix(自动修复)、Snapshot(配置快照)、设备信息枚举,以及 PCManager TVSU 模块。
> **本文档同时是净室取舍清单**:「排除」项属于私有/遥测/商业生态,不在重实现范围(charter §1)。
> 交叉引用:BIOS 设置写路径见 `05-bios-settings.md`;音视频见 `04-peripherals.md`。

## 1. 范围与取舍总表

| 功能 | 取舍 | 替代路径 |
|---|---|---|
| 驱动更新(EGather 搜索/下载/INF 安装) | **保留** | PnP 枚举 + HTTPS + SetupAPI |
| UEFI 运行时变量读取(LBSAVAR/LBLDVC) | **保留** | `GetFirmwareEnvironmentVariableExW` / Linux efivars |
| BIOS/固件刷写(IA/BootInstaller) | **排除** | 联想专用固件安装器,建议用户用官方包手动执行 |
| TVSU 私有包格式(MCP) | **排除** | — |
| HardwareScan 命令语义与结果格式 | **保留** | Linux:dmidecode/lm-sensors/smartctl/memtest |
| 诊断驱动 LenovoDiagnosticsDriver.sys | **排除** | 专有内核驱动 |
| MachineFix 修复动作(服务重启/注册表/设备状态) | **保留** | 纯策略逻辑 |
| Snapshot 15 模块 WMI 快照 | **保留** | 标准 WMI 查询 |
| Snapshot Vpp 商业授权 / CDAT 遥测 | **排除** | — |
| 设备信息枚举(SMBIOS/WMI/注册表) | **保留** | `lctrl info` 核心输入 |
| WiFi 安全评估 | **保留** | WlanApi / Linux nmcli |
| McAfee/SecurityCenter、SmartLock(Absolute)、Dolby 专有 API、Vantage 自更新 | **排除** | 商业/专有生态 |

## 2. SystemUpdate(驱动/BIOS 更新)

### 2.1 契约命令(`SystemOptimization.SystemUpdate`)

`CheckForUpdates` / `GetUpdateInformation` / `DownloadAndInstall` / `RollbackUpdate` /
`GetSystemUpdateStatus` / `SetSchedule` / `SetUNCCredential` / `SetProxy` / `SetIgnoredUpdate` /
`GetIgnoredDriver` / `GetAutoUpdate` / `SetAutoUpdate` / `GetCriticalUpdate` / `GetMostRecentUpdate` /
历史记录增删 / `GetDockInfo`。

### 2.2 更新包 XML 格式(Tvsu.XmlPackageProcessor)

每个更新包 = XML 描述 + 二进制附件:
- `PackageElement`:Title/Severity/Vendor/ReleaseDate/Brand/DiskSpaceNeeded/Reboot/PackageType/URL/Files;
- 适用性检测:`DetectVersionElement`(Or/And/Not 复合),检测器含
  `_BiosElement`、`_EmbeddedControllerVersionElement`、`_FirmwareElement`、`_DriverElement`、
  `_PnPIDElement`、`_OSElement`、`_RegistryKey(Element|ValueElement)`、
  `_FileExists/Date/VersionElement`、`_IoctlElement`(自定义驱动控制码)、
  `_ExternalDetectionElement`(外部进程)、`_CoreqElement`(依赖)、`_CPUElement` 等;
- 安装:`InstallElement` → `CmdlineElement`/`INFCmdElement`;卸载:`UninstallElement`。

### 2.3 流程与状态机

```
IDLE → SEARCHING(Egather XML 拉取 + GUR2 规则求值)→ SEARCH_COMPLETE
     → DOWNLOADING(BITS / HTTPS / UNC 三选一)→ DOWNLOAD_COMPLETE
     → INSTALLING(CmdInstaller | InfInstaller(SetupAPI→DriverStore) | ManualInstaller | ShellInstaller)
     → REBOOT_REQUIRED → (RunOnce: Tvsu.exe CONTINUEINSTALL)→ INSTALL_COMPLETE
Rollback:按 Coreq 拓扑倒序调 UninstallElement → ROLLED_BACK
```

- 严重级排序:Critical > Recommended > Driver > Optional;按 Coreq 构建拓扑安装序。
- 进度公式:已完成段×80/总数 + 当前包进度/总数 + 偏移(线性)。
- 调度:Windows 任务计划「Lenovo System Update」(日/周触发);
  AD 策略三个注册表值(全局/关键/推荐开关,`HKLM\SOFTWARE\Policies\Lenovo\…Companion…`)。
- 历史:本地 SQLite(UpdateName/Status/DateTime/PackageID/Severity),并同步到 WMI 记录器。
- 忽略规则:IgnoredDriverFileStore / IgnoredDriverExclusionRule / HardwareIdExclusionRule 三层。

### 2.4 UEFI 运行时变量(保留)

| 变量 | GUID | 内容 |
|---|---|---|
| `LBSAVAR` | `{FF424B14-BF81-48BE-D0F5-D4DCB813B93B}` | `LenovoBiosSyncInfo`:IsSupport/Version/FeatureFlags(SetupUtility/SecureWipe/StartupInterruptMenu…)/ACPITableVersion/ME 版本/UEFI 规范版本 |
| `LBLDVC` | `{871455D1-5576-4FB8-9865-AF0824463C9F}` | Logo DIY 版本 |

平台判定:`GetFirmwareType`(BIOS vs UEFI)。Linux 对应:`/sys/firmware/efi/efivars/`。

### 2.5 PCManager TVSU(参考)

`Installer64.exe`(GUI)/`DiDriverInstall64.exe`(CLI 驱动安装)+ TvsuUpdateMgrLib(搜索/安装 API)
+ egather 缓存 + mcp 打包 + 分架构证书校验 + hotfixplatform 热修复驱动。
净室实现**不兼容** MCP 格式;CLI 复现等价能力:`update check/download/install` 走 §2.2 XML + HTTPS。

## 3. HardwareScan(硬件自检)

契约:`SystemManagement.HardwareScan.General` → `Get-ItemsToScan` / `Do-Scan` / `Cancel-Scan` / `Get-Status`。

```
IDLE → PREPARING → RUNNING → 各部件 DEVICE_TEST → PASS/FAIL/SKIP/WARN
                 → CANCELLED | RESOURCE_USAGE_ERROR
```

- 官方深度测试依赖 `LenovoDiagnosticsDriver.sys`(排除);
- 净室保留:部件清单、测试语义、JSON 结果格式(`LdeApi.JsonObjects.TestResult`),
  Linux 用 dmidecode/lm-sensors/smartctl/memtest86+ 组合达到等效覆盖。

## 4. MachineFix(自动修复)

契约:`Vantage.MachineFixSystem`(`Scan`/`GetIssueList`/`Fix`/`Track`/`GetCapabilities`/`GetNoticePolicy`)
+ `Vantage.MachineFixUser`(`MFCapabilities`/`GetIssueDB`)。

```
SCAN_TRIGGER → SCAN_RUNNING → ISSUES_DETECTED → FIX_RUNNING → FIX_COMPLETE
                                                    ├── NEEDS_REBOOT
                                                    └── FIX_FAILED → TRACK(复发跟踪)
```

修复动作均为策略原语组合:重启服务、改注册表、改设备状态(音频/摄像头模块)。
事件源:App 启动/进程/注册表/设备事件;NoticePolicy 控制告警频率。

## 5. Snapshot(配置快照)

契约:`Lenovo.Vantage.Snapshot.UserAddin` → `GetSnapshotInfo`/`SetBaseline`/`GetLatest`。

```
FIRST_BOOT → BASELINE_CAPTURED →(WMI/注册表变化)→ COMPARE_WITH_BASELINE
             → DIFF=YES →(用户触发)→ RESTORE_FROM_BASELINE
```

15 个 WMI 快照模块(系统/设备/驱动/软件清单等),纯 WMI 查询 + 注册表 + 版本比对;
CLI 等价:`lctrl snapshot {capture,diff,restore}`(restore 限定到本工具管理的设置域)。

## 6. 设备信息枚举(VantageCoreAddin,`lctrl info` 的数据源清单)

| 信息 | 来源 |
|---|---|
| MachineInfo(机型/MTM/序列号) | SMBIOS(`Win32_ComputerSystem`/`Win32_BIOS`)+ 注册表 |
| DiskInfo / MemoryInfo | `Win32_DiskDrive`/`Win32_PhysicalMemory` |
| BootType | `GetFirmwareType` + `Win32_ComputerSystem` |
| CpuFreq / Gpu | WMI + 性能计数器 |
| Network / WlanInfo | WlanApi + `Win32_NetworkAdapter` |
| BiosCapabilities | LBSAVAR(§2.4) |
| Ble / Udc / OOBE | 标准 Windows API(按需保留) |

## 7. WiFi 安全评估(LenovoSecurityAddin,保留)

`SystemManagement.WiFiSecurity`:`Get-State`/`Set-State`/`Get-WiFiAssessment`/`Get-WiFiHistory`。
语义:评估当前/历史 WiFi 的加密强度(开放/WEP/WPA/WPA2/WPA3)并给风险提示;
纯 WlanApi + 注册表策略,Linux 用 nmcli/wpa_supplicant 等效。

## 8. 排除项明细(净室边界)

| 排除 | 原因 |
|---|---|
| IA/BootInstaller 固件刷写 | 联想专用安装器;引导前后特权操作 |
| TVSU MCP 包格式 | 私有打包格式 |
| LenovoDiagnosticsDriver.sys | 专有内核驱动 |
| Vpp 商业授权 / CDAT 遥测 / SmartPerformance 云订阅 | 商业/遥测 |
| McAfee SDK / SecurityCenter 深度集成 | 专有 SDK |
| SmartLock(Absolute) | 商业防盗产品 |
| Dolby DAX/DS1 专有 API | 需 Dolby 许可证(基础开关见 04 §6.1) |
| Vantage 自更新(UpdateOffline) | 官方应用生态 |
| ServiceBridge HTTP 桥 | 联想支持门户对接(语义可保留:直接读官方 support API) |

## 9. CLI 契约

```
update check [--severity critical|recommended|driver|optional]   # §2.3 SEARCHING 等价
update download <id>... / update install <id>... [--reboot]      # XML 包 + HTTPS + SetupAPI
update history [--json] / update ignore <id> / update rollback <id>
update schedule {daily|weekly|off}                               # 任务计划
scan list / scan run [ITEM...] [--json]                          # §3 自检
fix scan / fix run <issue>... / fix track                        # §4
snapshot capture|diff|restore                                    # §5
wifi security [--json]                                           # §7
```

## 10. 证据

| 结论 | 证据源 |
|---|---|
| SystemUpdate 契约命令全集/包 XML 元素/安装器分派/进度公式/RunOnce 续装/AD 策略 GUID | `Vantage 系统更新组件内部接口说明/§3.1/§3.9.1(Client.dll/Engine.dll/IA.dll 组件分析) |
| LBSAVAR/LBLDVC 变量与 GUID | `Vantage 系统更新组件内部接口说明(VantageCoreAddin BiosTool) |
| TVSU 目录构成 | `Vantage 系统更新组件内部接口说明`(TVSU 目录构成) |
| HardwareScan/MachineFix/Snapshot 状态机与契约 | `Vantage 系统更新组件内部接口说明/§3.9.2-3.9.4 |
| 取舍表 | `Vantage 系统更新组件内部接口说明(规格组逐项建议,本组采纳) |
| 设备信息来源 | `Vantage 系统更新组件内部接口说明 |
