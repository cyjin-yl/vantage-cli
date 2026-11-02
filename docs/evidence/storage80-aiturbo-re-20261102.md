# PowerBattery.dll / AiTurbo 逆向恢复报告 — 2026-11-02

来源:`C:\ProgramData\Lenovo\Vantage\Addins\IdeaNotebookAddin\1.0.13.114\PowerBattery.dll`
(145,376 B, v1.0.13.114, 原生 C++) 与 DriverStore `aiturbo.inf_amd64_6a35ee167b41705a`
中的 `BatteryLife.exe` 1.0.0.11145 / `AiPerfSvc.exe` 1.0.0.8 / `PreloadDll.dll` /
`LseFileMonitor.sys` 1.0.0.17。本报告的反汇编工作只读；后续目标机写入观察另行列出，
不与原先只读探测混称。PowerBattery.dll SHA-256：
`df7a014a15630dbfa36603014e15b8b729ec500840b6f45cfb853bb957c10e91`。
托管侧映射参考 IdeaBatteryAgent.dll / BatteryManagementContract.dll 的 IL；本报告不把
类名、能力位或驱动状态回读当成物理停充门限验收。

## Q1 · Conservation 分发与 Storage80 元数据（纠正）

### 类身份和虚表：03/05 属于 Storage，07/08 属于 Express

构造函数 `CChargingMode::CChargingMode`（RVA `0x9790`）建立两个成员：

| 成员 | 构造时写入的 vftable VA | RTTI 类型 | `+8` OpenFeature | `+0x10` CloseFeature |
| --- | --- | --- | --- | --- |
| `m_pFirst` | `0x180017e78` | `CStorageMode` | RVA `0xc3d0`：`03`，满足 subtype 2 时追加 `0d` | RVA `0xc580`：`05`，满足 subtype 2 时追加 `0f` |
| `m_pSecond` | `0x180017e28` | `CExpressMode` | RVA `0xbe90`：`07` | RVA `0xbfb0`：`08` |

上述 vftable 的首项（`+0`）是析构函数，不是 OpenFeature。类型身份还由 RTTI
交叉绑定：Storage 的 Complete Object Locator RVA `0x198f8` 指向 TypeDescriptor
RVA `0x21560`（`.?AVCStorageMode@PowerBattery@@`）；Express 的 Locator
RVA `0x198b8` 指向 TypeDescriptor RVA `0x21530`（`.?AVCExpressMode@PowerBattery@@`）。
此前把 `0x180017e30` 当成 Storage 虚表、进而将 `07/08` 标成 Storage80 开/关的
结论撤回；它们是 Express 的槽位，**不是 Storage 的选择子**。

### constructor subtype 与完整原生下发序列

构造函数用 GBMD `0x831020f8`、in=`[ff]`（1 B）、out=4 B 查询状态。
RVA `0x9865` 测试 `out & 0x4000`（**bit14**）：默认 subtype=`1`，查询成功且
bit14 置位则 subtype=`2`，在 `0x9875` 存入 Storage 对象 `+0x10`。
查询失败时原版仍保留默认 subtype 1；这不是客户端可以把未知能力当成已知的依据。

`CStorageMode` 的两个方法均经同一 `0xad80` helper 访问 `\\.\EnergyDrv`：

- OpenFeature：`0xc3e8` 先发 **`03`**，inLen=1、outLen=4；IOCTL 失败即返回假。
  首步成功后 `0xc4b8` 检查 subtype 是否为 2，只有为 2 才在 `0xc4c2` 追加 **`0d`**。
- CloseFeature：`0xc598` 先发 **`05`**；首步成功后 `0xc668` 检查 subtype 是否为 2，
  只有为 2 才在 `0xc672` 追加 **`0f`**。追加请求失败也返回假。

因此原版序列是 `03` / `05`（subtype 1），或成功门控的 `03 → 0d` /
`05 → 0f`（subtype 2）。**`0d/0f` 是追加步骤，不是替代 `03/05` 的另一组命令。**
目标基线 `0x00860004` 的 bit14 为 0，故对应 subtype 1。此次只证明了 subtype 2
序列与该 DLL 一致；目标机没有 subtype 2 的实测，更不构成物理安全证明。

### 原生 mode 分发、状态与返回值

`SetChargingMode`（RVA `0x9c30`）的 mode 是 `0=Normal, 1=Storage, 2=Quick`：

- mode 1：先调用 Storage.OpenFeature；成功后，仅当 Express 支持且当前开启时，
  再调用 Express.CloseFeature。
- mode 2：先调用 Express.OpenFeature；成功后，仅当 Storage 支持且当前开启时，
  再调用 Storage.CloseFeature。
- mode 0：对支持且当前开启的 Storage、Express 依次调用 CloseFeature。

Storage.IsSupport（RVA `0xc1d0`）在此 x64 DLL 中只是检查 EnergyDrv 句柄能否打开；
Express.IsSupport（`0xbd70`）查询 `[ff]` 后取 bit17。两者不是 bit23 分发。
Storage 当前状态（`0xc0c0`）查询 `[ff]` 后取 **bit5**；Express 当前状态
（`0xbc40`）取 **bit2**。

`GetChargingMode`（`0x9bc0`）先查 Storage，开启则返回 `1`，否则 Express 开启返回
`2`，否则返回 `0`；该实现不产生 `3`，也不把各状态查询失败传播为独立错误。
托管 `FillResponseWithCurrentChargingMode`（IL method RVA `0x25b0`）接受 bitmask
`0..3`（bit0 Storage、bit1 Express），不能据此把 native mode 1/2 的含义颠倒。

`SetChargingMode` 尾部 `0x9fde..0xa085` 按上述状态逻辑最多查询 10 轮，不匹配时
Sleep(50 ms)；仅在读回等于目标 mode 时写注册表缓存，最后返回读到的 mode。
客户端必须自行保留 IOCTL/短回复错误，不能把原版失败时可能回落为 0 的行为当成
已确认 Normal。

### Storage80：bit23 不是完整能力判定，也不是写分发条件

`DoesSupportStorage80`（RVA `0xaa80`）先调用 `GetEmSpecVersion`，用 signed
`jge` 检查版本达到常量后，才查询 GBMD `[ff]` 并返回 `(out >> 23) & 1`。
它与 Conservation 的 `03/05` 路径是两件事；`SetChargingMode` 没有调用它。

`GetEmSpecVersion`（RVA `0x22d0`）的精确契约：

1. IOCTL **`0x8310214c`**，**输入 DWORD `0x11`（4 B，不是 8 B，也不是 4 B 零）**，
   输出 DWORD（4 B）。
2. IOCTL 失败返回 `-1`；成功但输出 bit0 为 0 也返回 `-1`（会记录低位状态）。
3. 只有输出 bit0 为 1 时，返回版本 **`raw >> 4`**；原始 DWORD 不能直接叫作版本。

最小版本的 section/导出绑定已核对：导出表 index 67 / ordinal 68，符号
`?EM_SPEC_VERSION_STORAGE_80@CChargingMode@PowerBattery@@2HA` 指向 RVA `0x21174`。
`.data` VMA=`0x180021000`、fileoff=`0x1ee00`，因此常量文件偏移是
`0x1ee00 + (0x21174 - 0x21000) = 0x1ef74`，该处字节 **`b8 00 00 00`**，即
**184（`0xb8`）**。反汇编中未发现该地址的直接写入；不能用 `.data` 起点处的
内容或版本名称猜一个更低门限。

`storage80-aiturbo-20261102.txt` 保留的旧探测实际输入是 **4 B 零**，返回原始
DWORD `0x00000002`。这不是上述原生请求，且该原始值 bit0 为 0，不能称作
“EM Spec version 2”或用它证明原生版本门限通过。实测只成立的是 GBMD bit23 置位。
完整 Storage80 能力目前仍为**未知**，不能报告真，也不能因版本探测无效就报告假。
原生形态的只读 EM 重测因目标 SSH/Tailnet 传输不可用而待执行。

### 目标观察与旧“运输模式”警告的边界

目标原生 Rust consumer（不是 vendor DLL 调用跟踪）使用 `NativeIoctl` 得到：
`0x00860004 → [03] → 0x00860022 → [05] → 0x00860000`。
随后生产 CLI 恢复原 Rapid，单独 `[ff]` 回读为 `0x00860004`；从 Rapid 基线单发
`05` 则无变化。`05` 只是关 Storage，**不等于恢复原 Rapid**。
这些观察见 `storage80-native-state-20261102.md`；生产 CLI 的
`conservation --dry-run` / `--yes --auto-restore` 另见
`conservation-cli-native-20261102.md`，不混同为先前原始 consumer 的自动恢复成功。

旧文档断言“21VG 的 `03` 有运输模式风险，禁止发送”未给出可绑定到此路径的固件/
关机证据。当前安装 DLL 的 `CStorageMode::OpenFeature` 无条件先发 `03`；上述目标
观察未见关机，且原模式已回读恢复。因此不能继续把 `03` 描述成已经证明的运输模式
选择子。但这也**不证明所有固件/电源条件下普遍安全**，不授权裸发未知命令，更不等于
75%–80% 停充、充电电流、重启持久化、关机效果或独立 Vantage UI 已验收。

### 准入要求

- 写分发必须由成功的 GBMD `[ff]` 状态读取确定 bit14 subtype，不能按 bit23 选命令，
  也不能只发 `0d/0f`。按上述首步成功门控串行执行追加请求，保留每步错误。
- 能力探测与读回成功先于写；捕获原始语义模式。回读/恢复都重读 `[ff]`，按 bit5/bit2
  解码；由 Rapid 转 Storage 后必须恢复 Rapid（`07`），不能仅发 `05`。
- 除请求返回成功，还必须核对目标 mode、失败/恢复结果；同一驱动的分次回读不是独立
  硬件传感器。仅把已观察到的 driver-state/恢复闭环列为已验收。
- Storage80 元数据要求原生形态的 EM 查询成功、raw bit0 有效、`raw >> 4 >= 184` 且
  GBMD bit23 置位。未完成任一环时标未知，不冒充完整能力假值；即使成立也不证明
  物理 80% 门限。任意百分比阈值继续无可用通道证据。

## Q2 · 极端省电(AiTurbo / BatteryLife)

完整调用链、参数、注册表 hive、方案与恢复语义由专项报告
[`extreme-life-re-20261102.md`](extreme-life-re-20261102.md)（语义表
`extreme-life-policy-20261102.json`）裁定；其结论为 `extreme-life` 仍
Unsupported：无固定方案 GUID（捕获并重应用当前方案的三个 DC 索引）、
`WinJobList` 已定位使用仅为 HKCU 删除、完整策略含进程硬上限/窗口与进程终止，
完整切换另需替代项目自有的签名内核执行器与前端单开关映射。本报告撤回此前
仅凭导入/字符串就宣称 BatteryLife “全部可替代”、激活固定省电方案或监控/拉进程即为
完整 AiPerfSvc 行为的结论；不建立它们与 GBMD 停充门限之间未经证明的关联。
`PowerSetActiveScheme`、EcoQoS 名称和注册表路径的存在都不是完整等价实现的充分证据。

## 对实现的直接影响

1. Conservation 使用 `03/05` 基础选择子；bit14 subtype 2 时在对应基础请求成功后
   追加 `0d/0f`。`07/08` 只归 Express，不是 Storage80 的另一组开关。
2. GBMD 是位掩码，不与整数 mode 枚举混用：bit5 Storage、bit2 Express；不能把
   `f8 read == 2` 定义为 Storage80 active。恢复必须重建捕获的原语义模式。
3. bit23 helper 只命名/报告 advertised 位；完整 Storage80 需要有效解码 EM 版本至少
   184 且 bit23 置位，目前缺失有效的目标 EM 读数，不宣称支持或不支持。
4. 物理门限及任意百分比阈值不由上述位或类名确立；完整省电行为也不能以通用 API
   组合替代。本报告只裁定已列出的静态路径和受限 driver-state 观察。
