# 逆向接口规格（净室文档）· Windows 硬件控制通道

**方法**:静态分析 + 只读运行验证。规格由二进制分析产出,实现只依据本文档,不复制二进制代码。
**边界**:不修改厂商二进制、不注入进程、不绕过 ACL/VBS/HVCI、不刷固件、不写 BIOS 口令。

---

## 0. 依赖与常量边界(2026-11-01 用户裁定,适用于本文所有章节)

1. **联想交付的软件不得作为运行时依赖**:Vantage addins 目录下的 payload(DolbyRpcClient.dll、ElevocControlSDK_x64.dll、XCASDK.dll、MONITORCONTROL.dll)不引用、不复制。非联想驱动或应用组件可依第 3 条准入;使用系统 RPC 客户端并不消除对 RPC 服务端的依赖。
2. **接口描述表不得转录**:当前源代码不得内嵌从厂商二进制逐字节抄来的 MIDL proc/type format string 或 format-string offsets。描述符由本仓库生成器按公开 NDR 格式语法,从恢复出的"opnum → 参数类型与属性"生成;接口标识符和参数语义仍由逆向获得。生成的格式串可能与其他实现相同,这不能单凭输出字节证明其来源。旧 Git 历史中曾提交过的表不因此自动消失;源码哈希作为分析证据保留。
3. **非联想驱动/应用可引用,但 ABI 稳定是首要判据**:接口版本字段/结构布局必须稳定或带版本协商;引用时记录来源、版本与稳定依据,并评估 Linux 侧等价物(DRM/colord 等)。
4. **行为与原版一致,不做平替**:复刻原版的调用序、信封、字段序与返回码语义;若只能得到不等价的结果,宁可诚实 Unsupported。

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

## 11. EnergyDrv 充电域完整契约(2026-11-01,PowerBattery.dll 1.0.13.114 + IdeaBatteryAgent.dll + AcpiVpc.sys 15.11.30.14 反汇编/IL 实证)

二进制链:`LenovoVantage-(IdeaNotebookAddin).exe → IdeaNotebookAddin.dll → IdeaBatteryAgent.dll(BatteryAgent RPC 面)→ PowerBattery.dll(native,按序数导出)`。设备 `\\.\EnergyDrv` 由 `AcpiVpc.sys` 创建(服务 `acpivpc`,demand-start;用户态可开,无需 SYSTEM)。所有 IOCTL 输入输出校验以驱动 Dispatch 为准:`InputLength≥1`(USB)/`≥4`(夜间),`OutputLength≥4`,否则 `STATUS_BUFFER_TOO_SMALL`。

### 11.1 充电模式(GBMD,`0x831020f8`,1B 子命令)

| 子命令 | 语义 | 证据 |
|---|---|---|
| `0xFF` | 状态位掩码查询 | 实机 `0x00860004`(bits 2,17,18,23) |
| `0x03` | 打开 storage(养护) | CStorageMode v+8 @0xc3d0 |
| `0x05` | 关闭 storage | @0xc580 |
| `0x07` | 打开 express(快充) | CExpressMode v+8 @0xbe90 |
| `0x08` | 关闭 express | @0xbfb0 |

**读回解码**(`CChargingMode::GetChargingMode` @0x9bc0,IL 面为恒等映射):bit5=storage 开 → Storage;bit2=express 开 → Quick;否则 Normal。storage 支持在 x86 上=驱动可打开(@0xc1d0,无状态位);express 支持=bit17(@0xbd70 `shr 0x11`);Storage80 支持=bit23(@0xaa80)。`SetChargingMode` @0x9c30 写后按同一解码轮询(`Sleep 50ms ×10`)等待生效,然后写注册表缓存。

**对 §10.3 与 docs/01 的更正**:
- 注册表值实为 `HKCU\SOFTWARE\Lenovo\VantageService\AddinData\IdeaNotebookAddin\BatteryChargeMode`,**REG_EXPAND_SZ**,取值字符串 `Normal`/`Storage`/`Quick`(HKLM 键不存在)。它是**只写透传缓存——二进制内无任何读取方驱动行为**,硬件位掩码才是权威状态。vantage 不写该值。
- docs/01 §3.3 对 `0xFF` 的旧位注释(bit24=认证充电器, bit15-16=Inbox 适配器)与反汇编不符,以本节位表为准(bit2/bit5/bit17/bit23 已证;bit14 语义未决;bit18 未识)。

### 11.2 USB 充电控制(`0x831020e8`,1B 选择子)

| 选择子 | 语义 | 输出 |
|---|---|---|
| `0x02` | 标志字查询 | bit6=AC 支持, bit7=AC 开, bit14=电池模式支持, bit15=电池模式开 |
| `0x0a`/`0x0b` | AC AlwaysOnUSB 开/关 | 空输出(Information=4) |
| `0x13`/`0x12` | 电池模式开/关 | 空输出 |

`CUSBCharger::OpenOrClose`(@0xc730)就是状态查询(+0x20 槽),写后按位条件重查(厂商最多 10 次)。`IsSupportOnARM` 恒 0。**对 docs/02 §6 的更正**:`+0x32` 不是函数槽(该版本 vtable:+8 Open, +16 Close, +24 IsSupport, +32 OpenOrClose=status);旧资料 "同族命令字[推断]" 已解决为同一 IOCTL 0x0a/0x0b。实机 `0x02 → 0x0000CEC0`:两种模式均支持且**均开启**(bits 7/15;此前人工读数"关闭"有误,以本次 CLI 解码为准,与 BIOS AlwaysOnUSB=Enable 一致)。

### 11.3 夜间充电(`0x83102150`,4B 选择子)

`0x13` 支持探测(响应 bit0=支持);`0x11` 状态(bit4=开);写 `0x80000012`=开、`0x12`=关。**实机更正(2026-11-01)**:同机探测回 `0x11` 而状态回 `0x01`——探测响应 bit4 并非当前状态(此前[推断]有误);权威状态只取 `0x11` 查询。实机状态:支持,当前关。写响应按 4B 读取(内容不解释),写后以 `0x11` 轮询验证(与 GBMD 同轮询界)。

### 11.4 临时充电模式(`0x83102148`,4B 选择子)

`0x13` 支持探测、`0x11` 状态读。位语义沿用历史组件契约(docs/02 §5):bit0=Permanent, bit1=PlugIn, bit2=DynamicLifeSpan;PlugIn/DynamicLifeSpan 置位即临时模式。本机未发现写方(写选择子按夜间充电同型试探无对应代码路径)——只读探测。

### 11.5 DBDC(`LENOVO_REPORT_DBDC_DATA`,WMI 只读)

Counts/CurrentLimit[](mA)/ROS_Power[](mW)/Threshold[](%),实机 3 档 [7500,5000,4500]/[100,40,20]。写路径(`SetFeatureValue` + DBDC FeatureId)因 ID↔名映射未恢复保持门禁(§10.3 表)。

## 11.4 修正与 11.6 阈值写方结论(2026-11-01,ThresholdRe2:BatterySetting.exe/WrapPlugin.dll/ThinkPowerPlugin.dll/AcpiVpc.sys/PowerBattery.dll 反汇编 + 实机 ACPI 探针)

**11.4 修正**:PowerBattery.dll 的 temporary-mode 只读选择子为 0x23/0x21/0x31/0x41;§11.4 原记的 0x13/0x11 实为读回 3/1(实机)。写选择子**证实缺席**:BatteryManagementContract.dll 无 Set-BatteryTemporaryMode,BatterySetting 从不发送 0x148,固件侧写口未恢复→写保持门禁。

**11.6 自定义充电阈值**:完整链 BatterySetting.exe → WrapPlugin.dll(thunk `*0x560718` @+0x4268b8)→ ThinkPowerPlugin.dll → PWRMGRV(ThinkPad Power Manager)SetChargeThreshold → PWMIF32V.DLL SM_SetCharge{Start,Stop}CapacityThreshold,持久化 HKLM\SOFTWARE\Lenovo\PWRMGRV\Data\Charge{Start,Stop}{Control,Percentage},读回仅注册表(默认 76/80)。**21VG 判定不支持**:PWMIF32V.DLL 缺席、PWRMGRV 键空、AcpiVpc 能力方法 MHQI(IOCTL 0x83102130/0x83102134)实机返回 win32=2(该固件 ACPI 方法不存在)。无 powrprof/GUID 路径参与。

**驱动 IOCTL→ACPI 全映射**(AcpiVpc.sys 15.11.30.14,驱动仅透传 4 字节选择子,写门控全在固件):0x20e8 SALS/HALS、0x20f8 GBMD、0x211c/0x2138 ITHC、0x2120 GBID、**0x212c SMTF(可写透传,语义未恢复,探测返回 0xFFFF→门禁)**、0x2130/0x2134 MHQI 读、0x2144/58/5c KBL*、**0x2148 BTMC**、0x214c BSIF、0x2150/0x2154 BNMC。

**极端省电(原版实现还原)**:原版不是固件通道,而是 AiTurbo DriverStore 包的 BatteryLife.exe **EcoQoS 进程限流守护**:powrprof DC 索引写 + PowerSetActiveScheme + 进程节流委派 AiPerfSvc.exe,持久化 HKLM\SOFTWARE\Lenovo\AiTurbo\BatteryLife;BatterySetting 内的 "ExtremeBatteryLife" 代码实为云遥测(WININET 上报 + rainbow 注册表)。按用户 2026-11-01 指示(逆向实现与原版一致,不平替):本功能须按 AiTurbo 原版契约(AiPerfSvc 委派/其注册表键)实现,禁止用通用 ProcessPowerThrottling 平替;契约未恢复前保持门禁。
## 12. Dolby DAX RPC 契约(2026-11-01,DolbyRpcClient.dll pkg 1.0.3 + DAX3API.exe 3.30904.943.0 反汇编 + 实机只读探针实证;实现改为纯 Rust rpcrt4 通道)

链路:`vantage audio dolby → 纯 Rust rpcrt4 NDR 客户端 → ALPC ncalrpc:[DaxRpcEndpoint] → DAX3API.exe(DolbyDAXAPI 服务,SYSTEM)`。接口 GUID `11c6212d-d5f3-4500-ab16-634dc63037be` v1.0,NDR v2.0,`RPC_IF_AUTOLISTEN | RPC_IF_ALLOW_LOCAL_ONLY`(仅本机);Object UUID `B94BA572-658F-40E6-861D-DFB7BB4001A8`。许可 APO DSP(DolbyAPOv*/DolbyAPOAIDE)只被 DAX3API 内部使用,vantage 只切换开关/预设,不重实现 DSP。

**客户端依赖边界**:原实现绑定 WindowsApps 包内的 `DolbyRpcClient.dll` 并复制到 `%LOCALAPPDATA%` 缓存;当前源码已删除该装载和缓存。RPC 客户端使用系统组件 `rpcrt4.dll` 与 `oleaut32.dll`,接口描述符按恢复出的签名生成(见 12.4)。这不消除服务端依赖:调用仍需可用的 `DolbyDAXAPI` / `DAX3API.exe`;卸载 Vantage 后该服务是否保留尚未验证。

### 12.1 客户端导出(`DolbyRpcClient.dll`,按 thunk 字节验证,全部无调用方可见参数)

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

out 参数全部在 wrapper 栈帧内分配;调用方只需按上表传参。`Set*` 的 bool 返回值恒为真(thunk 内 `mov al,1`),失败检测靠读回。

### 12.2 运行时行为(厂商客户端基线,用于对照)

- (历史)WindowsApps 包路径 `C:\Program Files\WindowsApps\E046963F.LenovoCompanion_*\LenovoVantage\DolbyRpcClient.dll`;其 DACL 阻止原地 `LoadLibrary`(实机探针证实),须先复制到可写目录 —— 该复制/缓存机制已随 DLL 依赖一并删除。
- `DolbyClient_Initialize` 的 bool 返回不是就绪信号(实机探针返回 false,后续所有 RPC 均成功),调用但不判其值。纯 Rust 通道无此导出,等价地不判任何“就绪”返回值。
- 实机只读探针(ezra/管理员,非 Vantage 进程):`Initialize=0 GetDolbyEnabled=0 GetActiveProfile=5 GetCurrentMode=-1 IsSpatialAudioSupported=1 GetSpatialAudioStatus=2 GetSKU=1` —— Dolby 关、预设索引 5、空间音频支持。证明第三方进程可独立绑定该端点(非 SYSTEM 专属)。
- 预设索引表(打包名字表顺序,中置信):0=movie,1=music,2=game,3=voice,4=personalize,5=dynamic;服务器校验范围 [0,5]。`Off` 不是预设索引,走 `SetDolbyEnabled(0)`。
- 服务前置:`DolbyDAXAPI` 须 Running;标准用户(未提权)RPC ACL 未验证 —— 写操作经现有提权路径。

### 12.3 实现落点

- `lctrl-hal-win/src/dolby.rs`:`NativeDolbyRpc`(纯 Rust rpcrt4 NDR 客户端)+ `WindowsDolbyAudio<R: DolbyRpc>`(dry-run/commit + 读回轮询,`poll_readback` 10×50ms)。trait 接缝 `DolbyRpc` 与 `vantage-cli` 表面未变。
- `vantage/src/main.rs` Windows 组合根:`NativeDolbyRpc::load()` 失败(端点不可达)时不挂载 audio 服务,`audio dolby` 如实报 Unsupported。
- 第二通道 `ncalrpc:[FusionRpcEndpoint]`(DolbyFusionRpcClient.dll,Atmos/Fusion 附加项)未接入,列为后续。

### 12.4 纯 Rust rpcrt4 通道契约(取代厂商 DLL 绑定)

**ABI 稳定性依据**:锚点是 IfSpec 版本 `11c6212d-…` **v1.0** 与 NDR **v2.0**(DCE/RPC NDR32 封送规则,`NdrClientCall3` 由 rpcrt4 实现)。调用方与 DAX3API 之间只有这两项版本化约定;厂商 DLL 自身的导出签名不是契约,故可弃用。`rpcrt4.dll`/`oleaut32.dll` 是 Windows 系统组件,随系统更新而稳定。

**绑定与调用序列**(与厂商 `DolbyClient_Initialize` 逐条对齐):

1. `RpcStringBindingComposeW(NULL, "ncalrpc", NULL, "DaxRpcEndpoint", NULL, &binding)` —— 绑定串不带 object UUID(UUID 走 opnum 4 握手)。
2. `RpcBindingFromStringBindingW` → 会话 binding handle(懒绑定)。
3. `NdrClientCall3(proxy, 0, NULL, binding, &context)`:`[in] handle_t` binding → `[out]` 会话 context token;后续所有过程以该 token 作第 0 参数。
4. `NdrClientCall3(proxy, 4, NULL, context, bstr, &u16, bstr)`:`[in]` object-UUID BSTR → `[out]` u16;厂商忽略返回值(实现同样只报传输级失败)。

**各 opnum 的形参**(厂商 thunk 逐字节确认;x64 ABI:第 1 个变参在 `r9`,其余在栈参槽,`proc string` 的 `stack_size` 给出槽数):

| opnum | 过程 | 形参(stack_size) | 语义 |
|---|---|---|---|
| 0 | OpenContext | binding + `&context`(0x10) | 建立会话 context |
| 2 | IsSpatialAudioSupported | context + 4×`&u32`(0x28) | `p0 > 3 ‖ p1 ≥ 0x7853` |
| 4 | RegisterObject | context + BSTR + `&u16`(0x18) | object UUID 握手 |
| 5 | GetDolbyEnabled | context + `&u16`(0x10) | on/off 读(0/1) |
| 6 | SetDolbyEnabled | context + `u8`(0x10) | on/off 写(厂商返回恒真,靠读回判失败) |
| 7 | GetActiveProfile | context + `&u32`(0x10) | 预设索引读 [0,5] |
| 8 | SetActiveProfile | context + `u32`(0x10) | 预设写 |
| 37 | GetSKU | context + `&BSTR`(0x10) | SKU 名串(BSTR) |

**元数据**:MIDL 解释器表(proc format string / format offset / type format)由 `lctrl-hal-win/src/ndr/dolby.rs` 在编译期按公开的 NDR `-Oicf`(x64)格式规则,从上表恢复出的形参签名生成。当前源码不再内嵌原先逐字节转录的描述表;类型表仅保留本通道引用的 context handle 和 BSTR 语义。BSTR 参数经 `FC_USER_MARSHAL`(quad index 0)封送,quad 表绑定系统 oleaut32 的 `BSTR_UserSize/Marshal/Unmarshal/Free`。`-Oicf` 是 RPC 运行时的解释器元数据格式,不应仅凭线协议兼容就断言不同编译器生成的表必定逐字节相同。

**单语法决定**:`RPC_CLIENT_INTERFACE.Flags` 置 0(厂商为 `0x06000000`)。厂商同时携带 NDR64 格式表;本客户端只携带 NDR32 表,若置多语法位,rpcrt4 会协商到 NDR64 并因缺少 NDR64 表而报 `RPC_S_INTERNAL_ERROR`(`0x6e6`)。置 0 后协商语法恒为 NDR v2.0,线上字节与厂商客户端一致。

**异常处理**:rpcrt4 用 SEH 异常上报传输失败(厂商用 `__try/__except`)。Rust 无 SEH,故每次调用包在 vectored handler + `RtlCaptureContext`/`RtlRestoreContext` 守卫内,把 RPC 状态异常转成 `ChannelUnavailable` 错误;非 RPC 异常(真实故障)不吞。

**实机只读实证**(2026-11-01,ezra/管理员,非 Vantage 进程,纯 Rust 通道):`connect=ok GetDolbyEnabled(op5)=0 GetActiveProfile(op7)=5 IsSpatialAudioSupported(op2)=1 GetSKU(op37)=1` —— 与 12.2 节厂商客户端基线逐值一致;`vantage audio dolby dynamic --dry-run` 输出 `previous enabled=false profile=Dynamic`。写路径(opnum 6/8)未在实机验证,仍走既有写入门禁。

## 13. SmartSense HPD 设置管道契约(2026-11-01,ThinkSmartSenseAddin.dll 1.0.0.77 IL + SmartSense.exe/LnvSST.dll 1.0.8.5 反汇编 + 实机枚举实证)

链路:`vantage sense → \\.\pipe\com.lenovo.smartsense.settings → SmartSense.exe(LocalSystem)→ HKLM\SYSTEM\CurrentControlSet\Services\SmartSense\Parameters\HPD`。帧格式:`u32 code + u32 arg` 小端(legacy bool=0/1;camera/video 系 1=enable/2=disable),每连接一请求;响应单个 `i32`(0=拒绝/非零=受理/-1=服务端异常)。客户端任何用户可连接(addin runas=user)。完整命令表(IL .ctor 0x01-0x13a,已证;`code=0xNNvv`,payload `u32 code [+ u32 arg]`,响应 i32):

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


读回:服务持久化到 HPD 键(REG_DWORD),addin 自身 getter 也直读该键 —— 注册表即权威读回通道。已建模:PresentLeave/LeaveDistance/LeaveWait/Approach/ApproachDistance/Video(1开2关)/CameraDetect(1开2关)/Capability/SensorType/FacialFeatureRegistered。AttentionTracking 命令存在(0x0016-0x0019)但本机 HPD 无对应持久化键(Capability=1 最小集)→ 写后不轮询,如实报 write-only。键盘灯自动走 contract RPC 通道(Lenovo.Vantage.RpcClient,SystemManagement.SmartSense)未恢复,保持门禁。感知管线(VL53L5 ToF + IR 摄像头)留在厂商服务内,本通道只改设置。

## 14. Elevoc 降噪 RPC 契约(2026-11-01,ElevocControlSDK_x64.dll 1.0.1.1108.8f92a86 + ElevocControlService.exe 1.0.1.1142 反汇编实证)

链路:`vantage audio noise-cancel → ALPC ncalrpc:[ElevocRpcEndpoint] → ElevocControlService.exe(LocalSystem)`。接口 GUID `{6F761170-1054-4339-A573-063DD08C7E16}` v4.0,NDR32;单 opnum-0 通用分发,方法名在 JSON 请求信封内(elevoc_rpc_protocol)。**实现为纯 Rust rpcrt4 客户端**(`lctrl-hal-win/src/noise.rs`,`NativeElevocRpc`):按 2026-11-01 依赖裁定,Lenovo 交付的 ElevocControlSDK_x64.dll 已禁止作为运行时依赖,DLL 发现/缓存机制移除;传输改为系统 `rpcrt4.dll` + `NdrClientCall3` 直打 opnum-0。

**客户端依赖边界**:当前源码不再加载或复制 Elevoc SDK,也不再内嵌原先逐字节转录的描述表;客户端通过系统 `rpcrt4.dll` 调用。RPC 服务端仍是必需依赖:目标机实测 `ElevocService` 运行 `C:\Windows\System32\ElevocInstallDriver\ElevocControlService.exe`。卸载 Lenovo Vantage 后服务是否保留尚未验证;不能据此声称无需安装或保留该服务。

### 14.0 纯 Rust 通道实现要点(ABI 稳定依据)

- **绑定**:`RpcStringBindingComposeW(NULL,"ncalrpc",NULL,"ElevocRpcEndpoint")` → `RpcBindingFromStringBindingW`(与 SDK Initialize 9c0- 一致;无 AuthInfo,服务端无鉴权注册)。
- **MIDL 表**:当前源码由 `lctrl-hal-win/src/ndr/elevoc.rs` 的生成器按恢复出的签名 `void opnum0([in,out,string,size_is(0x5000)] wchar_t *json)` 生成描述符,不再保存原先转录的格式串。过程头启用 `HasNewCorrDesc` 和两端 correlation checks;因此 sized-string 的四字节常量符合度描述符后还必须生成两字节 `/robust` 标志(常量采用 early correlation)。结构体根据公开 `rpcndr.h` 定义构建。接口 GUID+v4.0 与 NDR32 v2.0 是当前测试版本的绑定依据,不是未来兼容性保证:服务端可能保留同一版本号但更改语义,必须针对更新版本重新验证。
- **异常面**:vendor `SafeRpcRequest` 7b0 以 `__try/__except` 将 RPC 异常换成返回码。复刻用 VEH+landing-pad(guard `NdrClientCall3`):异常线程还原到包装帧返回,异常码→ChannelUnavailable;正常路径零开销。
- **请求缓冲**:保留原有不超过 `0x3ff` 个 UTF-16 单元的请求限制;由于生成的 `[in,out,string,size_is(0x5000)]` 描述符允许更长响应,本地接收缓冲使用同一个 `0x5000` 单元上限,包括末尾 NUL。不声称此约束已验证恶意 RPC 服务端的全部异常情况。

### 14.1 调用序列(与 vendor Initialize/Login 逐步一致)

`load()` = Initialize 等价物:bind 成功后按 vendor 顺序发(暖机结果全部忽略,与 SDK 一致):`GetMode{link:"app_downlink"}` → `GetFunctionsByEndpoint{endpoint:"app_downlink"}` → `GetHeadSetStatus` → **`GetAuthorizedTokens{elevoc_rpc_protocol:"RPC/2.2",token:<rand>,user:""}`**(目标机上曾观察跳过该交换后查询 fault;空 `user` 可用,其他值并非均已验证,见 §14.3)→ `GetConfigFileContent{buffer_len:0x1000}` → `GetFunctions` → `IsVoiceidSetted{endpoint}` → `GetFunctionsByEndpoint{endpoint:"speaker_extraction"}`。

之后:`GetCurrentMode` ≡ `GetMode{link:"app_uplink",endpoint:<默认捕获端点>}` → `Content.mode` 名→0..5(未知名→0),再按 `GetAlgType` 的 `algroithm_type` 是否以 "AI" 开头升级 1→6/2→7(`find("AI")!=0` 即旧时域算法,vendor bb4;"Non-AI" 时域端点亦命中;**原记 "contains(token)" 系误判,已更正**)。`SetEffectMode(mode)` ≡ voiceid 门控(ret 4)→ 6/7→1/2 重映射 → 能力门控 → `SetAlgType{algroithm_type:"Non-AI"(原 mode 6/7)否则"AI"}`(结果被忽略)→ `SetMode{link:"uplink",mode:<名字>,CALLER:"SDK",proto,token,endpoint}`。`QueryVoiceIDStatus` ≡ `IsVoiceidSetted{endpoint}` → `is_voiceid_setted` 真→2 否→0。

请求字段序字节级一致(vendor writer 字串序逐一对照):`GetAuthorizedTokens`=`{request,proto,token,user}`;`GetConfigFileContent`=`{request,buffer_len,proto,token}`;`GetMode`=`{request,link,endpoint?,proto,token}`;`SetMode`=`{request,link,mode,CALLER,proto,token,endpoint?}`;`SetAlgType`=`{request,algroithm_type,endpoint?,proto,token}`;`IsVoiceidSetted`/`GetFunctionsByEndpoint`/`GetAlgType`=`{request,endpoint,proto,token}`;`GetFunctions`/`GetHeadSetStatus`=`{request,proto,token}`。默认捕获端点经 COM `IMMDeviceEnumerator::GetDefaultAudioEndpoint(eCapture,eConsole)`+`IMMDevice::GetId` 取得(vendor 同法),无捕获设备时 endpoint 字段省略。

### 14.2 导出与模式域

导出(opnum/签名/RVA 已证):`SetEffectMode(int mode, const char* ep=NULL)` @0x322c0、`GetCurrentMode(const char* ep=NULL)` @0x2d7d0、`QueryVoiceIDStatus()` @0x311d0(0=未注册,2=已注册)、`SetDownLinkStatus(bool)` @0x31f60(扬声器侧)、`SetRawModeSupport(bool)` @0x32950。

模式域(字符串槽 765):0=environmental(服务端默认,无独立 off)、1=shared、2=private、3=special、4=voiceid(需已注册否则 ret 4)、5=HQRecord;6/7=_td 旧别名,SDK 发送前重映射 1/2。`SetEffectMode` 返回码(545):1=成功、-1=RPC/默认端点失败、2=端点能力位不支持、4=VoiceID 未注册。能力门控位:GetMicAbilityByEndpoint bitmask(bit0=shared,bit1=private,bit2=special,bit3=voiceid,bit14=HQRecord)。

持久化:HKCU\SOFTWARE\Lenovo\SmartEngine\modules\F4006(REG_SZ)是 Vantage 插件的只写镜像(noise_micro_mode=GetCurrentMode 结果、noise_speaker_mode=扬声器布尔、scene_enhance_mode);**服务不消费该键**,只写注册表无效(已证:仅 SENoiseCancellingPlugin64.dll 引用)。实机值:micro=1(shared)、speaker=0、scene=0。

### 14.3 实机验证状态(2026-11-01)

纯 Rust 客户端在 TB14P-EZRA 实机跑通 Initialize→Login→GetCurrentMode 全链路:`vantage audio noise-cancel shared --dry-run` → **previous mic mode=Shared, speaker=off, voice-id=enrolled**(`--json`:`{"mic_mode":"shared","voice_id_enrolled":true}`),与厂商 DLL 基线一致(同机 P/Invoke `GetCurrentMode` = 1 = shared);读操作后实机复核 HKCU F4006 `noise_micro_mode=1`/`noise_speaker_mode=0` 未变。C35 记录了引入生成描述符之后的只读结果;它不证明所有请求均不会产生服务端副作用。

**应用键结论(2026-11-01 目标机只读探针)**:历史探针中,`user:""` 和省略字段均返回同一份令牌列表且 `GetMode` 正常;一次非白名单值使服务端处理函数 fault,跳过交换也导致后续查询 fault。当前实现不再携带原应用键,发送 `user:""`;此结论仅限已观察服务版本与测试值,不等于证明服务端对任意值均不校验。

**根因(已闭合)**:`RPC_CLIENT_INTERFACE::Flags`。厂商 ifspec 的取值是 `0x06000000`,该位型会把 rpcrt4 引到 NDR64-capable 分发路径;该路径需要厂商 DLL 自己 .rdata 里的 NDR64 格式表,而本客户端只携带 NDR32(v2.0)解释器表,于是 `NdrClientCall3` 在类型格式解释器里抛 `RPC_S_INTERNAL_ERROR (0x6e6)`(异常面由 VEH 兜住转 `ChannelUnavailable`,不崩溃)。把 `Flags` 改为 **0**(单一 NDR v2.0 语法)后同一套表、同一段 JSON、同一调用序列即成功;协商到的传输语法仍是 NDR32 v2.0,线上字节不变。12 变体 metadata sweep 实测:仅 `Flags=0` 是必要条件,`DispatchTable=NULL`、`InterpreterInfo` 取 `&ProxyInfo` 或 NULL、`nCount` 取 1 或 2 均不影响成功。

过程中一并排除/修正:pHandle 间接层(SDK `rpcndr.h` 证实 `MIDL_STUB_DESC` +0x18 是 `IMPLICIT_HANDLE_INFO` 联合体,单间接即可;双间接会得到 `0x6a6 INVALID_BINDING`)、COM vtable 取址优先级(`(*(*o).vtbl).add(N)` → `*(*o).vtbl.add(N)`)、参数按值/按址两种传法、`mFlags=0x02000001`/`MIDLVersion=0x0801026e`/`Version=0x00060001`/过程与类型格式串字节、`SetAlgType` 先行请求、`GetAlgType` 的 td 判定(`find("AI")!=0`)。

## 15. 面板图像特性契约(2026-11-01,SEColorManagerPlugin64.dll/SmartColorAddin.dll/PlatformInterface.dll 反汇编 + IGCL 公版头 + 实机文件枚举)

**护眼(已实现)**:原版链路 Lenovo SmartInteract `PlatformInterface.dll!IGCLInterfaceClient::SetColorTemperature(display, kelvin)` → 动态加载显卡驱动的 `IntelControlLib.dll`(IGCL,**非联想驱动组件,允许引用**)→ `ctlPixelTransformationSetConfig` 写 1D LUT;Off=restore-default。IGCL ABI 稳定性依据(用户裁定之首要判据):公版 SDK、所有结构带 Size+Version 字段、`ctlInit` 输出实现版本——实现 pin `CTL_API_VERSION(1,1)` 且运行时校验实现 major==1,不符即 fail-closed。LUT 数据依据:PlatformInterface 的 `RGBToTemperatureColor`+`kelvin_table`(std::map,360 键,1988K–19964K,.rdata 节点已恢复,证实原版为查表实现);UI 档位 Kelvin 值由 Vantage 运行时配置下发(静态不可恢复)→ Mid/High 暂用 Tanner-Helland 近似(Mid=5000K,High=4000K,标注 INFERRED),验收阶段用只读 GetConfig 与原版档位 LUT 比对后修正。驱动无稳定 get-config 时读回退化为进程内最近写入态(如实)。

**超分(已实现)**:同库 `ctlGetSetVideoProcessingFeature`,FeatureType=SUPER_RESOLUTION(4),ValueType=CUSTOM,自定义结构 `ctl_video_processing_super_resolution_info_t{flag}`;flag:1=DISABLE、2=ENABLE_DEFAULT_SCENARIO。DC 模式原版强制关。Linux 无标准 DRM 等价 → 诚实 Unsupported。

**色域(诚实 Unsupported,依赖被禁)**:原版链路 `SEColorManagerPlugin64.dll` → `MONITORCONTROL.dll`(SmartColor addin 1.1.4.32 交付,**联想交付组件,禁依赖**)→ `MonCtrl_GetMonitorColorGamutByIndex`(按名查索引)/`MonCtrl_SetMonitorColorGamut(monitor, index)`,配 `WcsSetDefaultColorProfile`+`InstallColorProfileW` 挂 ICM(ICM 包 9 个 TPLCD_8AD7_*.icm 位于 C:\ProgramData\Lenovo\SmartColorAddin\AddinData\icc\ColorCalibration\)。无 MONITORCONTROL 只剩 ICC 关联 ≠ 原版行为(面板原生色域未切),不平替 → Unsupported, recovered contract 存档于此待非联想路径。Linux 等价评估:colord。

### 15.1 21VG 实机 IGCL 探针结论(2026-11-01,只读)

目标机 IGCL 组件:`C:\Windows\System32\DriverStore\FileRepository\iigd_dch.inf_amd64_*\IntelControlLib.dll`(两版 1.2.265 / 1.2.301.0;`ctlInit` 回 `SupportedVersion=0x00010001`)。导出名以实机为准:**`ctlClose`**(非 `ctlDestroy`);`ctlInit`/`ctlEnumerateDevices`/`ctlEnumerateDisplayOutputs`/`ctlPixelTransformationGetConfig`/`ctlPixelTransformationSetConfig`/`ctlGetSupportedVideoProcessingCapabilities`/`ctlGetSetVideoProcessingFeature` 均在。结构体按公版头:`ctl_init_args_t` 的 `ctl_application_id_t` 为 16 字节(Data1 u32/Data2 u16/Data3 u16/Data4[8]),写错会让 `ctlInit` 返 `CTL_RESULT_ERROR_UNSUPPORTED_VERSION(0x40000009)`。

探针(`crates/lctrl-hal-win/examples/pixtx_probe.rs`,只读)实测:

| 查询 | 结果 | 结论 |
|---|---|---|
| `ctlEnumerateDevices` / `ctlEnumerateDisplayOutputs` | SUCCESS,各 1 个 | 内屏输出句柄可用 |
| `ctlPixelTransformationGetConfig(CAPABILITY)`,1 block | SUCCESS,但 `NumBlocks=0` | **21VG 内屏无像素变换(1D LUT)管线** |
| `ctlPixelTransformationGetConfig(CURRENT)` | `0x4800000c INVALID_PIXTX_BLOCK_NUMBER` | 无活动 LUT 可读 |
| `ctlGetSupportedVideoProcessingCapabilities` | SUCCESS,7 项特性 | 适配器支持视频处理族 |
| `ctlGetSetVideoProcessingFeature(get SUPER_RESOLUTION)` | `0x4000000b INVALID_ARGUMENT` | 状态查询被驱动拒(尺寸/版本/参数语义未对齐)→ 状态报 unknown,不伪造 |

因此 CLI 现状:`panel eye-care` → Unsupported(`panel.eye-care.no-pixel-transformation-pipeline`,硬件不适用);`panel super-resolution` → 状态 unknown,写路径保留但仍在写门禁内。护眼 Kelvin 档位(INFERRED)与 LUT 生成逻辑保留,供具备像素变换管线的面板使用。

**Overdrive(诚实 Unsupported,硬件缺席)**:21VG 无 \\.\SPBTESTTOOL PnP 设备/INF,BIOSOD GetFeatureValue(0x1A0000) 返回 null —— 原版 UI 亦无此项。
