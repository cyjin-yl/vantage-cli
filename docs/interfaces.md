# Windows 硬件控制接口规格与验证记录

**方法**:接口行为分析 + 只读运行验证。本文件记录各通道的行为契约、参数语义与验证状态;源码不含任何外部来源的描述符字节表,描述符由本仓库生成器按公开 NDR 格式语法生成。
**边界**:不修改系统或厂商组件、不注入进程、不绕过 ACL/VBS/HVCI、不刷固件、不写 BIOS 口令。

---

## 0. 依赖与常量边界(2026-11-01 用户裁定,适用于本文所有章节)

1. **联想交付的软件不得作为运行时依赖**:Vantage addins 目录下的 payload(DolbyRpcClient.dll、ElevocControlSDK_x64.dll、XCASDK.dll、MONITORCONTROL.dll)不引用、不复制。非联想驱动或应用组件可依第 3 条准入;使用系统 RPC 客户端并不消除对 RPC 服务端的依赖。
2. **接口描述表不得转录**:当前源代码不得内嵌外部来源的 MIDL proc/type format string 或 format-string offsets。描述符由本仓库生成器按公开 NDR 格式语法,从"opnum → 参数类型与属性"生成;接口标识符与参数语义按 §12/§14 的契约描述实现。生成的格式串可能与其他实现相同,这不能单凭输出字节证明其来源。
3. **非联想驱动/应用可引用,但 ABI 稳定是首要判据**:接口版本字段/结构布局必须稳定或带版本协商;引用时记录来源、版本与稳定依据,并评估 Linux 侧等价物(DRM/colord 等)。
4. **行为与原版一致,不做平替**:复刻原版的调用序、信封、字段序与返回码语义;若只能得到不等价的结果,宁可诚实 Unsupported。

## 1. 参考环境组件(目标机实测版本)

| 组件 | 版本 | 角色 |
|---|---|---|
| `LnvMSRIO.sys` | 3.2.0.17 | MSR/物理内存访问微过滤器 + 设备 |
| `WinMSRIO.dll` | 3.2.0.17 | 用户态客户端（MSR/Tcon） |
| `LenovoIPF.dll` | 3.2.0.17 | Intel DPTF/ESIF 性能客户端 |
| `LNVDispatcherService.exe` | 3.2.0.16 | Dispatcher（IPF 服务端宿主） |
| `LnvSST.dll` | 1.0.8.5 | SmartSense 驱动侧 |
| `ludp.dll` | 2.1.6.11081 | LUDP Core（联想统一设备协议） |
| `SmartSAMD.sys` | 1.50.1.0 | **SmartRAID/SmartHBA 存储 miniport（与笔电控制无关）** |
| `lenovoDriverBus.sys` | 2.0.000.133 | **WDF 虚拟总线（VHCI: 虚拟 HID/显示枚举）** |

PDB 源路径（同源构建证据）:
- `D:\Work\1.Dispatcher\Git\process_management\x64\Release\LnvMSRIO.pdb`
- `D:\3.0-dispatcher\process_management-3.2\x64\Release\WinMSRIO.pdb`
- `D:\3.0-dispatcher\process_management-3.2\x64\Release\LenovoIPF.pdb`

---

## 2. MSR/物理内存访问通道 — 设备与访问控制

**微过滤器**:`fltmc filters` 实测 `LnvMSRIO`,6 实例,altitude **370039**;服务 `LnvMSRIO` Running/Auto。

**设备对象**:`\Device\WinMsrDev` → 符号链接 `\DosDevices\WinMsrDev`(用户态 `\\.\WinMsrDev`)。
另注册过滤器通信端口 `\LnvMiniFilterDriverPort`。

**访问控制**:`IoCreateDeviceSecure` + SDDL **`D:P(A;;GA;;;SY)`** — 保护性 DACL,仅 `LocalSystem` 拥有 `GENERIC_ALL`。

实测:
| 身份 | `CreateFileW(\\.\WinMsrDev, GENERIC_RW)` |
|---|---|
| 管理员 (Administrators) | `err=5` ACCESS_DENIED |
| `nt authority\system` | `OPEN_OK` |

**结论**:该通道的合法调用者身份是 LocalSystem。以 SYSTEM 服务运行即符合设备 ACL,不是绕过安全控制。

**导入的能力原语**(实现用途,非漏洞):`MmMapIoSpace`/`MmUnmapIoSpace`、`HalGetBusDataByOffset`(PCI 配置空间)、`PsSetCreateProcessNotifyRoutineEx`、`PoRegisterPowerSettingCallback`。

---

## 3. MSR/物理内存访问通道 — IOCTL 分派表

分派读取 `IO_STACK_LOCATION.Parameters.DeviceIoControl.IoControlCode`(`[irpSp+0x18]`),精确比较下列 8 个值:

| IOCTL | 设备 | 功能 | 方法 | 访问 | 输入 | 输出 |
|---|---|---|---|---|---|---|
| `0x9C402000` | 0x9C40 | 0x800 | BUFFERED | ANY | 无 | 4 |
| `0x9C402004` | 0x9C40 | 0x801 | BUFFERED | ANY | 无 | 4 |
| `0x9C4021C0` | 0x9C40 | 0x870 | BUFFERED | ANY | 无 | 16 |
| `0x9C4021C4` | 0x9C40 | 0x871 | BUFFERED | ANY | ≥12 | 变长 |
| `0x9C4021C8` | 0x9C40 | 0x872 | BUFFERED | ANY | =12 | ≥count×size |
| `0x9C4021CC` | 0x9C40 | 0x873 | BUFFERED | ANY | 4 | 8 |
| `0x9C4021D0` | 0x9C40 | 0x874 | BUFFERED | ANY | 12 | 4 |
| `0x9C4021D4` | 0x9C40 | 0x875 | BUFFERED | ANY | 12 | 4 |

**关键语义**:输入首个 `u32` 是**选择子(0..5)**,不是任意 MSR 索引。选择子 >5 → `STATUS_INVALID_PARAMETER`。
即供应商把可读写集合限制为预定义 MSR 集合,不支持任意 MSR 写入。

### 3.1 选择子 → MSR 映射（驱动硬编码，0x140001C25 起）

| 选择子 | MSR | 名称 | 可写性判断 |
|---:|---|---|---|
| 0 | `0x19B` | `IA32_THERM_STATUS` | 状态寄存器,写仅用于确认/清除位 |
| 1 | `0x19C` | `IA32_MISC_ENABLE` | **含 Turbo 禁用位(bit 38),禁止整字写入** |
| 2 | `0x1B0` | `IA32_ENERGY_PERF_BIAS` | 位 3:0 为 EPP,可安全读改写 |
| 3 | `0x772` | `MSR_HWP_REQUEST_PKG` | 封装级 HWP 请求,可读改写 |
| 4 | `0x774` | `MSR_HWP_REQUEST` | 核级 HWP 请求,可读改写 |
| 5 | `0x65C` | `MSR_PLATFORM_POWER_LIMIT` | PL1/PL2 平台功率限制,可读改写 |

### 3.2 平台门禁（`0x9C4021D4` = NotifyPlatformInfo）

```
BOOL NotifyPlatformInfo(PVOID info8)
  in8 = { u32 platformId, u32 cookie }
  DeviceIoControl(h, 0x9C4021D4, in8, 8, out4, 4, &ret, NULL)
```
驱动端逻辑（0x140001C0E）:`platformId ∈ {0x33, 0x35, 0x36}` → 写入上表;**否则整表清零**,所有选择子返回 `STATUS_INVALID_PARAMETER`。
第二个 `u32` 与内部 4 字节常量比较,仅影响另两个全局,不影响选择子表。

**实测（SYSTEM 身份，只改驱动内部白名单，不写硬件）**:

| platformId | 选择子 0..5 |
|---:|---|
| 0x33 / 0x35 / 0x36 | 全部 `OK`,返回真实 MSR 值 |
| 0x37 / 0x38 / 0x00 | 全部 `err=87` |

因此实现必须先发 `NotifyPlatformInfo` 并**以读回验证**确认通道可用,不得假定平台 ID。

### 3.3 实测读回值（2026-10-30，21VG）

| 选择子 | 原始 64 位值 | 解读 |
|---:|---|---|
| 0 (THERM_STATUS) | `0x0000000000000010` | bit 4 置位 |
| 1 (MISC_ENABLE) | `0x0000000088250008` | bit 38 = 0 → Turbo **未被**禁用 |
| 2 (ENERGY_PERF_BIAS) | `0x000000008000FF01` | 位 3:0 = 1 |
| 3 (HWP_REQUEST_PKG) | `0x0000019E7F002501` | min=0x01 max=0x25 desired=0x00 EPP=0x7F |
| 4 (HWP_REQUEST) | `0x000081E0005B81E0` | min=0xE0 max=0x81 desired=0x5B EPP=0x00 |
| 5 (PLATFORM_POWER_LIMIT) | `0x0000000000000009` | PL1=9 单位,PL1 使能位=0 |

注:选择子 3/4 的 min/max 字段出现 min>max 的组合,说明本平台 HWP 编码与通用 SDM 语义不完全一致,**写入前必须以本机读回值为基线做读改写**,不得套用通用位域假设。

---

## 4. MSR/物理内存访问通道 — 用户态调用契约

导出:`InitializeMSRIO`/`DeinitializeMSRIO`、`RdmsrCollection`、`WrmsrCollection`、`ReadPhysicalMemoryCollection`、`WritePhysicalMemoryCollection`、`NotifyPlatformInfo`、`TconInit`/`TconRead`/`TconWrite`/`TconWriteRead`/`TconUninit`。

### 4.1 MSR 设备打开

`\\.\WinMsrDev`:`CreateFileW(path, 0xC0000000 /*GENERIC_READ|WRITE*/, 0 /*share none*/, NULL, 3 /*OPEN_EXISTING*/, 0x80 /*FILE_ATTRIBUTE_NORMAL*/, NULL)`。
备用路径 `\\.\MSRIO.sys`(本机走 WinMsrDev 分支)。

### 4.2 `RdmsrCollection`

```
BOOL RdmsrCollection(ULONG selector, ULONG *low, ULONG *high)
  DeviceIoControl(h, 0x9C4021CC, &selector, 4, out8, 8, &ret, NULL)
  *low  = out8[0..4];  *high = out8[4..8]
```
返回 64 位值 = `((u64)high << 32) | low`。

### 4.3 `WrmsrCollection`

```
BOOL WrmsrCollection(ULONG reserved0, ULONG reserved1, ULONG selector, ULONG low, ULONG high)
  in12 = { selector, low, high }
  DeviceIoControl(h, 0x9C4021D0, in12, 12, out4, 4, &ret, NULL)
```
前两个参数在函数体内未被使用(仅保存到栈)。

### 4.4 Tcon(热控)通道

设备 `\\.\SPBTESTTOOL`,重叠 I/O 打开:
`CreateFileW(path, 0xC0000000, 3 /*READ|WRITE*/, NULL, 3 /*OPEN_EXISTING*/, 0x40000000 /*FILE_FLAG_OVERLAPPED*/, NULL)`。

| 函数 | 契约 |
|---|---|
| `TconInit()` | `DeviceIoControl(h, 0x4001C00, NULL, 0, NULL, 0, &ret, &ovl)` + `GetOverlappedResult(wait)` |
| `TconWrite(ULONG len, void *buf)` | `WriteFile(h, buf, len, &written, &ovl)` + `GetOverlappedResult(wait)` |
| `TconRead(ULONG len, void *buf)` | `ReadFile(h, buf, len, &read, &ovl)` + `GetOverlappedResult(wait)` |
| `TconWriteRead(ULONG inLen, void *in, ULONG outLen, void *out)` | `DeviceIoControl(h, 0x4001C10, in, inLen, out, outLen, &ret, &ovl)` + 等待 |

**目标机实测**:`\\.\SPBTESTTOOL` 不存在(`err=2` FILE_NOT_FOUND)。Tcon 通道在本机型**不适用**。

---

## 5. DPTF/ESIF 性能框架

导出:`InitialConnection*`(`_Win32`/`_LNVDES`/`_ML`/`_ToolKit`)、`CheckConnection`、`DisConnection`、`GetCpuTempValue`、`GetSystemPowerValue`、`GetDefaultPLX`、`RestoreDefaultPLX`、`SendOEMV`、`SendOEMVUpdate`、`SendOEMV_GROUP`、`UpdtePowerLevel`(`4`/`NoTAU`/`_Platform`/`mw`/`mw2`)、`setCallBackFuntion`。

架构证据:字符串 `ESIF_I_ACPI_TRIP_POINT_NOT_PRESENT`、`ESIF_E_MSR_IO_FAILURE`、`ESIF_I_MSR_AFFINITY`、`\Intel\IPF` 注册表键、`ipfcore.dll`、`pipe://ipfsrv.public`、`pipe://ipfsrv.elevated`。
→ 实际 PL 控制在 `LNVDispatcherService.exe` 内,经命名管道 + ESIF/DPTF。

**目标机实测**:`\\.\pipe\ipfsrv.public` 与 `\\.\pipe\ipfsrv.elevated` **均不存在**;`LNVDispatcherService.exe` 正在运行但未创建这两个管道(版本 3.2.0.16,与 IPF 3.2.0.17 不同源)。
→ 该 RPC 路径在本机不可用,不实现。

---

## 6. 已排除的误判通道

| 通道 | 实测 | 结论 |
|---|---|---|
| `\\.\Lenovo0` / `\\.\Lenovo1` | `err=2` | 由 `lenovoDriverBus.sys`(WDF VHCI 虚拟总线)注册,仅用于枚举虚拟 HID/显示子设备,**非硬件控制** |
| `SmartSAMD.sys` | 版本 1.50.1.0「Storport Miniport Driver for SmartRAID/SmartHBA」 | 存储驱动,**与风扇/充电/性能无关** |
| `\\.\MSRIO.sys` | 客户端备用路径;设备由同一驱动注册 | 与 `\\.\WinMsrDev` 同源,本机走 WinMsrDev |

---

## 7. 规格与验证状态核对

| 规格条目 | 二进制证据 | 结果 |
|---|---|---|
| 设备名 `\\.\WinMsrDev` | 驱动 `\Device\WinMsrDev` + `\DosDevices\WinMsrDev`;客户端 `\\.\WinMsrDev` | 一致 |
| 仅 SYSTEM 可访问 | 驱动 SDDL `D:P(A;;GA;;;SY)`;实测 admin `err=5`、SYSTEM `OPEN_OK` | 一致 |
| 微过滤器已加载 | `fltmc filters`:LnvMSRIO / 6 实例 / 370039;服务 Running/Auto | 一致 |
| `0x9C4021CC` RDMSR | 驱动分派 `cmp ...,0x9C4021CC` == 客户端 `mov edx,0x9C4021CC` | 逐字一致 |
| `0x9C4021D0` WRMSR | 驱动分派 `cmp ...,0x9C4021D0` == 客户端 `mov edx,0x9C4021D0` | 逐字一致 |
| 输入 4 / 输出 8 | 客户端 `r9d=4`、`nOutBufferSize=8`;驱动读 `*(u32*)inBuf` | 一致 |
| 选择子 0..5 | 驱动 `cmp [rsp+0x24],5; ja invalid` | 一致 |
| Tcon `0x4001C00/0x4001C10` | 客户端 `edx` 立即数 | 一致（设备不存在） |
| 版本同源 | sys/dll/IPF 均 3.2.0.17,PDB 同属 `process_management` | 一致 |
| `SmartSAMD` 为联想控制 | 版本资源为 SmartRAID 存储 miniport | **规格修正** |
| `Lenovo0/1` 为 EC 控制 | WDF VHCI 虚拟总线 | **规格修正** |

---

## 8. 选择子语义（实测）

（见下表，来自 SYSTEM 身份下的只读调用）

映射已在 §3.1 由驱动硬编码确定，读回值见 §3.3。写入位域语义尚未逐个验证，
因此实现不写这些选择子（见 §9.3）。


---

## 9. HAL 跨平台映射（Linux 用主线内核，Windows 用 OS 原生 API）

**决策**:CLI 操作硬件**不得经过联想专有层**。因此 `\\.\WinMsrDev` / `WinMSRIO.dll` /
`LNVDispatcherService.exe` 只作为"为什么 Windows 缺通道"的证据,不作为实现路径。

| 能力 | Linux（主线内核 ABI） | Windows（OS 原生） | 状态 |
|---|---|---|---|
| 性能模式 `perf.mode` | `ideapad/thermal_mode`（`ideapad_laptop`） | **Power overlay scheme**（`powrprof!PowerSetActiveOverlayScheme`） | 已实现 |
| EPP `tune.epp` | `cpufreq/energy_performance_preference`（`intel_pstate`） | Power 方案 EPP 设置（`PowerWriteACValueIndex`） | 已实现 |
| PL1/PL2/tau | `powercap/intel-rapl:0/constraint_*`（`intel_rapl`） | **无 OS 接口**；仅厂商驱动暴露 MSR | 诚实不可用 |
| 风扇模式 | `ideapad/fan_mode`（`ideapad_laptop`） | **无 OS 接口** | 诚实不可用 |
| 温度 | `class/hwmon/*/temp*` | ACPI 热区（WMI） | 已实现 |
| 充电阈值 | `power_supply/BAT0/charge_control_end_threshold` | 固件 WMI（需写门禁） | 部分 |

### 9.1 Windows 性能模式的实现契约（`crates/lctrl-hal-win/src/power_overlay.rs`）

| 模式 | overlay GUID | 依据 |
|---|---|---|
| Balanced | `00000000-0000-0000-0000-000000000000` | SDK 平衡方案 |
| Quiet | `961cc777-2547-4f9d-8174-7d86181b8a7a` | 最佳能效 |
| Performance | `ded574b5-45a0-4f42-8737-46345c09c238` | 最佳性能 |
| Geek / SilentHighPerformance / Custom | — | **拒绝**（无 OS 等价语义，不近似替代） |

写路径规则:先读 `PowerGetActualOverlayScheme` 作为前值 → dry-run 不写 → 提交后
读回校验 → 不一致则回滚到前值并报 `VerifyMismatch`;前值不是本实现拥有的 scheme 时
**在写之前拒绝**（无法回滚的变更不做）。

### 9.2 实现 ↔ 规格对照（更正记录）

| 项目 | 接口规格 | 实现 | 一致性 |
|---|---|---|---|
| `perf.mode` 依赖 | 原能力行声明 `LenovoProcessManagement`/`Win32Scm`/`LinuxSysfs` 为必需 | 改为 `Win32PowerApi` 必需,`Win32Registry`/`LenovoProcessManagement` 降为可选（仅读回显示）,移除 Windows 行里的 `LinuxSysfs` | **已更正** |
| `perf.mode` 回读 | 原为 `Unverified` | 改为 `Required`（overlay 读回校验） | **已更正** |
| `perf.mode.set` | 原返回 `Unsupported` | 经 overlay 实现,含 dry-run/回滚/读回 | **已更正** |
| `perf.mode` 能力值 | 原静态表硬编码 `Unavailable`（且覆盖先前的动态记录） | 移除该静态条目,改为按 `PowerGetEffectiveOverlayScheme` 探测,记录 `Limited` | **已更正** |
| `powrprof` 链接方式 | 曾用 `#[link(name="powrprof")]` | SDK 的 `powrprof.lib` 不导出该符号 → 改为运行时 `GetProcAddress` 解析,缺失时诚实报不可用 | **已更正** |
| PL1/PL2/tau | 曾拟用 `\\.\WinMsrDev` | 2026-10-31 决策后改为：Linux 走 `/dev/cpu/N/msr`;Windows 自有签名驱动未落地前保持不可用(原因:Secure Boot+VBS/HVCI 需 EV 签名,见 §9.3) | 按决策更正 |

### 9.3 Windows 的 MSR 通道现状(2026-11-01 按项目决策更新)

1. 设备 ACL 仅 `LocalSystem`,合法调用者身份是系统服务;
2. 选择子集合固定为 6 个 MSR,由驱动按平台 ID 白名单启用;
3. ~~该驱动为联想签名专有组件,使用即"经过联想专有层",与实现约束冲突~~
   (2026-10-31 决策已取消该约束;经联想签名驱动调用在允许范围内,但受 1、2 限制);
4. Windows 不向用户态暴露 MSR;目标机 Secure Boot ON + VBS/HVCI running,
   自建 Rust 内核驱动需 EV+WHQL 签名(test-sign 不可行)。

结论:Windows 上 PL/风扇写路径二选一——(a) 加载联想 LnvMSRIO 驱动并以 SYSTEM
身份服务代理 IOCTL(受选择子白名单限制,见 §3.1);(b) 分发自签名+EV 路径的自有
Rust 驱动(需证书,见 §3 门禁)。未落地前能力行保持 `unavailable` 并注明具体原因;
Linux 上同类能力由主线内核 `/dev/cpu/N/msr` 完整提供(见 `09-linux-backend.md`)。

## 10. GAMEZONE WMI 接口面(root\wmi)

### 10.1 提供者拓扑

- `LENOVO_GAMEZONE_DATA` / `LENOVO_FAN_METHOD` / `LENOVO_OTHER_METHOD` / `LENOVO_UTILITY_DATA`
  注册在 `ACPI\PNP0C14\GMZN_0` 上,宿主是微软标准 **WmiProv**(`wbem\wmiprovse.exe`,
  `wmiprov.dll`)——纯 WMI→ACPI 胶合层,方法语义全部在 BIOS ACPI(固件 GUID 对应的
  WMxx 控制方法)。**没有任何厂商内核驱动参与**:固件 GUID 不在磁盘上任何 .sys 里
  (已扫全 DriverStore)。HAL 客户端不依赖厂商驱动即可调用。
- 21VG 上仅 GAMEZONE_DATA / OTHER_METHOD / UTILITY_DATA 各有 1 个活实例;
  FAN_METHOD / CPU / GPU / MEM / PANEL / LIGHTING 实例报"不支持"——GAMEZONE 是
  拯救者硬件特性,ThinkBook 只暴露同一提供者骨架。

### 10.2 调用形态

- 全部为普通 WMI ExecMethod;root\wmi 实例级调用。setter 收单个
  VT_I4/u32 `Data`(`SetIGPUModeStatus` 形参名 `mode`);布尔路径
  (`Fan_Set_FullSpeed` 除尘)用 VT_BOOL(u32 1/0)。
- `LENOVO_OTHER_METHOD.GetFeatureValue(IDs: u32)` 返回 CIM_UINT32(0x1A),
  与 `value: u32` 的 `SetFeatureValue` 配对。
- 相关方法名与调用封装出现在厂商的用户态组件中(如 GameSettingsPlugin.dll /
  WSHardwarePlugin.dll / lde_module_fan.dll / TconSDK.dll);典型调用路径如
  `AcWMI::ExecMethodWMI`(Game/AcWMI.cpp)与 `FunControlHelper.hpp`。

### 10.3 常量

- SmartFan 模式值:**先前"0..3 四值"判断被修正**。WSGameSettingPlugin.dll 中
  真正的模式跳转表属于 `GetFnQModeEx(type)` getter(type 1..4),不是 setter;
  `DealWithSMfan` 直接操作**固件原始编码**,出现
  `cmp eax,0x7A` / `push 0x80; cmp ebx,0x80` 阈值比较(0x7A/0x80 语义未完全解析,
  可能是钳位阈值而非模式值)。唯一确证的语义:`RecordMode==2 && fnq==3` 为
  性能/电源特判。**实现中的 standard=0/silent=1/performance=2/custom=3 映射
  未获确认且部分被驳**——本机 provider 全拒、无法真机校验,保留仅作占位并须
  按未知处理。
- FeatureId 布局:`(family << 16) | index`,**bit31 = supported 标志**。
  lde_module_fan.dll(x64)有 `{u32 flag, u32 id}` 对表(stride 8,
  flag 0 为终止符),紧邻 `IDs`/`value`/`LENOVO_OTHER_METHOD` 字符串,旁侧
  `LENOVO_FAN_TEST_DATA`/`NumOfFans`/`FanMaxSpeed` → **风扇域 ID 表**:
  `0x8002FCF0, 0x8002FBC0, 0x8002FBE0`(族 0x2F)、`0x80040320`(0x403)、
  `0x80041A90`(0x41A)、`0x800414A0`/`0x80041430`(0x414)、
  `0x80040E40`/`0x80040E50`(0x40E)、`0x80040DF0`(0x40D),终止符 `{0, 0x05040000}`。
- **ID ↔ 特性名映射未恢复**(表项无名称;调用经间接调用传 ID,预算内无法 xref)。
  WSHardwarePlugin 的充电走**注册表**机制
  (`SOFTWARE\Lenovo\VantageService\AddinData\IdeaNotebookAddin` 值 `BatteryChargeMode`,
  Storage/Quick/Normal)——另一子系统。GAMEZONE 侧只有 `GetPowerChargeMode` 读。
- 热模式 ID:无字符串证据把表内任何 ID 绑到 thermal/perf。
- 已知命名特性:`Get_Support_LegionZone_Version`(OTHER_METHOD,不支持时默认 2)。

### 10.4 21VG 实测限制(客户端必须按此处理)

- 本机**每一个** GAMEZONE 方法(包括 `GetVersion`)在实例级与类级调用都返回
  `WBEM_E_INVALID_METHOD_PARAMETERS` / "Invalid object"。原因未查明(PowerShell
  与 Rust 客户端行为一致),推测固件按机型/调用方校验拒绝。**客户端必须把它当
  "该固件不支持"处理,而不是调用形态错误**——当前实现映射为
  `LctrlError::Unsupported`,能力行动态探测如实报 Unavailable。
- FAN_METHOD 常量无法在本机验证(实例不存在),按 §10.2 形态实现、按 §10.4 报错。

## 11. 充电域接口契约(EnergyDrv)

二进制链:`LenovoVantage-(IdeaNotebookAddin).exe → IdeaNotebookAddin.dll → IdeaBatteryAgent.dll(BatteryAgent RPC 面)→ PowerBattery.dll(native,按序数导出)`。设备 `\\.\EnergyDrv` 由 `AcpiVpc.sys` 创建(服务 `acpivpc`,demand-start;用户态可开,无需 SYSTEM)。所有 IOCTL 输入输出校验以驱动 Dispatch 为准:`InputLength≥1`(USB)/`≥4`(夜间),`OutputLength≥4`,否则 `STATUS_BUFFER_TOO_SMALL`。

### 11.1 充电模式(GBMD,`0x831020f8`,1B 子命令)

| 子命令 | 语义 | 证据 |
|---|---|---|
| `0xFF` | 状态位掩码查询 | 实机 `0x00860004`(bits 2,17,18,23) |
| `0x03` | 打开 storage(养护)的基础请求 | CStorageMode vftable `0x180017e78` 的 `+8`，RVA `0xc3d0` |
| `0x05` | 关闭 storage 的基础请求 | 同一 vftable 的 `+0x10`，RVA `0xc580` |
| `0x0d` / `0x0f` | subtype 2 的开/关追加请求，不是基础请求替代品 | 仅 `03` / `05` 成功且构造时 GBMD bit14 置位才追加 |
| `0x07` | 打开 express(快充) | CExpressMode vtable 槽位 +8 |
| `0x08` | 关闭 express | 同一 vtable 的关闭槽位 |

**读回解码**：bit5=storage 开 → Storage；否则 bit2=express 开 → Quick；否则 Normal。
安装 x64 DLL 的 storage IsSupport 只检查驱动能否打开；express 支持=bit17。
构造函数 `0x9790` 查询 GBMD `[ff]`，bit14 清零为 subtype 1（`03/05`），置位为
subtype 2（成功门控的 `03→0d` / `05→0f`）；不能按 bit23 分发。
`SetChargingMode` 原生 mode 为 0=Normal、1=Storage、2=Quick，最多按该状态逻辑
查询 10 轮，不匹配时 Sleep(50ms)，仅读回匹配目标时写注册表缓存；底层状态查询失败
不应被客户端当成 Normal 成功。

**Storage80 元数据**：bit23 仅是 advertised 位，不是完整能力。原版
`DoesSupportStorage80`（RVA `0xaa80`）还要求有效的 `GetEmSpecVersion() >= 184`。
版本查询（RVA `0x22d0`）发 `0x8310214c`，输入 DWORD `0x11`、输出 4B raw；
IOCTL 失败或 raw bit0 清零时返回 `-1`，否则版本=`raw >> 4`。最小版本静态 DWORD
位于 `.data` RVA `0x21174`，对应文件偏移 `0x1ef74`，字节 `b8 00 00 00`。
旧探测输入4B零/raw=`0x00000002` 不是原版请求，不能解读成版本 2 或完整能力真；
原生形态的有效版本读数未获得，完整 Storage80 能力仍未知而不是已证明不存在。

**安全/效果边界**：当前 DLL 的 RTTI 与方法体确认 `03/05` 归 Storage，`07/08`
归 Express；旧文档对 `03` 的运输模式禁令没有绑定到此路径的关机证据。目标原生
consumer/生产 CLI 已有受限模式状态与恢复观察，见
`docs/evidence/storage80-aiturbo-re-20261102.md`、
`docs/evidence/conservation-cli-native-20261102.md` 和 C2。不得把这改写成所有固件/
电源条件安全或 75%–80% 物理停充点已验收；独立官方 UI、物理门限、持久化均另论。

**对 §10.3 与 docs/01 的更正**:
- 注册表值实为 `HKCU\SOFTWARE\Lenovo\VantageService\AddinData\IdeaNotebookAddin\BatteryChargeMode`,**REG_EXPAND_SZ**,取值字符串 `Normal`/`Storage`/`Quick`(HKLM 键不存在)。它是**只写透传缓存——二进制内无任何读取方驱动行为**,硬件位掩码才是权威状态。vantage 不写该值。
- docs/01 §3.3 对 `0xFF` 的旧位注释(bit24=认证充电器, bit15-16=Inbox 适配器)与文档旧注释不符,以本节位表为准(bit2/bit5/bit17/bit23 已证;bit14 语义未决;bit18 未识)。

### 11.2 USB 充电控制(`0x831020e8`,1B 选择子)

| 选择子 | 语义 | 输出 |
|---|---|---|
| `0x02` | 标志字查询 | bit6=AC 支持, bit7=AC 开, bit14=电池模式支持, bit15=电池模式开 |
| `0x0a`/`0x0b` | AC AlwaysOnUSB 开/关 | 空输出(Information=4) |
| `0x13`/`0x12` | 电池模式开/关 | 空输出 |

状态查询槽(`OpenOrClose`,+0x20)用于读回,写后按位条件重查(最多 10 次)。`IsSupportOnARM` 恒 0。**对 docs/02 §6 的更正**:`+0x32` 不是函数槽(该版本 vtable:+8 Open, +16 Close, +24 IsSupport, +32 OpenOrClose=status);旧资料 "同族命令字[推断]" 已解决为同一 IOCTL 0x0a/0x0b。实机 `0x02 → 0x0000CEC0`:两种模式均支持且**均开启**(bits 7/15;此前人工读数"关闭"有误,以本次 CLI 解码为准,与 BIOS AlwaysOnUSB=Enable 一致)。

### 11.3 夜间充电(`0x83102150`,4B 选择子)

`0x13` 支持探测(响应 bit0=支持);`0x11` 状态(bit4=开);写 `0x80000012`=开、`0x12`=关。**实机更正(2026-11-01)**:同机探测回 `0x11` 而状态回 `0x01`——探测响应 bit4 并非当前状态(此前[推断]有误);权威状态只取 `0x11` 查询。实机状态:支持,当前关。写响应按 4B 读取(内容不解释),写后以 `0x11` 轮询验证(与 GBMD 同轮询界)。

### 11.4 临时充电模式(`0x83102148`,4B 选择子)

`0x13` 支持探测、`0x11` 状态读。位语义沿用历史组件契约(docs/02 §5):bit0=Permanent, bit1=PlugIn, bit2=DynamicLifeSpan;PlugIn/DynamicLifeSpan 置位即临时模式。本机未发现写方(写选择子按夜间充电同型试探无对应代码路径)——只读探测。

### 11.5 DBDC(`LENOVO_REPORT_DBDC_DATA`,WMI 只读)

Counts/CurrentLimit[](mA)/ROS_Power[](mW)/Threshold[](%),实机 3 档 [7500,5000,4500]/[100,40,20]。写路径(`SetFeatureValue` + DBDC FeatureId)因 ID↔名映射未恢复保持门禁(§10.3 表)。

## 11.4 / 11.6 修正与阈值写入路径结论

**11.4 修正**:PowerBattery.dll 的 temporary-mode 只读选择子为 0x23/0x21/0x31/0x41;§11.4 原记的 0x13/0x11 实为读回 3/1(实机)。所分析客户端未发现对应写入调用,但不能证明固件没有其他写口;写选择子尚未恢复,保持未准入。

**11.6 自定义充电阈值**:已分析到 BatterySetting.exe → WrapPlugin.dll → ThinkPowerPlugin.dll → PWRMGRV/PWMIF32V 的 ThinkPad 路径,其读回为注册表。21VG 上观察到 PWMIF32V.DLL 缺席、PWRMGRV 键空、所测 MHQI 方法返回 win32=2;因此此已知路径不可用,但这些负向结果不能证明不存在任何其他写入通道。当前命令保持 Unsupported。

**驱动 IOCTL→ACPI 全映射**(AcpiVpc.sys 15.11.30.14,驱动仅透传 4 字节选择子,写门控全在固件):0x20e8 SALS/HALS、0x20f8 GBMD、0x211c/0x2138 ITHC、0x2120 GBID、**0x212c SMTF(可写透传,语义未恢复,探测返回 0xFFFF→门禁)**、0x2130/0x2134 MHQI 读、0x2144/58/5c KBL*、**0x2148 BTMC**、0x214c BSIF、0x2150/0x2154 BNMC。

### 11.7 极限续航（AiTurbo：完整运行态策略仍未替代）

完整静态证据、精确 GUID/恢复 JSON、选择目录及剩余前置条件见
[extreme-life-re-20261102.md](evidence/extreme-life-re-20261102.md)。官方 AI Turbo
1.0.0.12311 包与留存的 BatteryLife/AiPerfSvc/Workman/LseFileMonitor 逐字节匹配。

原版没有固定省电方案 GUID：它捕获当前活动方案，改三个 DC 索引（无线 3、幻灯片 1、
PCIe 链路 2）再重应用同一方案。配置及 `PowerSchemeSetting` 恢复 JSON 位于
**HKCU\Software\Lenovo\AiTurbo\BatteryLife**，不是 HKLM。`Enable` 和
`EnableEcoQos` 独立处理；`WinJobList` 的已定位引用仅是停机时删除该键，不能据此称为
已恢复的进程名单结构或猜造它的值。

完整激活另受版本化 ITS/AC 来源和八项常驻策略控制，包含 Job CPU 硬上限、窗口最小化、
关闭和进程终止，并非仅 EcoQoS 或充电阈值。内核执行器的 rate=80 是 class15
`ENABLE|HARD_CAP` 的 0.8%，其同步进程创建回调负责父子 job 迁移；完整替代仍需项目
自己拥有且适当签名的内核生命周期实现。原端口另有联想 ECDSA-P256 准入，不能以普通
Windows 签名或新客户端复用；原 DLL/服务/驱动均不作为准入运行时依赖。

公开 Windows API 足以表达单次方案/Job/EcoQoS 操作，不足以把一个通用节流循环宣称为
完整开关。真实单个设置 UI 的写入映射、完整守护及独立原生效果/恢复验收仍是前置条件，
当前 `battery extreme-life` 保持 Unsupported；不宣称 60% 停充或已经增加续航。

## 12. Dolby 音频控制接口(DAX RPC)

链路:`vantage audio dolby → 纯 Rust rpcrt4 NDR 客户端 → ALPC ncalrpc:[DaxRpcEndpoint] → DAX3API.exe(DolbyDAXAPI 服务,SYSTEM)`。接口 GUID `11c6212d-d5f3-4500-ab16-634dc63037be` v1.0,NDR v2.0,`RPC_IF_AUTOLISTEN | RPC_IF_ALLOW_LOCAL_ONLY`(仅本机);Object UUID `B94BA572-658F-40E6-861D-DFB7BB4001A8`。许可 APO DSP(DolbyAPOv*/DolbyAPOAIDE)只被 DAX3API 内部使用,vantage 只切换开关/预设,不重实现 DSP。

**客户端依赖边界**:原实现绑定 WindowsApps 包内的 `DolbyRpcClient.dll` 并复制到 `%LOCALAPPDATA%` 缓存;当前源码已删除该装载和缓存。RPC 客户端使用系统组件 `rpcrt4.dll` 与 `oleaut32.dll`,接口描述符按恢复出的签名生成(见 12.4)。这不消除服务端依赖:调用仍需可用的 `DolbyDAXAPI` / `DAX3API.exe`;卸载 Vantage 后该服务是否保留尚未验证。

### 12.1 操作(opnum)与语义

| RVA | 导出 | 签名 | RPC opnum |
|---|---|---|---|
| `0x74e0` | `DolbyClient_GetDolbyEnabled` | `fn() -> bool` | 5 (u16 out) |
| `0x74f0` | `DolbyClient_SetDolbyEnabled` | `fn(u8) -> bool` | 6 |
| `0x7510` | `DolbyClient_GetActiveProfile` | `fn() -> u32` | 7 |
| `0x7520` | `DolbyClient_SetActiveProfile` | `fn(u32) -> bool` | 8 |
| `0x7540` | `DolbyClient_Initialize` | `fn() -> bool` | 0/握手 |
| `0x7660` | `DolbyClient_GetCurrentMode` | `fn() -> i32` | 无(缓存字段 +0x10) |
| `0x7670` | `DolbyClient_IsSpatialAudioSupported` | `fn() -> bool` | 2 |
| `0x7700` | `DolbyClient_GetSpatialAudioStatus` | `fn() -> u32` | 35 |
| `0x7710` | `DolbyClient_WaitForCallBack` | 回调注册 | 3 |
| `0x7720` | `DolbyClient_GetSKU` | `fn() -> u32` | — |

out 参数全部在 wrapper 栈帧内分配;调用方只需按上表传参。`Set*` 的 bool 返回值恒为真,失败检测靠读回。

### 12.2 运行时行为(对照基线)

- (历史)WindowsApps 包路径 `C:\Program Files\WindowsApps\E046963F.LenovoCompanion_*\LenovoVantage\DolbyRpcClient.dll`;其 DACL 阻止原地 `LoadLibrary`(实机探针证实),须先复制到可写目录 —— 该复制/缓存机制已随 DLL 依赖一并删除。
- `DolbyClient_Initialize` 的 bool 返回不是就绪信号(实机探针返回 false,后续所有 RPC 均成功),调用但不判其值。纯 Rust 通道无此导出,等价地不判任何“就绪”返回值。
- 实机只读探针(ezra/管理员,非 Vantage 进程):`Initialize=0 GetDolbyEnabled=0 GetActiveProfile=5 GetCurrentMode=-1 IsSpatialAudioSupported=1 GetSpatialAudioStatus=2 GetSKU=1` —— Dolby 关、预设索引 5、空间音频支持。证明第三方进程可独立绑定该端点(非 SYSTEM 专属)。
- 预设索引表(打包名字表顺序,中置信):0=movie,1=music,2=game,3=voice,4=personalize,5=dynamic;服务器校验范围 [0,5]。`Off` 不是预设索引,走 `SetDolbyEnabled(0)`。
- 服务前置:`DolbyDAXAPI` 须 Running;标准用户(未提权)RPC ACL 未验证 —— 写操作经现有提权路径。

### 12.3 实现落点

- `lctrl-hal-win/src/dolby.rs`:`NativeDolbyRpc`(纯 Rust rpcrt4 NDR 客户端)+ `WindowsDolbyAudio<R: DolbyRpc>`(dry-run/commit + 读回轮询,`poll_readback` 10×50ms)。trait 接缝 `DolbyRpc` 与 `vantage-cli` 表面未变。
- `vantage/src/main.rs` Windows 组合根:audio 服务当前**不挂载**。2026-11-03 真机上该通道在 NDR 封送器内触发访问冲突(0xC0000005)并使整个 CLI 启动即崩溃,因此在描述符差异定位前一律摘除,`audio dolby` 如实报 Unsupported(见 `docs/evidence/native-acceptance-20261103.md`)。同机的 Elevoc 通道(§14)不受影响。
- 第二通道 `ncalrpc:[FusionRpcEndpoint]`(DolbyFusionRpcClient.dll,Atmos/Fusion 附加项)未接入,列为后续。

### 12.4 rpcrt4 通道契约(取代 DLL 绑定)

**ABI 稳定性依据**:锚点是 IfSpec 版本 `11c6212d-…` **v1.0** 与 NDR **v2.0**(DCE/RPC NDR32 封送规则,`NdrClientCall3` 由 rpcrt4 实现)。调用方与 DAX3API 之间只有这两项版本化约定;厂商 DLL 自身的导出签名不是契约,故可弃用。`rpcrt4.dll`/`oleaut32.dll` 是 Windows 系统组件,随系统更新而稳定。

**绑定与调用序列**(与原初始化次序逐条对齐):

1. `RpcStringBindingComposeW(NULL, "ncalrpc", NULL, "DaxRpcEndpoint", NULL, &binding)` —— 绑定串不带 object UUID(UUID 走 opnum 4 握手)。
2. `RpcBindingFromStringBindingW` → 会话 binding handle(懒绑定)。
3. `NdrClientCall3(proxy, 0, NULL, binding, &context)`:`[in] handle_t` binding → `[out]` 会话 context token;后续所有过程以该 token 作第 0 参数。
4. `NdrClientCall3(proxy, 4, NULL, context, bstr, &u16, bstr)`:`[in]` object-UUID BSTR → `[out]` u16;厂商忽略返回值(实现同样只报传输级失败)。

**各 opnum 的形参**(逐项确认;x64 ABI:第 1 个变参在 `r9`,其余在栈参槽,`proc string` 的 `stack_size` 给出槽数):

| opnum | 过程 | 形参(stack_size) | 语义 |
|---|---|---|---|
| 0 | OpenContext | binding + `&context`(0x10) | 建立会话 context |
| 2 | IsSpatialAudioSupported | context + 4×`&u32`(0x28) | `p0 > 3 ‖ p1 ≥ 0x7853` |
| 4 | RegisterObject | context + BSTR + `&u16`(0x18) | object UUID 握手 |
| 5 | GetDolbyEnabled | context + `&u16`(0x10) | on/off 读(0/1) |
| 6 | SetDolbyEnabled | context + `u8`(0x10) | on/off 写(返回恒真,靠读回判失败) |
| 7 | GetActiveProfile | context + `&u32`(0x10) | 预设索引读 [0,5] |
| 8 | SetActiveProfile | context + `u32`(0x10) | 预设写 |
| 37 | GetSKU | context + `&BSTR`(0x10) | SKU 名串(BSTR) |

**元数据**:MIDL 解释器表(proc format string / format offset / type format)由 `lctrl-hal-win/src/ndr/dolby.rs` 在编译期按公开的 NDR `-Oicf`(x64)格式规则,从上表恢复出的形参签名生成。当前源码不再内嵌原先逐字节转录的描述表;类型表仅保留本通道引用的 context handle 和 BSTR 语义。BSTR 参数经 `FC_USER_MARSHAL`(quad index 0)封送,quad 表绑定系统 oleaut32 的 `BSTR_UserSize/Marshal/Unmarshal/Free`。`-Oicf` 是 RPC 运行时的解释器元数据格式,不应仅凭线协议兼容就断言不同编译器生成的表必定逐字节相同。

**单语法决定**:`RPC_CLIENT_INTERFACE.Flags` 置 0(多语法位取值 `0x06000000` 会把 rpcrt4 引到 NDR64 分发路径,而本客户端只携带 NDR32 表,结果是 `RPC_S_INTERNAL_ERROR`(`0x6e6`)。置 0 后协商语法恒为 NDR v2.0。

**异常处理**:rpcrt4 用 SEH 异常上报传输失败。Rust 无 SEH,故每次调用包在 vectored handler + `RtlCaptureContext`/`RtlRestoreContext` 守卫内,把 RPC 状态异常转成 `ChannelUnavailable` 错误;非 RPC 异常(真实故障)不吞。

**实机只读实证**(2026-11-01,ezra/管理员,非 Vantage 进程,纯 Rust 通道):`connect=ok GetDolbyEnabled(op5)=0 GetActiveProfile(op7)=5 IsSpatialAudioSupported(op2)=1 GetSKU(op37)=1` —— 与 12.2 节对照基线逐值一致;`vantage audio dolby dynamic --dry-run` 输出 `previous enabled=false profile=Dynamic`。写路径(opnum 6/8)未在实机验证,仍走既有写入门禁。

## 13. SmartSense 人体感知设置接口(HPD RPC + 独立注册表读回)

### 13.1 当前 HPD 设置通道

链路:`vantage sense → 系统 rpcrt4/NdrClientCall3 → ncalrpc:[HumanPresenceDetectionEndpoint] → LnvSST.dll(UMDF)`。
接口 `{848A33AC-B5A7-4253-A54F-E0AC20648468}` v1.0;客户端只协商 NDR v2.0。
`sense_rpc.rs` 自有绑定/context 生命周期,`ndr/hpd.rs` 按语义签名生成解释器表;
不加载 Lenovo 交付的 SmartSenseRpcClient.dll,不内嵌逐字节抄录的 MIDL 表。
驱动/服务端仍是必要依赖,不能将「移除客户端 SDK 依赖」解释成可卸载感知驱动。

**当前状态(2026-11-03):**该通道在真机上调用时于 NDR 封送器内触发访问冲突
(0xC0000005),与 §12 的 DAX 通道同型,因此未挂载进服务图:`sense status` 返回
`HKLM\…\SmartSense\Parameters\HPD` 的持久化投影并标注 `source=hpd-registry-store`,
HPD 写入命令如实报 Unsupported,§13.2 的 privacy-alert / attention-tracking 旧管道
保持可用。描述符差异尚未定位,真机验收因此未完成。

| wire opnum | 过程 | 形参(均先传 context) |
|---|---|---|
| 0 | Initialize | 显式 handle_t binding → out context;后续复用该 context |
| 1 | ClientClose | in/out context;释放后再 RpcBindingFree |
| 4 | GetVersion | out u32 |
| 5 | GetCapability | out u64(完整 8 字节,不能按托管 enum 大小只给 4 字节) |
| 6 | GetAllSetting(V1/V2 同一 wire op) | 15×out u32 + out wchar_t**;第 15 个标量是数组元素数;始终按 MIDL allocator 释放数组 |
| 8 / 9 | WakeOnApproach / ApproachDistance | in u32:bool 0/1 或距离 0/1/2 |
| 11 / 12 / 14 | LockOnLeave / LeaveWait / LeaveDistance | in u32:bool 0/1 或枚举 0/1/2 |
| 16 | PauseVideo | in u32:0=off/1=on,**不是旧管道的 1/2** |

所有这些主过程返回 i64 **Win32 状态**,0 才成功(非 HRESULT 规则);RPC SEH 传输异常转成
ChannelUnavailable,非 RPC 程序故障不吞。读取按 op5 能力位投影 Option 字段,缺能力不是 false。
写入不在 RPC 错误后重放或回退到旧 HPD 管道;成功状态后仍须独立注册表读回所改字段。
源实现已切换,**本次未编译/运行,真实写入验收未据静态恢复自动升级为通过**。

客户端不设置 SSPI auth,但不代表服务端匿名放行:端点 DACL 与 RpcImpersonateClient 的
token/context 检查仍生效。恢复出的写权限可由 enabled Administrators、SYSTEM 或专用
capability 满足;普通未签名进程的非提权写入不作承诺。精确五项 ACE、逐个 wire op、
回调/保留调用边界与原生读/写/恢复步骤见 [HPD RPC 证据](evidence/hpd-rpc-re-20261102.md)。

### 13.2 历史设置管道(仅保留独立的 privacy-alert / attention-tracking 写入)

旧链路:`\\.\pipe\com.lenovo.smartsense.settings → SmartSense.exe(LocalSystem)`。
帧格式:`u32 code + u32 arg` 小端(legacy bool=0/1;camera/video 系 1=enable/2=disable),
每连接一请求;响应单个 i32(0=拒绝/非零=受理/-1=服务端异常)。下表是恢复出的历史命令表,
不是当前六项 HPD RPC 操作的备用通道;此前 HPD 管道返回 0、值未变的实机记录仍保留为失败证据。

厂商 `NamedPipeClientStream` 的 `Connect(3000)` 对应 `WaitNamedPipeW(...,3000)`;其
`TokenImpersonationLevel.Anonymous` 对应 `CreateFileW` flags
`SECURITY_SQOS_PRESENT|SECURITY_ANONYMOUS`(`0x00100000`)。2026-11-02 补齐了这两项
传输契约,以及所有非零响应按受理处理;保留两项 legacy 请求的编码仍有源码回归测试。
这些 parity 修复不预设服务会受理此前返回 0 的功能,仍须独立注册表读回实机验收。

| 命令 | code | 参数编码 | 功能 |
|---|---|---|---|
| ENABLE_SST | 0x00010101 | "True"→1 else 0 | SmartSense 总开关 |
| ENABLE_BLC | 0x00020101 | bool u32 | 背光控制 |
| SET_BLC_AUTO | 0x00030101 | u32 | BLC 自动 |
| ENABLE_MODE | 0x00040101 | bool u8 + i32 modeId(2=Browsing,3=Facedown,5=Walking) | 模式开关 |
| SET_BROWSING_TIME | 0x00050101 | i32 秒(分钟×60) | 浏览延时 |
| ENABLE_SCREEN_LOCK | 0x00060101 | bool u32 | 自动锁屏(旧 HPD) |
| ENABLE_APPROACH | 0x00070101 | bool u32 | 靠近唤醒 |
| SET_APPROACH_DISTANCE | 0x00080101 | i32 0/1/2 | Near/Middle/Far |
| ENABLE_PRESENTLEAVE | 0x00090101 | bool u32 | 离开检测 |
| SET_LEAVE_WAIT | 0x000A0101 | i32 0/1/2 | Short/Middle/Long |
| UI_INSTALL | 0x000B0101 | u32 | UI 安装标志 |
| ENABLE_VIDEOSTOP | 0x000D0101 | 1 enable/2 disable | 视频自动暂停 |
| ENABLE_CAMERADETECT | 0x000D0201 | 1/2 | 摄像头检测(肩窥隐私告警) |
| ENABLE_AUTOADJUST | 0x000E0101 | 1/2 | 距离自适应 |
| HPD_RESET_COMMAND | 0x000F0101 | =1 | 重置全部 HPD 设置 |
| HPD_SET_GLOBAL | 0x00100101 | 2=off/3=on | HPD 总开关 |
| SET_LEAVE_DISTANCE | 0x00110101 | i32 0/1/2 | Near/Middle/Far |
| ENABLE_CAMERAASSIST | 0x00120101 | 1/2 | 摄像头辅助 |
| ENABLE_OVERRIDEOSTIMER | 0x00130101 | 1/2 | 覆盖 Windows 关屏计时 |
| ENABLE_LEAVE | 0x00140101 | 0/1 | 离开锁 |
| SET_LEAVE_TIMER | 0x00150101 | i32 | 离开计时 |
| ENABLE_ATTENTION_TRACKING | 0x00160101 | 0/1 | 注意力追踪 |
| ENABLE_ATTENTION_TRACKING_TIMER | 0x00170101 | i32 | AT 计时 |
| ENABLE_ATTENTION_TRACKING_AC | 0x00180101 | 0/1 | AT 接电时 |
| ENABLE_ATTENTION_TRACKING_DIM | 0x00190101 | i32 | AT 调光级别 |
| ENABLE_FACE_SENSING | 0x001A0101 | 0/1 | 人脸感知 |
| ENABLE_APPROACH_EXTDISP | 0x001C0101 | 0/1 | 外接屏时靠近 |
| ENABLE_LEAVE_EXTDISP | 0x001D0101 | 0/1 | 外接屏时离开 |
| ENABLE_AD_EXTDISP | 0x001E0101 | 0/1 | 外接屏时自适应调光 |


独立读回:服务持久化到 `HKLM\SYSTEM\CurrentControlSet\Services\SmartSense\Parameters\HPD`
(REG_DWORD),addin 自身旧 getter 也直读该键。已建模 PresentLeave/LeaveDistance/LeaveWait/
Approach/ApproachDistance/Video(**实测 2开1关**,见下)/CameraDetect(1开2关)/Capability/SensorType/
FacialFeatureRegistered。RPC status 读只覆盖其确知字段,legacy privacy-alert/attention 字段
由注册表补充;写后对所改字段作 10 次、间隔 50ms 的有界独立读回,不拿不同来源的无关元数据比较。
**Video 解码更正(2026-11-03 实测)**:LnvSST 服务将 `SetVideoAutoPauseResume(arg)`
持久化为 `Video = arg + 1`——arg=0(off)→Video=1、arg=1(on)→Video=2,与 RPC status
`values[10]` 逐项一致。本机服务的 Video 持久化语义是 **1=off / 2=on**,不是旧文档的
"1开2关";此前按 `raw==1→true` 解码导致 RPC 写成功却被读回判为 mismatch。
CameraDetect 的 1/2 语义未实测更正,legacy 表原样保留。
AttentionTracking 命令存在(0x0016–0x0019),但本机无对应持久化键,维持明确的 write-only 边界;
不能把 RPC CameraAssist(op13)冒充肩窥 PrivacyAlert。
**legacy 管道实测(2026-11-03)**:对 `\\.\pipe\com.lenovo.smartsense.settings` 逐连接探测
0x00030101/0x00070101/0x000D0201/0x00100101/0x00160101(arg 1、2)全部返回 0(rejected),
包括曾在 addin 常数表中的命令;SmartSense.exe 1.0.8.5 分派表仅识别
0x10201/0x10401/0x10701/0x10801/0x10802/0x70101/0x80101/0x90101/0xa0101/0xb0101/
0xd0101/0xd0201/0xe0101/0x10803/0x20201/0x30201(且各自对 arg 域进一步校验)。本服务版本对
0x0d/0x0e 族亦按 arg 拒绝,说明 legacy 设置管道在该版本上整体不可用于写入;
privacy-alert / attention-tracking 的 legacy-pipe 写入路径维持诚实拒绝边界,HPD 覆盖项继续
走 RPC。感知管线(ToF + IR 摄像头)仍由驱动/服务
实现,本通道只改设置。

### 13.3 自动键盘背光(`sense kbd-light-auto`,维持 Unsupported)

2026-11-03 对 ThinkSmartSenseAddin.dll 1.0.0.77 的二进制取证修正了此前的"contract RPC 未恢复"
措辞:addin 以 `LPATSVC_AUTO_KBD_BACKLIGHT_REGISTRY_PATH` /
`LISS_AUTO_KBD_BACKLIGHT_REGISTRY_PATH` 两个常量分别指向
`HKLM\SYSTEM\CurrentControlSet\Services\LPlatSvc\Parameters\AutomaticKeyboardLight` 与
LITSSvc 对应路径——自动键盘背光的持久化目标是 **LPlatSvc / LITSSvc 两个服务的参数键**。
21VG 实机 `Get-CimInstance Win32_Service` 确认这两个服务**均不存在**(也无
`LITSSVC` 服务键)。写入无人消费的注册表键不构成原版行为(不平替),故 `sense
kbd-light-auto` 在该机型上没有可实现的写入语义,维持 Unsupported;理由已更新为
"无消费服务",不再是"contract RPC 未恢复"。若未来机型存在 LPlatSvc/LITSSvc,
addin 侧的 VantageService contract 调度契约(`CommandNameGet/SetAutoKBDBacklight`
等命令名)可再恢复。

## 14. 降噪接口(Elevoc RPC)

链路:`vantage audio noise-cancel → ALPC ncalrpc:[ElevocRpcEndpoint] → ElevocControlService.exe(LocalSystem)`。接口 GUID `{6F761170-1054-4339-A573-063DD08C7E16}` v4.0,NDR32;单 opnum-0 通用分发,方法名在 JSON 请求信封内(elevoc_rpc_protocol)。**实现为纯 Rust rpcrt4 客户端**(`lctrl-hal-win/src/noise.rs`,`NativeElevocRpc`):按 2026-11-01 依赖裁定,Lenovo 交付的 ElevocControlSDK_x64.dll 已禁止作为运行时依赖,DLL 发现/缓存机制移除;传输改为系统 `rpcrt4.dll` + `NdrClientCall3` 直打 opnum-0。

**客户端依赖边界**:当前源码不加载或复制任何厂商 SDK,也不内嵌外部描述表;客户端通过系统 `rpcrt4.dll` 调用。RPC 服务端仍是必需依赖:目标机实测 `ElevocService` 运行 `C:\Windows\System32\ElevocInstallDriver\ElevocControlService.exe`。卸载 Lenovo Vantage 后服务是否保留尚未验证;不能据此声称无需安装或保留该服务。

### 14.0 纯 Rust 通道实现要点(ABI 稳定依据)

- **绑定**:`RpcStringBindingComposeW(NULL,"ncalrpc",NULL,"ElevocRpcEndpoint")` → `RpcBindingFromStringBindingW`(与原版初始化次序一致;无 AuthInfo,服务端无鉴权注册)。
- **MIDL 表**:当前源码由 `lctrl-hal-win/src/ndr/elevoc.rs` 的生成器按恢复出的签名 `void opnum0([in,out,string,size_is(0x5000)] wchar_t *json)` 生成描述符,不再保存原先转录的格式串。过程头启用 `HasNewCorrDesc` 和两端 correlation checks;因此 sized-string 的四字节常量符合度描述符后还必须生成两字节 `/robust` 标志(常量采用 early correlation)。结构体根据公开 `rpcndr.h` 定义构建。接口 GUID+v4.0 与 NDR32 v2.0 是当前测试版本的绑定依据,不是未来兼容性保证:服务端可能保留同一版本号但更改语义,必须针对更新版本重新验证。
- **异常面**:原客户端以 `__try/__except` 将 RPC 异常换成返回码。复刻用 VEH+landing-pad(guard `NdrClientCall3`):异常线程还原到包装帧返回,异常码→ChannelUnavailable;正常路径零开销。
- **请求缓冲**:保留原有不超过 `0x3ff` 个 UTF-16 单元的请求限制;由于生成的 `[in,out,string,size_is(0x5000)]` 描述符允许更长响应,本地接收缓冲使用同一个 `0x5000` 单元上限,包括末尾 NUL。不声称此约束已验证恶意 RPC 服务端的全部异常情况。

### 14.1 调用序列(与 vendor Initialize/Login 逐步一致)

`load()` = Initialize 等价物:bind 成功后按 vendor 顺序发(暖机结果全部忽略,与 SDK 一致):`GetMode{link:"app_downlink"}` → `GetFunctionsByEndpoint{endpoint:"app_downlink"}` → `GetHeadSetStatus` → **`GetAuthorizedTokens{elevoc_rpc_protocol:"RPC/2.2",token:<rand>,user:""}`**(目标机上曾观察跳过该交换后查询 fault;空 `user` 可用,其他值并非均已验证,见 §14.3)→ `GetConfigFileContent{buffer_len:0x1000}` → `GetFunctions` → `IsVoiceidSetted{endpoint}` → `GetFunctionsByEndpoint{endpoint:"speaker_extraction"}`。

之后:`GetCurrentMode` ≡ `GetMode{link:"app_uplink",endpoint:<默认捕获端点>}` → `Content.mode` 名→0..5(未知名→0),再按 `GetAlgType` 的 `algroithm_type` 是否以 "AI" 开头升级 1→6/2→7(`find("AI")!=0` 即旧时域算法,"Non-AI" 时域端点亦命中)。`SetEffectMode(mode)` ≡ voiceid 门控(ret 4)→ 6/7→1/2 重映射 → 能力门控 → `SetAlgType{algroithm_type:"Non-AI"(原 mode 6/7)否则"AI"}`(结果被忽略)→ `SetMode{link:"uplink",mode:<名字>,CALLER:"SDK",proto,token,endpoint}`。`QueryVoiceIDStatus` ≡ `IsVoiceidSetted{endpoint}` → `is_voiceid_setted` 真→2 否→0。

请求字段序字节级一致(vendor writer 字串序逐一对照):`GetAuthorizedTokens`=`{request,proto,token,user}`;`GetConfigFileContent`=`{request,buffer_len,proto,token}`;`GetMode`=`{request,link,endpoint?,proto,token}`;`SetMode`=`{request,link,mode,CALLER,proto,token,endpoint?}`;`SetAlgType`=`{request,algroithm_type,endpoint?,proto,token}`;`IsVoiceidSetted`/`GetFunctionsByEndpoint`/`GetAlgType`=`{request,endpoint,proto,token}`;`GetFunctions`/`GetHeadSetStatus`=`{request,proto,token}`。默认捕获端点经 COM `IMMDeviceEnumerator::GetDefaultAudioEndpoint(eCapture,eConsole)`+`IMMDevice::GetId` 取得(vendor 同法),无捕获设备时 endpoint 字段省略。

### 14.2 模式域与返回码

接口方法与语义:`SetEffectMode(int mode, const char* ep=NULL)`、`GetCurrentMode(const char* ep=NULL)`、`QueryVoiceIDStatus()`(0=未注册,2=已注册)、`SetDownLinkStatus(bool)`(扬声器侧)、`SetRawModeSupport(bool)`。

模式域:0=environmental(服务端默认,无独立 off)、1=shared、2=private、3=special、4=voiceid(需已注册否则 ret 4)、5=HQRecord;6/7=_td 旧别名,SDK 发送前重映射 1/2。`SetEffectMode` 返回码:1=成功、-1=RPC/默认端点失败、2=端点能力位不支持、4=VoiceID 未注册。能力门控位:GetMicAbilityByEndpoint bitmask(bit0=shared,bit1=private,bit2=special,bit3=voiceid,bit14=HQRecord)。

持久化:HKCU\SOFTWARE\Lenovo\SmartEngine\modules\F4006(REG_SZ)是 Vantage 插件的只写镜像(noise_micro_mode=GetCurrentMode 结果、noise_speaker_mode=扬声器布尔、scene_enhance_mode);**服务不消费该键**,只写注册表无效(已证:仅 SENoiseCancellingPlugin64.dll 引用)。实机值:micro=1(shared)、speaker=0、scene=0。

### 14.3 实机验证状态(2026-11-01)

纯 Rust 客户端在 TB14P-EZRA 实机跑通 Initialize→Login→GetCurrentMode 全链路:`vantage audio noise-cancel shared --dry-run` → **previous mic mode=Shared, speaker=off, voice-id=enrolled**(`--json`:`{"mic_mode":"shared","voice_id_enrolled":true}`),与厂商 DLL 基线一致(同机 P/Invoke `GetCurrentMode` = 1 = shared);读操作后实机复核 HKCU F4006 `noise_micro_mode=1`/`noise_speaker_mode=0` 未变。C35 记录了引入生成描述符之后的只读结果;它不证明所有请求均不会产生服务端副作用。

**应用键结论(2026-11-01 目标机只读探针)**:历史探针中,`user:""` 和省略字段均返回同一份令牌列表且 `GetMode` 正常;一次非白名单值使服务端处理函数 fault,跳过交换也导致后续查询 fault。当前实现不再携带原应用键,发送 `user:""`;此结论仅限已观察服务版本与测试值,不等于证明服务端对任意值均不校验。

**根因(已闭合)**:`RPC_CLIENT_INTERFACE::Flags`。厂商 ifspec 的取值是 `0x06000000`,该位型会把 rpcrt4 引到 NDR64-capable 分发路径;该路径需要厂商 DLL 自己 .rdata 里的 NDR64 格式表,而本客户端只携带 NDR32(v2.0)解释器表,于是 `NdrClientCall3` 在类型格式解释器里抛 `RPC_S_INTERNAL_ERROR (0x6e6)`(异常面由 VEH 兜住转 `ChannelUnavailable`,不崩溃)。把 `Flags` 改为 **0**(单一 NDR v2.0 语法)后同一套表、同一段 JSON、同一调用序列即成功;协商到的传输语法仍是 NDR32 v2.0,线上字节不变。12 变体 metadata sweep 实测:仅 `Flags=0` 是必要条件,`DispatchTable=NULL`、`InterpreterInfo` 取 `&ProxyInfo` 或 NULL、`nCount` 取 1 或 2 均不影响成功。

过程中一并排除/修正:pHandle 间接层(SDK `rpcndr.h` 证实 `MIDL_STUB_DESC` +0x18 是 `IMPLICIT_HANDLE_INFO` 联合体,单间接即可;双间接会得到 `0x6a6 INVALID_BINDING`)、COM vtable 取址优先级(`(*(*o).vtbl).add(N)` → `*(*o).vtbl.add(N)`)、参数按值/按址两种传法、`mFlags=0x02000001`/`MIDLVersion=0x0801026e`/`Version=0x00060001`/过程与类型格式串字节、`SetAlgType` 先行请求、`GetAlgType` 的 td 判定(`find("AI")!=0`)。

## 15. 面板图像特性接口(Intel IGCL)

**护眼(尚未完成 ABI 对齐与验收)**:曾发现应用的 `SetColorTemperature` 走 `IntelControlLib.dll` 的 `ctlPixelTransformationSetConfig`。当前 Rust 绑定和此前探针的结构体布局与 Intel 公开定义不一致,因此读写行为及硬件适用性均未证实。代码中的 Mid=5000K、High=4000K 是近似推测,不证明与原版等价;不得将该通道视作已实现或进行硬件写入。

**超分(尚未完成 ABI 对齐与验收)**:已找到 `ctlGetSetVideoProcessingFeature` 和 SUPER_RESOLUTION 接口,但当前自定义结构布局和查询参数尚未被目标驱动接受;不应把 dry-run 中的 unknown 状态视为有效读回或写入成功。Linux 无已验证的标准 DRM 等价实现。

**色域(诚实 Unsupported,依赖被禁)**:原版链路 `SEColorManagerPlugin64.dll` → `MONITORCONTROL.dll`(SmartColor addin 1.1.4.32 交付,**联想交付组件,禁依赖**)→ `MonCtrl_GetMonitorColorGamutByIndex`(按名查索引)/`MonCtrl_SetMonitorColorGamut(monitor, index)`,配 `WcsSetDefaultColorProfile`+`InstallColorProfileW` 挂 ICM(ICM 包 9 个 TPLCD_8AD7_*.icm 位于 C:\ProgramData\Lenovo\SmartColorAddin\AddinData\icc\ColorCalibration\)。无 MONITORCONTROL 只剩 ICC 关联 ≠ 原版行为(面板原生色域未切),不平替 → Unsupported, recovered contract 存档于此待非联想路径。Linux 等价评估:colord。

### 15.1 21VG 实机 IGCL 探针结论(2026-11-02,只读)

目标机 IGCL 组件:`C:\Windows\System32\DriverStore\FileRepository\iigd_dch.inf_amd64_*\IntelControlLib.dll`(两版 1.2.265 / 1.2.301.0;`ctlInit` 回 `SupportedVersion=0x00010001`)。导出名以实机为准:`ctlClose`、`ctlInit`、`ctlEnumerateDevices`、`ctlEnumerateDisplayOutputs`、`ctlPixelTransformationGetConfig`、`ctlPixelTransformationSetConfig`、`ctlGetSupportedVideoProcessingCapabilities`、`ctlGetSetVideoProcessingFeature`。

**结构布局已按公开头文件校正**(此前把 `ctl_pixtx_color_primaries_t` 写成 `u32`、`ctl_pixtx_config_t` 只留 1D LUT 成员、`ctl_video_processing_super_resolution_info_t` 缺两个 range 字段——这会让驱动按更大的结构写入调用方栈帧)。校正后的布局由 `crates/lctrl-hal-win/src/panel_features.rs` 的布局测试逐字段钉住(大小与偏移)。

**2026-11-02 契约与实机复验**:像素变换查询必须使用**显示输出句柄**,并固定请求与公开头文件匹配的 1.1 ABI;超分信息结构(116B)与 get/set 负载(92B)是不同结构。此前查询错用 adapter 句柄,不能用来判断块管线是否存在。修正后的独立只读探针成功枚举所选显示输出,返回 0 个 pixtx 块;超分 caps 查询仍返回 `0x4000000b`(`INVALID_ARGUMENT`)。见 `docs/evidence/igcl-readonly-probe-20261102.md`。因此:

- `panel eye-care` 仍报 `Unsupported(panel.eye-care.pixel-transformation-query-unsupported)`;本次无块可配置,没有护眼 LUT 写入入口;
- 超分状态查询被驱动以 `0x4000000b` 拒(`INVALID_ARGUMENT`),CLI 报 `unknown`;写路径同理拒绝(`panel.super-resolution.write-unverified`);
- 0 块结论只适用于本次所选显示输出与该驱动版本,不推断其他显示输出/未来驱动也没有能力。写路径不做猜测。

**Overdrive(当前 Unsupported)**:21VG 上未观察到 `\\.\SPBTESTTOOL` PnP 设备/INF,所测 BIOSOD `0x1A0000` 返回 null。上述结果不足以证明不存在其他硬件控制通道。
