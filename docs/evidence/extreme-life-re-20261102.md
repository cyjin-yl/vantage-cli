# 极限续航：原版策略、公开 API 与完整替代边界（2026-11-02）

## 结论与证据等级

`battery extreme-life {on|off}` **仍为 Unsupported，不是已完成的原版替代能力**。
本次没有增加只做电源计划、优先级或 EcoQoS 的替代开关，也没有装载任何联想 DLL、
执行原版程序、安装驱动或向目标机发送请求。以下为精确版本的静态控制流、数据和
公开 API 对照，不是省电效果、目标机写入或故障恢复验收。

此前“只差省电方案 GUID/WinJobList 格式，因此可直接纯 Rust 复刻”的结论不成立：

1. **没有固定的极限续航方案 GUID**。原版捕获当前活动方案，只改变它的三个 DC 索引，
   然后重新激活同一 GUID。完整 GUID、值和持久化恢复格式已恢复，见 §2。
2. `WinJobList` 的已定位使用是 **HKCU 键删除**，不是已证明的进程白/黑名单读写；
   没有依据给它编造值结构，见 §3。
3. 完整策略还包含精确进程关系选择、0.8% Job CPU 硬上限、独立 EcoQoS/优先级、
   窗口最小化、弹窗关闭及后台进程终止。`off` 不会重新启动被杀进程或恢复关闭的窗口。
4. Job 的**同步进程创建回调、父子成员迁移及句柄生命期**是原版契约的一部分。
   已恢复的单次限流可以用公开 Job API 实现，但轮询/ETW 延后修正不是已证明的等价回调。
   完整切换仍需要替代项目自己拥有、适当签名的内核执行器及对应用户会话守护，不能
   依赖受联想签名门控的旧驱动端口。另须取得实际设置前端的单开关写入映射，见 §8。

本报告替代 `interfaces.md` 中把路径写成 HKLM、把行为缩成 EcoQoS 守护的旧概括。
研究用原版文件仅作契约来源，**不是准入的运行时依赖**。

## 1. 精确来源与绑定

官方包：Microsoft Update Catalog 项目 `950615e5-5491-4d9e-b0e8-caf9bb0f0f0e`，
[原始 CAB](https://catalog.s.download.windowsupdate.com/d/msdownload/update/driver/drvs/2026/02/c4f86393-34e6-4f9d-b098-ee707916a533_04d0f7366812c168c003dee9fff29faa52231fc5.cab)。
CAB SHA-256：`f3750d4b91e6888b1f32e706f1cf9a75a3baf33c29ba39bc597e464748afa6e8`。
INF 为 Lenovo AI Turbo `1.0.0.12311`、日期 `2025-01-01`、`ACPI\IDEA2012`。
原留存文件在 `/tmp/vantage-re`；官方包解出文件在 `/tmp/aiturbo-policy-native`。
四个既有二进制与官方包成员逐字节相等。

| 文件 | 大小（B） | 文件 SHA-256 |
|---|---:|---|
| BatteryLife.exe | 1230256 | `b41dc6b8f63222fcd0127d043bac9e969be8e0140604afcdc63b5129923ea87d` |
| AiPerfSvc.exe | 868864 | `dd41ce1f482b18ec0e59f6082023e1af9d15884fd94b1f75ca7f0b9c467ca19a` |
| Workman.exe | 2128296 | `5909c8d9ca10cf5ffccefbdb78b7567c165e5c6a28a7f555ea9dc81f8c3ae2ce` |
| LseFileMonitor.sys | 41384 | `f7e1de322aa3bd1e6b0ae9c1b5beb9dda402b2264a8ff3c1e4d028ed131a9ab4` |
| Lenovo.CertificateValidation.Native.dll | 366592 | `7b368db17c74de6a44461b86d14fa2ecb0502960a69cd616116bc2a28d1639f7` |

BatteryLife 文件版本为 `1.0.0.11145`。下文 RVA 均属于明确指出的文件；前三个 EXE
和驱动的首选基址为 `0x140000000`，证书 DLL 为 `0x180000000`。

[完整语义表](extreme-life-policy-20261102.json) 保存 322 次目录初始化赋值、
最后赋值生效的 314 个进程名、限制例外、清理分组，以及证书准入标量列表及其 RVA。
它不是可执行实现、二进制描述符转录或运行时验收收据。

### 1.1 证书提供方不是“同名 DLL 即兼容”

BatteryLife `0x7d5f0` 只接纳相邻 DLL 的特定 PE 图像摘要：
`5B6AAC903AA06F0346C03E5FD4F96A4BFC7E6D8085F6506A97BBE8CC1EEA893F`。
此值**不是整个文件的 SHA-256**。`0x7d090` 的散列流跳过：

- `e_lfanew + 0x58` 的四字节 CheckSum；
- `e_lfanew + 0xa8` 的八字节 Security Data Directory；
- 该目录指向的证书表文件区间。

所获 DLL 的实际排除区间为 `(368,4)`、`(448,8)`、`(345088,21504)`；计算所得摘要
与装载器常量完全一致。DLL 后续仅被静态分析，未执行。不能以另一个 Addin 中的
同名文件、整个文件摘要不匹配或普通 Authenticode 签名代替此身份绑定。

## 2. 当前方案上的三个 DC 设置与恢复

BatteryLife `CFeatures::StartSetPowerScheme`：RVA `0x421c0–0x43467`。
`PowerGetActiveScheme(NULL,&guid)` 得到的 **当前活动方案**是全部读写和重应用的对象；
不选择 Balanced/Power Saver 等固定方案，不写 AC 索引，也未在此路径发现充电阈值写入。

| JSON 名 | subgroup GUID（字节来源 RVA） | setting GUID（字节来源 RVA） | DC 目标 |
|---|---|---|---:|
| `wlan` | `19cbb8fa-5279-450e-9fac-8a3d5fedd0c1` (`0xebc08`) | `12bbebe6-58d6-4636-95bb-3217ef867c1a` (`0xebb00`) | 3 |
| `desk` | `0d7dbae2-4294-402a-ba8e-26777e8488cd` (`0xebb10`) | `309dce9b-bef4-4119-9921-a851fb12f0f4` (`0xebbf8`) | 1 |
| `pcie` | `501a4d13-42af-4429-9fd1-a8218c268e20` (`0xebaf0`) | `ee12f906-d277-404b-b6da-e5fa1a576df5` (`0xebc20`) | 2 |

语义分别是无线最大节能、暂停桌面背景幻灯片、PCIe 链路最大节能；API 调用点为
`0x42529/0x426c4`、`0x4280b/0x429a6`、`0x42aed/0x42c88`。
每项只在读取成功且旧值不等于目标时写入；对应旧值保存到同一个 JSON 对象。
只有存在尝试写入的项目才生成恢复值并 `PowerSetActiveScheme(NULL,guid)`（`0x432bb`）。
分支并不以每次 `PowerWriteDCValueIndex` 的结果证明成功，最后返回真也不是效果回读。

### 2.1 `PowerSchemeSetting` 的真实格式

位置为 **`HKCU\Software\Lenovo\AiTurbo\BatteryLife`**，值名
`PowerSchemeSetting`，类型 `REG_SZ`，内容为数值型成员组成的 JSON 对象，**不是 GUID
字符串，也不是另一个方案名称**。写帮助函数 `0x6a510` 中的根句柄为
`0xffffffff80000001`（HKCU），类型为 1。结构如下：

| 成员 | 内容 |
|---|---|
| `wlan` / `desk` / `pcie` | 对应被改变项目的原 DC 值；未改的成员可以不存在 |
| `g_1` | 捕获 GUID 的 Data1（u32） |
| `g_2`、`g_3` | GUID Data2/Data3（u16 作为 JSON 数值） |
| `g_40`…`g_47` | GUID Data4 的八个字节，各为 JSON 数值 |

`CFeatures::RestorePowerScheme`（`0x43470`）读取 JSON、重建捕获 GUID，只对存在的
旧值写回；缺项哨兵为 `0x00ffffff`。它重应用捕获的方案，而不是仅恢复“目前碰巧活动”
的方案。旧兼容成员 `cpu` 有读取分支，但本版本 Start 未生成它、Restore 也未以它
下发 CPU 电源设置；不能据此增加处理器功率/上限写入。

**生命期及失败边界：** vtable `0xef958` 的 Init 和 Stop 都指向 Restore，Start
指向上述写函数；因此守护启动会先尝试处理遗留恢复值，模式退出/正常停机会再次恢复。
原版先改 DC 值、后写恢复记录；Restore 又在解析/恢复前调用删除值帮助函数（`0x4377b`）。
这些顺序在失败/崩溃时会丢失恢复信息，不能把它描述为可靠事务。替代实现必须先持久化
完整原值、逐项回读并在恢复确证前保留记录；重复 `on` 不得覆盖最初快照。外部切换方案
时还须显式处理并发所有权，不能把“请求成功”当作独占/原子保证。

## 3. `Enable`、`EnableEcoQos` 与 `WinJobList`

读取函数 `0x67ee0` 分别读取同一 **HKCU** 键下两个四字节值。查询成功且数值恰为 1
才产生 true；其他数值/查询失败产生 false。读取帮助函数虽然预置 DWORD 类型和大小，
但没有独立验证返回类型。两个值独立保存、独立处理，不能擅自把单个 CLI `on` 定义成
“同时把两值都写 1”。原版以 `RegNotifyChangeKeyValue(subtree=TRUE, filter=4,
async=TRUE)` 观察，并在每次处理后重新布防。

`Enable=false` 时选强制禁用行 `(mode=0,isAC=1)`；变回 true 时使用保存的真实 AC/mode。
关闭 EcoQoS 子项会发送清除请求，不等价于停止其他七类效果。

`WinJobList` 唯一已定位的字面引用位于 Restrict Stop：
`RegDeleteKeyW(HKCU, L"SOFTWARE\\Lenovo\\AiTurbo\\BatteryLife\\WinJobList")`
（`0x5ace9–0x5ad26`）。它前面发送 `job_resume_all`、`CLEAR_ECOQOS` 并清空 PID 请求表。
本次未建立该键的生产者、读取者、值名/类型或其对选择器的输入关系；
**删除不是白/黑名单结构证据**，不能据此创建猜测的注册表名单。
真实名单和关系来自下述静态目录、窗口、音频及进程树路径，而不是这个键。

## 4. 激活条件与原版会话生命期

### 4.1 精确策略表

`0x69de0` 生成八个 16 字节行：`u32 mode, u8 isAC, u8 enabled,
flags[8], padding[2]`；无效 mode 归零。表为：

| mode | AC | enabled | 八项 flags |
|---:|---:|---:|---|
| 0 | 0 或 1 | 0 | 全 0 |
| 1 | 1 | 0 | 全 0 |
| **1** | **0** | **1** | **全 1** |
| 2 | 1 | 0 | 全 0 |
| 2 | 0 | 1 | 全 0 |
| 3 | 0 或 1 | 0 | 全 0 |

这些数值**不是 Windows 电源 overlay 枚举**。AiPerfSvc `0x269b0` 以
`GetSystemPowerStatus` 的成功结果且 `ACLineStatus==1` 判 AC；独立 mode 来自
`HKLM\SYSTEM\CurrentControlSet\Services\LenovoProcessManagement\Performance\PowerSlider`：

- 必须能打开 `LenovoProcessManagement` 服务及该键；打开句柄不证明服务 Running。
- `Version` 必须恰为 `0x2000` 或 `0x2001`，`ITS_FN_Capability` 必须为非零 DWORD；
- `ITS_CurrentSetting` 的映射是 `0→1, 1→2, 3→3, 4→4`，其他/失败为 0；BatteryLife
  把 4 再归零。**mode-1/DC 对应成功来源值 0**，不是随意选一个“节能 overlay”。

本版本值名含下划线的具体位置如上，不应把其他资料中的 `ITS_CURRENT_SETTING`
当作相同值名。直接用公开注册表 API 读取这个版本化来源不需要装载联想 DLL，
但仍不证明脱离原状态生产者后有等价、实时的 mode 来源。

### 4.2 八项与启动阶段

| ID | 功能 | vtable RVA | Start / Stop / RunOnce RVA |
|---:|---|---|---|
| 0 | CProcessRestrict | `0xf2d58` | `0x5a6b0 / 0x5aa50 / 0x5b600` |
| 1 | CWindowRestrict | `0xf6e88` | `0x77130 / 0x773e0 / 0x778a0` |
| 2 | CGarbageCollects | `0xf0f60` | `0x464c0 / 0x46610 / 0x46760` |
| 3 | CProcessKG | `0xf10d8` | `0x50e70 / 0x51110 / 0x51450` |
| 4 | CFeatures（方案） | `0xef958` | `0x421b0 / 0x421a0 / 0x4b020` |
| 5 | CResidentCollection | `0xf1000` | `0x61b80 / 0x61cd0 / 0x61dd0` |
| 6 | CClosePopMsg | `0xeecd0` | `0x3bd60 / 0x3bf80 / 0x3c080` |
| 7 | CProcessInformation | `0xef440` | 无独立 Start 副作用 / `0x4ef10 / 0x4f010` |

管理器先对全部预建对象运行 Init；变化时逐 flags 停止旧对象或挂载并 PreStart 新对象，
随后阶段字段 `0→1→2`，通过 `CheckBatteryLifeState` 执行实际 Start。
工作循环对所有已挂载对象 RunOnce，取返回超时的最小值并等待事件；
这不是一次性 CLI 生命周期，也不是只枚举 CPU top-100 的循环。

### 4.3 默认值、启动及退出

AiPerfSvc 的自动服务由 INF 安装，LseFileMonitor 为 System-start 文件系统过滤驱动。
服务对活动控制台用户取 token/SID，写 **HKU\<SID>\Software\Lenovo\AiTurbo** 下默认值：

| 可打开的服务 | BatteryLife Enable 默认 | EcoQoS 默认 | 处理已有 DWORD |
|---|---:|---:|---|
| LenovoVantageService | 0 | 0 | 保留 |
| 仅 LenovoSmartService | SmartEngine 版本条件 | 0 | 保留 |
| 两者均不可打开 | 0 | 0 | 强制覆盖 |

版本条件读取 `HKLM\SOFTWARE\Lenovo\SmartEngine\DisplayVersion`，四段都需解析；
判定 `major>1 || (major==1 && (minor!=0 || third>139))`，第四段不比较。
这也说明“没有客户端 DLL 依赖”不能被改写成“卸载官方服务已经证明无影响”。

服务 OOBE 门控可等待一小时；`oobefinishtime` 是服务 HKLM 键的 QWORD 时间记录。
其后将 `Performance.exe/T1001`、`BatteryLife.exe/T1002` 作为用户 token 子进程，
Workman 为另一路 `T0100` 执行器，子进程通过继承管道交换 NUL 结尾 JSON。
Console-connect/logon 重新初始化默认值并更新会话；console-disconnect/logoff 清会话，
remote-connect/disconnect 不执行相同更新。管道读结束会关闭记录并尝试重拉起，失败后
可中断等待 5 秒。服务停止关闭/取消管道，BatteryLife 的读失败路径 Stop、join 工作
线程后退出；服务等待线程结束不是子进程全部恢复的独立确认。不能把此路径当作
强杀、崩溃或失联后的恢复证明。

## 5. 原版选择器与不可逆效果

### 5.1 进程关系与限制

全量 PKG 源为 Toolhelp 进程树、初始/事件窗口、音频 PID 和启动时收集的已安装产品。
`explorer.exe/svchost.exe/services.exe` 是父节点边界，而不是递归连接所有系统服务。
窗口路径和事件路径条件不同；WinEventHook 的 cloak/uncloak、hide、foreground 事件
不能以仅轮询前台 PID 代替。

`RefreshProcessRestrictv2`（`0x5ca30`）枚举全部进程；有效 PID 若在 PKG 的任一根或
任一成员中就请求限制，不在则仅对已跟踪 PID 请求解除。其 classifier 的 0/1/3/4
类别全部限制，2 才解除；不能因为第一个根叫 `kg_main` 就把它改写为始终豁免前台。
前台和音频到达事件会独立请求解除已有记录，随后关系更新仍可再次改变请求。

有效性过滤依次为：非空 basename；Windows 目录字符串前缀排除；76 个初始化例外字面项
及 `mobilemark` 子串排除；证书提供方 `ValidateAssembly(path)==0` 排除；非零本地首次
观察 tick 的 30 秒等待。路径缓存只在 OpenProcess 失败时回退；成功打开但查询失败
不是同一个分支。首次观察 tick 不是内核进程创建时间。

PKG 种子目录的 322 次赋值/314 个最终键见 JSON。匹配是精确 basename，目录数值是
类别、不是一张豁免表。关系序列化成员为 `pid/top/stype/rel`，根分别在 `kg_main`
和 `kg_others`；空 `rel` 可缺省。全量窗口表是局部表，windowless-seed pass 对持续表
默认值插入后仅添加成员，未复制根字段；**[静态推断]** 该路径可以输出 `pid=0` 而
`rel` 含真实种子 PID。不能把静态反汇编变成已运行的正确性收据，也不能无说明“修正”
这项选择差异。

路径配对（`0x70f20`）使用 `PathCommonPrefixW` 和反斜线深度：两路径均在正偏移含精确
`WindowsApps` 则拒绝；无公共前缀拒绝；前缀深度不足 2 要求两路径及前缀完全相等；
一方深度等于前缀则接受；否则深度须相等、目录探针为假且相差小于 3。
`Microsoft Office` 特例及两路径字面相同产品名影响附加；这些检查的大小写不能
擅自统一。附加 pass 区分图 key、value.root_pid 及音频 PID，不是所有音频进程永久豁免。

产品目录按原枚举顺序取**第一个** `InstallLocation` 前缀命中的产品名，不是最长前缀。
其来源要求先能打开 HKLM Classes\Installer\Products，再扫 32/64 位卸载项和 HKU 各子项。
`WindowsInstaller` 是查询成功而非数值 1 判据；更新项目不被整体排除，显示名按区分
大小写去重。回退安装目录仅去除卸载串中所有双引号并截最后反斜线，不展开环境变量、
不解析命令行参数、不做路径规范化。MSI GUID 去 `{}`，前三段逆序、后两段逐对换位。

### 5.2 证书排除不是任意 Lenovo 发布者

精确提供方 `ValidateAssembly` 导出 `0xa5c0→0x3b70`；只接受 WinVerifyTrust 的嵌入式
签名成功或 `CERT_E_CHAINING (0x800b010a)` 进入自有判据。未签名返回 3，文件不存在 4，
空参数 5，其他初步信任失败 2；存在签名但不符合下述条件返回 1，匹配返回 0。
它枚举主/附加签名并取 signer 证书记录，不把同 DLL 的 `IsCatalogSigned` 导出自动
当成 ValidateAssembly 的回退。

接受条件为：

- 叶证书 SHA-1 精确命中默认允许列表（57 次初始化赋值、56 个不同指纹）；**或**
- 叶证书不等于特殊旧指纹的两种大小写形式；其 Subject 满足三种字段模板之一，
  NotBefore 严格晚于 `2016-02-28 00:00:00 UTC`，并且第二个链元素命中 4 项列表或
  根元素命中 7 项列表。

模板包含 Lenovo/Morrisville/North Carolina/US、Lenovo/Beijing/CN，**以及**
Motorola Mobility LLC/Chicago/Illinois/US。Subject 通过 `CertNameToStrW(...,3)`
转字符串、逗号分项、等号分键值和 trim，再以不区分大小写的 key 与折叠后值子串匹配；
不是单纯 `Publisher == Lenovo`。链通过公开 CryptoAPI、cache-only 标志及嵌入的七张
辅助证书建立。所有标量指纹、模板、辅助证书身份和 RVA 均保存在 JSON，原 DLL 不准入
运行时。BatteryLife 按路径缓存此结果；提供方缺席时替代状态 6 产生“不豁免”，不是
“已验证非 Lenovo”。

### 5.3 窗口和清理

- **窗口覆盖（ID1）**：10 秒 QPC debounce，实际乘数为二进制 double `1000.0`；余时
  小于 500 ms 就扫描。按 Z-order 遍历；要求有效、可见、非 iconic、非 toolwindow、
  非 DWM-cloaked；窗口及工作区交集宽高均严格大于 25。每显示器 covered 是布尔值，
  只由之前遇到的 maximized/全屏窗口设置，不是遮挡矩形的几何并集。全交集已覆盖且
  `(style & 0x08c20000) != 0x00c00000` 的候选还受 owner、同 PID 非 topmost 基础窗口、
  三个调试名例外过滤。七个邮件/Teams 基础名见 JSON；恰一个有效 Teams 窗口有额外
  topmost 候选分支。结果发送 `WM_SYSCOMMAND/SC_MINIMIZE`，条件满足时另发 EcoQoS。
- **弹窗关闭（ID6）**：全局 foreground hook 在其后通用窗口验证前，就对 topmost 窗口
  排队 kind1；vtable `0xeecd0+0x30` 精确指向 `0x3c0b0`。窗口须在主工作区右下半部内：
  `left>width/2, right<=width, top>height/2, bottom<=height`。15 秒后发送 `WM_CLOSE`。
  原版按 HWND 去重，发送前不复核 PID/窗口身份；Stop 不重新打开窗口，也未清空全部旧队列。
- **垃圾子进程（ID2）**：七组精确成员及 type 见 JSON。组内无 monitor 项时终止 kill
  成员；第 4 组中文名为 `芒果tv.exe`（不是乱码）。Edge 单独在 60 秒检查后、找不到合格
  可见窗口时枚举并终止匹配进程。直接路径 `OpenProcess(PROCESS_TERMINATE)` /
  `TerminateProcess(...,0)` 失败后请求 Workman 按名处理。
- **常驻收集（ID5）**：每 Start 在进程初始收集完成后只扫一次。五个分组若有匹配 PID
  而没有保护窗口，就对整个分组终止，非工作集收缩。保护谓词精确为
  `!toolwindow && (visible==1 || iconic==1) && (WS_VISIBLE || iconic!=1)`。

以上最小化、关闭、终止都不是可由 `off` 完全恢复的写入。替代的 dry-run 必须列出
当前明确身份的拟作用进程/窗口，并明确未来策略、会话边界与不可逆效果；不得隐式
扩大为不受限的按名全机杀进程。

## 6. Job 和 EcoQoS 的精确执行面

### 6.1 LseFileMonitor，不是暂停进程

AiPerfSvc 将 `job_set_rate/job_resume_all/job_get_list` 映射为 FilterSendMessage 命令
5/6/7。命令 5 总长 20B：`u32 command=5, u32 rate, u32 zero, u64 pid`；PID 位于
不对齐的线偏移 `0x0c`。驱动 `0x1bf0` 要求 16B payload，`0x3334` 要求 PID>4、
rate 在 1..10000。已知 PID 更新 job；未知 PID 的 10000 请求不创建 job。

新限制的顺序是 CreateJob → extended limits → CPU rate → assign process → 发布记录：

- `ZwCreateJobObject`：未命名、`OBJ_KERNEL_HANDLE`、访问 `0x1f003f`；
- `ZwSetInformationJobObject(class=9, size=0x90)`：x64 extended limits 中
  `LimitFlags=0x1800 = BREAKAWAY_OK | SILENT_BREAKAWAY_OK`，没有 KILL_ON_JOB_CLOSE；
- `class=15, size=8`：`{ControlFlags=5 (ENABLE|HARD_CAP), CpuRate=rate}`；
- 用 PID 打开进程并 AssignProcessToJobObject，关闭进程句柄、保留 job 句柄/记录。

因此 80 为 **0.8% CPU 硬上限**，不是 80%、freeze 或 SuspendThread；嵌套 job 的
百分比相对有 rate-control 的父 job，不能一概称整机 0.8%。解除请求是改到 10000，
不是解除成员关系。命令 6 遍历所有 job 改 10000，忽略个别失败且保留记录/句柄；
命令 7 只返回 rate<10000 项，8B header 后每项 16B `{u32 rate,pad4,u64 pid}`，
padding 未由所见写入初始化，不得比较其无意义字节。

**成员生命期是替代前置条件：** 内核进程创建回调先处理 job；若新子进程确在父亲的旧
job 中，驱动把父记录迁到新 job（原 rate），将旧 job 改 10000 并关闭旧句柄，不把子
进程迁入新限制 job。进程退出/驱动卸载会删记录、尝试 10000、关闭句柄。客户端断开
只关端口，不执行全恢复；客户端崩溃可留下 resident driver cap。并发查找和发布之间
也不是单一原子区间。关键 RVA：`0x2ea0/0x2f90/0x2d2c/0x3160/0x2fdc/0x3428`。

连接端口 `\{994676AB-9EC9-4B11-83A2-4F5C107BC1B9}` 还有联想私有 ECDSA-P256 门控：
连接进程映像的最后非空 raw section 后，必须有 64B 签名和 `LENOVO AI TURBO\0`
标记，验证 PE header/section SHA-256。普通 Windows 签名不代替该准入；新编译客户端
不能假设可复用原驱动。文件 post-read 回调仅异步通知，BatteryLife 的 file_message
分支明确忽略它，**不是该限流的文件读取唤醒机制**。

公开 [Job Object API](https://learn.microsoft.com/en-us/windows/win32/procthread/job-objects)
及 [CPU rate 结构](https://learn.microsoft.com/en-us/windows/win32/api/winnt/ns-winnt-jobobject_cpu_rate_control_information)
能表达 class9/class15 单次效果，但没有据此建立任意进程同步创建回调的用户态等价。
完整替代需要自行拥有并适当签名的内核执行器，不能偷偷改为轮询修正。

### 6.2 Workman

`SET_ECOQOS` 请求参数是整数 `pid/set`；仅 set==1 视为开启。
Workman `0x4ab30` 先 `OpenProcess(0x200,FALSE,pid)`，然后
`SetProcessInformation(class=4,size=12,{Version=1,ControlMask=set,StateMask=set})`，
成功后 `SetPriorityClass(set ? 0x40 : 0x20)`（IDLE / NORMAL）。
[公开结构](https://learn.microsoft.com/en-us/windows/win32/api/processthreadsapi/ns-processthreadsapi-process_power_throttling_state)
是三个 ULONG，不使用通用 affinity/working-set profile。

clear-all 枚举所有 PID；仅原优先级 IDLE 的 PID 做 `{1,0,0}` 和 NORMAL。它不是
`{1,1,0}`，也不是回到记录的原优先级。原 OpenProcess 失败可以被包装成成功；正常
请求包装也会丢弃 setter 返回。因此请求收据不是独立效果确认。解除及失败恢复应
限定替代自己管理的同身份对象、保留原错误，不能复刻为全机无区别清除所有 IDLE。

`CProcessRestrict::DoWinJob` 顺序先发 rate，再按独立 EcoQoS flag 发 set/unset，后者
即使前者发送失败仍尝试；返回的只有 rate 消息写管道结果，不是驱动完成状态。
前台/音频事件、screen-off、Stop 和启动恢复还有独立解除路径，见前文。

## 7. 本次交付与未实施事项

- 已恢复问题中的准确方案/设置 GUID、备份 JSON、HKCU 权限域、策略门控、八项效果、
  公开执行 API、私有驱动准入和进程生命周期；完整静态语义表随 JSON 留存。
- 已将官方包内提供方与已分析客户端绑定，并恢复证书过滤，不再把未知 DLL 行为或
  乱码/未解码字符串当作缺失前置条件。
- `WinJobList` 没有可证结构；未猜造 registry 列表，也未用它虚构完整功能。
- **没有 Rust 功能代码变更**：签名内核生命周期和真实 UI 写入映射尚未具备；现有
  Unsupported 保留。没有引入不能被完整调用的孤立执行器、无效守护或“方案已生效”假成功。
- 没有构建、测试、格式化、目标机 smoke 或硬件写入。源文件字节比较、摘要绑定、
  反汇编/数据解码是来源调查，不是运行时验收。

## 8. 精确剩余前置条件与 smoke 程序

### 8.1 完整替代的前置条件

1. **替代拥有的内核 job 生命周期**：实现已恢复的同步进程创建/退出、父子 job 迁移、
   retained handles、10000 恢复及 owner/卸载清理；取得目标 Windows 安全策略认可的
   签名/部署能力。不能依赖 Lenovo LseFileMonitor 或其私有签名键，也不能把 ETW/轮询
   近似宣称成原版同步回调。
2. **真实单开关生产者映射**：保留 Dashboard `26.0.1012` 仅将
   `/device/device-settings/power` 路由到 `rainbow`；其 source map 并无 AiTurbo/两键的
   写调用。需要该目标实际选择的 Rainbow 包/源映射或相应 PCManager 开关处理方，确认
   一个 UI 操作究竟写哪个 flag、是否顺带更改 ITS 状态。公开 Rainbow URL
   `https://vantage.csw.lenovo.com/v1/web/rainbow/default/` 本次返回 403；不是“控制不存在”。
   额外留存的 22 个服务/驱动 PE 对四个 AiTurbo/EnableEcoQos 字面项的 ASCII/UTF-16
   扫描均无命中；这仍不排除动态组成路径的处理方。
3. **完整、会话隔离的守护**：按准确版本化 mode 来源、窗口/音频/树/产品及证书规则，
   连接上述内核执行器；明确 desired-state、恢复快照、进程/窗口身份及变化时并发边界。
   注册表值存在、daemon 进程存在或管道回复成功都不能单独发布为能力 A。

### 8.2 当前代码的拒绝 smoke（仅在主代理允许并完成合并检查后运行）

在已校验来源的当前 Windows CLI 上，按以下两条执行；本次没有执行：

```powershell
.\vantage.exe --json --dry-run battery extreme-life on
.\vantage.exe --json --dry-run battery extreme-life off
```

预期均为非零退出和 `Unsupported`，而不是成功 plan/committed。该 smoke 只证明当前
门禁诚实，不证明极限续航支持。两次前后独立记录活动 GUID、上述三项 DC 索引、
HKCU 两 flag/备份值、受测进程优先级/节流及窗口状态，不应出现变更。

主代理已在 Linux 构建后实际启动 CLI，分别执行上述 on/off dry-run 参数：两次均退出 3，
stdout 为空，stderr 为 `kind=unsupported` 的 JSON，说明完整内核生命周期与目标 UI
开关映射前提，并指向本报告。这仅验证当前 CLI 拒绝路径和诊断，不是 Windows 原生
零副作用读回或极限续航验收；目标机 SSH 不可达，上述 Windows 状态对照尚未执行。

### 8.3 完整实现后才可授权的原生验收

- 先在隔离用户会话准备可丢弃应用，保留方案 GUID/三个 DC 原值、两独立 flag、
  当前 mode 来源及签名、拟作用 PID+创建时间/路径、HWND+PID、父子图、job 查询句柄。
  禁止在所有者正常使用的桌面上测试杀进程/关窗。
- dry-run 必须枚举明确身份目标及每类副作用；未显式确认不提交。`--yes` 确认所示
  计划后真实写出，但不代表物理效果已验收，也不得扩张到未列出的跨会话匹配。
- 对 mode-1/DC 下 on、重复 on、EcoQoS 独立变化、mode-2/DC、AC 返回、screen-off/on
  （含 60 秒等待）、正常 off、正常 daemon 停止逐项采集独立状态。通过公开
  `PowerReadDCValueIndex`、`GetProcessInformation`/`GetPriorityClass` 及复制的只读 job
  查询句柄上的 `QueryInformationJobObject` 观察，不以同执行器日志自证效果。
- 对自己创建的父/子进程检查创建时成员、迁移后的旧/新 job rate 和正常退出清理；
  观察者必须绑定同一进程身份。分别证明执行器操作与完整选择器，不能给工作进程改名
  冒充真实应用就声称原版匹配和签名策略均获验收。
- `off` 后三项恢复到捕获原值；不能以重新激活 GUID 代替索引恢复。窗口关闭/进程终止
  如实记录为不可逆，不能宣称已完整回滚。另行验证错误、owner 崩溃、失联和重启路径，
  不得从正常 stop 或一次注入错误推导全部故障恢复。
- API 状态/驱动 rate 回读只证明请求和状态；实际 CPU 调度、交互响应、耗电和续航时长
  是不同验收层次。本次没有进行任何一层的目标机原生验收。
