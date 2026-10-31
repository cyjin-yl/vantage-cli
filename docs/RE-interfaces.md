# 逆向接口规格（净室文档）· Windows 硬件控制通道

**方法**:静态分析 + 只读运行验证。规格由二进制分析产出,实现只依据本文档,不复制二进制代码。
**边界**:不修改厂商二进制、不注入进程、不绕过 ACL/VBS/HVCI、不刷固件、不写 BIOS 口令。

---

## 1. 二进制清单（实测版本 + SHA-256）

| 二进制 | 版本 | 角色 | SHA-256 |
|---|---|---|---|
| `LnvMSRIO.sys` | 3.2.0.17 | MSR/物理内存访问微过滤器 + 设备 | `0a9d6e89888ff39764dd85c167b9c14709ee9701df4c695be89b40ed6fa7f6ff` |
| `WinMSRIO.dll` | 3.2.0.17 | 用户态客户端（MSR/Tcon） | `98088fef32770e0d63eb7462b69d71abea5c63ad6053793324928442b711553a` |
| `LenovoIPF.dll` | 3.2.0.17 | Intel DPTF/ESIF 性能客户端 | `fec74bcc0906295ca442ccdaf0b694ab4a127df2d8b19f5d5b412af29c81d273` |
| `LNVDispatcherService.exe` | 3.2.0.16 | Dispatcher（IPF 服务端宿主） | `d536f3c0c23ddf92baef5ac57ba3af66a792e3bbe858f933c731195948ee88d8` |
| `LnvSST.dll` | 1.0.8.5 | SmartSense 驱动侧 | `601f9fc413c9886023642d7d8e1f5a1d7eaaff333aa08460c8cc144ccbaf85d5` |
| `ludp.dll` | 2.1.6.11081 | LUDP Core（联想统一设备协议） | `eb83b56d8b512782913c8bd75461bcac2d4d5862218be93edb595a469be86919` |
| `SmartSAMD.sys` | 1.50.1.0 | **SmartRAID/SmartHBA 存储 miniport（与笔电控制无关）** | `0aacf89126c5cfa00d33fdab2b79e6cf15e6e4dcb8eea13a4ab4d062d1239f53` |
| `lenovoDriverBus.sys` | 2.0.000.133 | **WDF 虚拟总线（VHCI: 虚拟 HID/显示枚举）** | `a46e2f5d3e15c8d7b95f921c1cd30274a220620f2b08da8941a8cfafa2e684d4` |

PDB 源路径（同源构建证据）:
- `D:\Work\1.Dispatcher\Git\process_management\x64\Release\LnvMSRIO.pdb`
- `D:\3.0-dispatcher\process_management-3.2\x64\Release\WinMSRIO.pdb`
- `D:\3.0-dispatcher\process_management-3.2\x64\Release\LenovoIPF.pdb`

---

## 2. `LnvMSRIO.sys` — 设备与访问控制

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

## 3. `LnvMSRIO.sys` — IOCTL 分派表

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

## 4. `WinMSRIO.dll` — 用户态调用契约

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

**本机实测**:`\\.\SPBTESTTOOL` 不存在(`err=2` FILE_NOT_FOUND)。Tcon 通道在本机型**不适用**。

---

## 5. `LenovoIPF.dll` — DPTF/ESIF 性能框架

导出:`InitialConnection*`(`_Win32`/`_LNVDES`/`_ML`/`_ToolKit`)、`CheckConnection`、`DisConnection`、`GetCpuTempValue`、`GetSystemPowerValue`、`GetDefaultPLX`、`RestoreDefaultPLX`、`SendOEMV`、`SendOEMVUpdate`、`SendOEMV_GROUP`、`UpdtePowerLevel`(`4`/`NoTAU`/`_Platform`/`mw`/`mw2`)、`setCallBackFuntion`。

架构证据:字符串 `ESIF_I_ACPI_TRIP_POINT_NOT_PRESENT`、`ESIF_E_MSR_IO_FAILURE`、`ESIF_I_MSR_AFFINITY`、`\Intel\IPF` 注册表键、`ipfcore.dll`、`pipe://ipfsrv.public`、`pipe://ipfsrv.elevated`。
→ 实际 PL 控制在 `LNVDispatcherService.exe` 内,经命名管道 + ESIF/DPTF。

**本机实测**:`\\.\pipe\ipfsrv.public` 与 `\\.\pipe\ipfsrv.elevated` **均不存在**;`LNVDispatcherService.exe` 正在运行但未创建这两个管道(版本 3.2.0.16,与 IPF 3.2.0.17 不同源)。
→ 该 RPC 路径在本机不可用,不实现。

---

## 6. 已排除的误判通道

| 通道 | 实测 | 结论 |
|---|---|---|
| `\\.\Lenovo0` / `\\.\Lenovo1` | `err=2` | 由 `lenovoDriverBus.sys`(WDF VHCI 虚拟总线)注册,仅用于枚举虚拟 HID/显示子设备,**非硬件控制** |
| `SmartSAMD.sys` | 版本 1.50.1.0「Storport Miniport Driver for SmartRAID/SmartHBA」 | 存储驱动,**与风扇/充电/性能无关** |
| `\\.\MSRIO.sys` | 客户端备用路径;设备由同一驱动注册 | 与 `\\.\WinMsrDev` 同源,本机走 WinMsrDev |

---

## 7. 规格 ↔ 二进制一致性核对

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

| 项目 | 逆向规格 | 实现 | 一致性 |
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

## 10. GAMEZONE WMI 面(root\wmi)逆向结论(2026-11-01,目标机 21VG 实测 + ~3708 个联想二进制字符串/反汇编扫描)

### 10.1 提供者拓扑

- `LENOVO_GAMEZONE_DATA` / `LENOVO_FAN_METHOD` / `LENOVO_OTHER_METHOD` / `LENOVO_UTILITY_DATA`
  注册在 `ACPI\PNP0C14\GMZN_0` 上,宿主是微软标准 **WmiProv**(`wbem\wmiprovse.exe`,
  `wmiprov.dll`)——纯 WMI→ACPI 胶合层,方法语义全部在 BIOS ACPI(固件 GUID 对应的
  WMxx 控制方法)。**没有任何厂商内核驱动参与**:固件 GUID 不在磁盘上任何 .sys 里
  (已扫全 DriverStore)。HAL 客户端不依赖厂商驱动即可调用。
- 21VG 上仅 GAMEZONE_DATA / OTHER_METHOD / UTILITY_DATA 各有 1 个活实例;
  FAN_METHOD / CPU / GPU / MEM / PANEL / LIGHTING 实例报"不支持"——GAMEZONE 是
  拯救者硬件特性,ThinkBook 只暴露同一提供者骨架。

### 10.2 调用形态(已从二进制证实)

- 全部为普通 WMI ExecMethod;root\wmi 实例级调用。setter 收单个
  VT_I4/u32 `Data`(`SetIGPUModeStatus` 形参名 `mode`);布尔路径
  (`Fan_Set_FullSpeed` 除尘)用 VT_BOOL(u32 1/0)。
- `LENOVO_OTHER_METHOD.GetFeatureValue(IDs: u32)` 返回 CIM_UINT32(0x1A),
  与 `value: u32` 的 `SetFeatureValue` 配对。
- 方法名出现在用户态二进制:GameSettingsPlugin.dll / WSGameSettingPlugin.dll /
  WSHardwarePlugin.dll / lde_module_fan.dll(x64,代码最干净)/ TconSDK.dll 等。
  路径示例:`AcWMI::ExecMethodWMI`(Game/AcWMI.cpp)、`FunControlHelper.hpp`。

### 10.3 常量(反汇编证实;2026-11-01 二次深挖更新)

- SmartFan 模式值:**先前"0..3 四值"判断被修正**。WSGameSettingPlugin.dll
  (rva 校正后)真正的跳转表在 rva 0x157E0,属于 `GetFnQModeEx(type)`  getter
  (type 1..4),不是 setter;`DealWithSMfan` 直接操作**固件原始编码**,出现
  `cmp eax,0x7A` / `push 0x80; cmp ebx,0x80` 阈值比较(0x7A/0x80 语义未完全解析,
  可能是钳位阈值而非模式值)。唯一确证的语义:`RecordMode==2 && fnq==3` 为
  性能/电源特判。**实现中的 standard=0/silent=1/performance=2/custom=3 映射
  未获确认且部分被驳**——本机 provider 全拒、无法真机校验,保留仅作占位并须
  按未知处理。
- FeatureId 布局:`(family << 16) | index`,**bit31 = supported 标志**。
  lde_module_fan.dll(x64)file 0x6CF40 起有 `{u32 flag, u32 id}` 对表(stride 8,
  flag 0 为终止符),紧邻 `IDs`/`value`/`LENOVO_OTHER_METHOD` 字符串,旁侧
  `LENOVO_FAN_TEST_DATA`/`NumOfFans`/`FanMaxSpeed` → **风扇域 ID 表**:
  `0x8002FCF0, 0x8002FBC0, 0x8002FBE0`(族 0x2F)、`0x80040320`(0x403)、
  `0x80041A90`(0x41A)、`0x800414A0`/`0x80041430`(0x414)、
  `0x80040E40`/`0x80040E50`(0x40E)、`0x80040DF0`(0x40D),终止符 `{0, 0x05040000}`。
- **ID ↔ 特性名映射未恢复**(表项无名称;调用经间接调用传 ID,预算内无法 xref)。
- **充电写路径不在 OTHER_METHOD**:三 DLL 中 `SetPowerCharge` 0 命中;
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
