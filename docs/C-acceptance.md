# 附录 C · 真机验收记录

> 本文件记录每条功能域在目标机(Lenovo 21VG / ThinkBook 14 G8+ IPH)上的**实际验收**
> 证据:执行命令、独立回读来源、结果与恢复方式。只有出现在本文件的功能域才算验收完成;
> 代码存在、单测通过、CI 通过均不替代本文件的记录。

## 0. 验收环境

| 项 | 值 |
|---|---|
| 机型 | LENOVO 21VG / ThinkBook 14 G8+ IPH, Intel Arc B390 (PCI\VEN_8086&DEV_B080) |
| 内屏 | 3072x1920 原生路径 120 Hz, `LENOVO_INTERNAL_PANEL_REFRESH_RATE_DATA` 范围 60..120, 默认 60 |
| 会话 | 控制台会话 1(交互式桌面)。SSH/服务会话 0 无显示拓扑(CCD 路径数 0),显示类验收必须在会话 1 执行 |
| 构建 | `cargo build --offline --release --workspace`(MSVC, 独立 target 目录) |
| 回读来源 | CLI `ChangeReport.previous/requested/actual` + 独立 WMI/CIM 或固件数据块读取,两者必须一致 |

执行显示类验收的会话 1 脚本以计划任务(`-LogonType Interactive -RunLevel Highest`)承载,
输出落盘为审计日志;所有写操作均先 `--dry-run`,写后回读,并在 `finally` 中恢复原状态。

### 0.1 · 38 个功能域的准入基线(2026-10-27)

Gitea `ezra/vantage-cli` 的 #1–#38 均仍为 open;#39 是总追踪,不是完成证明。
当时冻结的本地实现基线为 `work/vantage-replacement` @
`0d149130feb7779bb15ba25095c38b33bd60a58b` 加 C7 的触控板修复工作区变更。
此后已将经验证的实现基线作为 Gitea PR #40 合入 `main` 的 `15766658e16ad6bb98af8d1db707119f79d0b186`;
该合并不是 38 项完整替代验收,后续工作区仍含尚未交付的实验性改动。

逐项保存的 issue 原始验收条件、冻结基线的 111 行原生命令注册表观测、已有 C 节证据索引、远端分支 SHA
和全部 38 项覆盖检查见 `evidence/replacement-baseline-20261027.json`。
程序的只读基线见 `evidence/target-read-baseline-20261027.json`:
info、电池状态/适配器、电源计划、性能模式、风扇清单、温度、调优遥测、隐私状态、
BIOS 密码元数据、MagicBay 检测和 scan 清单共 **12 次实际运行,均退出 0,无超时**。
它们只证明相应读取路径,不证明控制或深度诊断已完成。

| issue | 当前准入结论与未完成项 |
|---|---|
| #1 事件/watch | 原生 WMI 源已有源码;CLI watch 尚未闭环,不得以 daemon IPC 代替 |
| #2 自动策略 | parser/catalog/手动计划已有;自动求值、配置恢复与执行闭环待完成 |
| #3 OSD | 当前未附接真实 overlay/热键动作消费者 |
| #4 能力/切换检查 | 原生矩阵可运行;平台依赖投影、实际服务绑定与替代就绪报告仍需修正 |
| #5 电池/适配器 | C14:来源分组及实时电压/功率只读对照通过;健康/寿命、适配器额定功率等仍待逐字段验收 |
| #6 充电/阈值 | 任意阈值仍 Unsupported。Rapid/Normal 的驱动状态写入与恢复已有记录(C28/C29)。本轮按厂商 bit14 分型修正基础养护 `0x03/0x05` 与追加 `0x0d/0x0f` 的关系；生产 CLI `battery charge-mode conservation --dry-run`→`--yes --auto-restore` 已读回养护并自动恢复 Rapid，另启进程读回 `0x00860004`。见 [生产 CLI 回执](evidence/conservation-cli-native-20261102.md)；同驱动分次查询不等于独立物理 80% 停充或官方 UI 验收。bit23 单独不足以证明 Storage80 支持，旧 EM Spec=2 结论撤销 |
| #7 夜间/临时充电 | 夜充真实写、独立回读及自动恢复已验收(C28/C29);临时模式写选择子尚未恢复,保持只读 |
| #8 USB 供电 | AC/电池模式两态真实写、EnergyDrv 独立回读及自动恢复已验收(C28/C29) |
| #9 电源计划 | 实际枚举成功;高级字段/名称一致性仍须按 issue 逐项验证 |
| #10 性能模式 | C4:ITS V3 可读;写入缺版本绑定的已确认控制契约 |
| #11 风扇 | 清单读取成功;真实 RPM/手动策略/曲线未完成;不可把元数据当实时转速 |
| #12 温度/功耗 | 读取入口成功;有界 interval、源定位和实际功率指标待完成 |
| #13 DBDC | 当前命令矩阵未覆盖;读表、单位与受门控写入待完成 |
| #14 超频/SuperBoost | 当前命令矩阵未覆盖;条件能力与精确适用性待核实 |
| #15 GPU 模式/功耗墙 | 独立枚举仅见 Intel Arc B390 实体 GPU 和两个 ROOT 虚拟显示适配器;不得将虚拟适配器算独显,控制面按具体硬件继续门控 |
| #16 EPP/睿频 | C5/C6 已有真实写入/独立注册表回读/恢复;矩阵的 turbo 声明仍与执行器不一致 |
| #17 PL1/PL2/tau | 尚无可供精确写入与独立回读的目标机契约;不绕过 HVCI |
| #18 进程管理 | Windows 进程通道已接入 CLI(`perf top/boost/throttle`):PID + 创建时间身份守卫、优先级写入后独立回读、`--auto-restore` 复原。2026-11-03 真机验收通过(dry-run 无副作用、写入后独立观察为 AboveNormal/BelowNormal、恢复回到本次快照值),见 [真机记录](evidence/native-acceptance-20261103.md)。线程优先级、亲和性选择、工作集收缩、游戏白名单与前台识别仍未实现;Linux 后端未接入,能力矩阵报 Unavailable |
| #19 profile/省电事务 | 现有部分手动执行/restore;完整目标、配方及复合动作仍须完成。极限续航已恢复真实策略,但同步内核 Job 生命周期与目标版本单一 UI 开关映射尚缺准入前提,命令仍 Unsupported;不以电源计划/EcoQoS 部分动作冒充验收。见 [完整边界](evidence/extreme-life-re-20261102.md) |
| #20 键盘灯 | 当前目标机方法不可用;不得以静态灯光元数据提升写能力 |
| #21 Win 锁/触控板 | C7 仅确认触控板恢复及非中断 smoke;新版禁用循环与 Win 锁尚未完成验收 |
| #22 内屏刷新率 | C1 已验收;要求交互式会话,不以 SSH 无显示路径误判硬件不支持。2026-11-05 独立复核实机复现 `--auto-restore` 缺陷(自适应固件下把 `auto` 当恢复目标,恢复落空且面板停在新频率),已修正为只回放观测到的前一固定刷新率,修正后真机验收待做;模式列表改以 `EnumDisplaySettings` 实枚举核对,见 [证据](evidence/re-panel-refresh-20261105.md) |
| #23 色彩/显示增强 | 修正 pixtx 使用显示输出句柄与 1.1 ABI、超分 get/set 为 92B。独立只读探针在当前输出返回 0 块;超分 caps 仍 0x4000000b。2026-11-05 复核(证据 re-panel-20261105.md):超分能力查询改为契约规定的两趟枚举,写路径改为“读到旧状态才写、写后回读、不一致回滚”;overdrive 原版 owner(BiosOD.dll → `LENOVO_OTHER_METHOD` IDs 0x1A0000)已恢复,但本机 `LENOVO_CAPABILITY_DATA_00` 无 0x1A 家族,保持 Unsupported;护眼 owner(Vantage `EyeCareMode` / PCManager `LenovoMonitorManager.exe`)已恢复,机制为系统 gamma ramp,已实现 `GammaRampApi`/`NativeGammaRamp` 并修正 Tanner-Helland 量纲缺陷,写入验收需交互会话(SSH 网络登录不可验收)。色域仍 Unsupported(依赖被禁的 MONITORCONTROL.dll) |
| #24 运行时隐私 | 状态实际读取成功;逐层写入、独立读回及恢复证据须按版本保存 |
| #25 SmartSense/HPD | **已修复真机访问冲突**。根因:生成的类型格式串把 `[out]`/`[in,out]` 上下文参数写成裸 `FC_BIND_CONTEXT`,而 MIDL 为 `[context_handle]` 参数生成 `FC_RP -> FC_BIND_CONTEXT` 的间接层;同时类型偏移按裸 4 字节记录步进,`TYPE_IN_CONTEXT` 落在第二条记录内部。该错位使解释器直接解引用参数槽,在 `NdrClientCall3` 内触发 0xC0000005。改后上下文区与厂商字节逐字节一致(`1104020030a4` / `1104020030e5` / `3045`)。真机验收:`sense status` 返回 `source=hpd-rpc` 与实时 `advertised=2047`;`sense pause-video on/off` 经 RPC 往返并由独立读回确认。由于 HPD 能力探针在 HAL 构造期运行,此前该崩溃会杀掉**每一条**命令,修复后 `audio dolby` 等通道一并恢复。见 [WMI 穷尽记录](evidence/wmi-capability-exhaustion-20261104.md)。attention-tracking 仍保留独立旧通道与 write-only 边界;键盘灯自动受固件限制(见 #31 行与同一证据) |
| #26 Dolby | 纯 Rust rpcrt4/NDR 客户端不加载 DolbyRpcClient.dll,描述符由本仓库生成(interfaces §0/§12)。2026-11-01 的禁用/启用及 profile 真实写、厂商 DLL 独立读回与 C29 自动恢复已验收(C28/C29)。2026-11-03 观察到的启动崩溃并非 Dolby 通道自身缺陷:HPD 能力探针在 HAL 构造期于封送器内触发 0xC0000005,使每条命令启动即死,从而把 Dolby 也一并掩盖。HPD 描述符修正后真机复验:`audio dolby game` 提交(enabled false→true, profile dynamic→game)、`dynamic` 与 `off` 恢复均成功且无崩溃。仍不宣称物理音效;见 [WMI 穷尽记录](evidence/wmi-capability-exhaustion-20261104.md) |
| #27 音频降噪 | 纯 Rust rpcrt4/NDR 客户端(不加载 ElevocControlSDK,也不内嵌其接口表;描述符由本仓库生成,interfaces §0/§14);仍需要 ElevocService。模式真实写、厂商 SDK 独立读回与 C29 自动恢复已验收(C28/C29) |
| #28 BIOS 批量/元数据 | 原生事务源码与只读密码元数据已有;CLI 批量、风险和实际生效态验收待完成 |
| #29 MagicBay LTE | 当前检测未见附件;实现缺口与附件未连接分别记录,不伪报完成 |
| #30 MagicBay 摄像头 | 当前检测未见附件;UVC 实现及目标附件验收待完成 |
| #31 MagicBay 扩展屏 | 当前检测未见附件;关联与安全切换待完成 |
| #32 可信目录 | 缺已准入的目录/信任/适用性契约;不能靠猜测接口开放安装 |
| #33 下载/驱动安装 | 本地 INF 执行器源码已有;远程下载及主力机安全安装验收未完成 |
| #34 历史/回滚/调度 | 本地策略源码不等于真实任务调度;各生命周期仍须验收 |
| #35 实际诊断 | 清单入口成功不算硬件通过;六类检查、取消/状态与结果语义仍须验收 |
| #36 MachineFix | 当前无命令矩阵条目;透明规则、允许的修复原语和跟踪未完成 |
| #37 系统/固件清单 | 现有 info 读取成功;完整模块 schema、逐模块错误和只读 UEFI 待完成 |
| #38 Wi-Fi 评估 | 当前 CLI 尚未接入现有服务;原生 WlanApi、持久本地历史与权限失败语义待完成 |

每一项适用写入的准入门槛保持为:**实际效果、独立回读、dry-run 零副作用、前值与允许时的恢复**。
缺实现、缺接口契约、当前未连接硬件与型号不适用分别记录;所有必要功能及最小依赖切换验证之前,
不建议停用或卸载任何官方驱动/服务。当前记录不宣称 38 项已完成。

## C1 · 内屏刷新率(显示)

**状态:已验收** — 真实生效 + 独立回读 + dry-run 无副作用 + 失败前检测 + 状态恢复。

证据(2026-10-27, 会话 1):

| 步骤 | 命令 | 结果 |
|---|---|---|
| 拓扑 | `GetDisplayConfigBufferSizes(QDC_ONLY_ACTIVE_PATHS)` | rc=0, paths=1, modes=2;活动路径 = 内屏 `\\.\DISPLAY1`,adapter low=105674,source id=0,target id=8388688 |
| dry-run | `vantage --dry-run panel rate 60 --json` | `mode=dry_run`, `previous.refresh_hz=120`, `requested.refresh_hz=60`;独立回读 `Win32_VideoController.CurrentRefreshRate` 仍为 **120**(无副作用) |
| 提交 60 | `vantage --yes panel rate 60 --json` | `actual.refresh_hz=60`;独立回读 **60**(即时与 3 s 后),缓冲区保持 3072x1920 |
| 提交 120 | `vantage --yes panel rate 120 --json` | `actual.refresh_hz=120`;独立回读 **120** |
| 固件默认 | `vantage --yes panel rate auto --json` | `actual.refresh_hz=60`(WMI `DefaultRefreshRate=60`);独立回读 **60** |
| 恢复 | `vantage --yes panel rate 120 --json` | 终态独立回读 **120**,与验收前一致 |
| 互斥 | 未带 `--yes` 的提交 | 因检测到 `LenovoVantage*.exe` / `LenovoPcManagerService.exe` 等 16 个厂商控制进程而拒绝写入,提示 `--yes`,符合 docs/03 §1 互斥策略 |

写入路径为公开 API `ChangeDisplaySettingsExW(CDS_UPDATEREGISTRY)` + `CDS_TEST` 干跑,
刷新率能力与 auto 语义取自固件 WMI 数据块,不写任何厂商 IOCTL。

### C1 真机暴露并修复的缺陷

1. **重复固件记录**:本机 `LENOVO_INTERNAL_PANEL_REFRESH_RATE_DATA` 有 22 条 `Active=TRUE`
   记录且字段完全一致,旧实现一律判为 ambiguous。改为仅按控制器读取的四字段
   (Minimum/Maximum/Default/Mode)去重:一致则视为同一面板,不一致仍 fail-closed,
   字段缺失/类型错误的记录仍报错。
2. **枚举项缺布局字段**:`EnumDisplaySettingsExW` 的枚举项不携带 `DM_POSITION` /
   `DM_DISPLAYORIENTATION`,旧解码要求全字段导致模式列表为空(`panel.refresh.native_modes`)。
   改为按枚举项的形状(宽/高/bpp/刷新率)建候选,布局继承当前模式;写回前把位置与旋转换填回
   `DEVMODE`,保证只改刷新率。
3. **回归测试**:Windows 上 `cargo test -p lctrl-hal-win --test display_controller`
   = 14 passed;`cargo test -p lctrl-hal-win --lib display::native` = 3 passed。

**限制**:显示切换要求交互式桌面会话;停止 Sunshine/GameViewer 不改变活动路径数(其虚拟适配器
不占用活动路径),因此验收期间未改动它们的运行状态,仅在需要时以 `--yes` 接受并发控制风险。

## C2 · 电池充电模式:已测写入被接受,已测回读未见变化

**状态:表中写请求被接口接受；硬件实际效果与独立回读尚未验收。**

证据(2026-10-27,`\\.\EnergyDrv` 文档化命令字差分测量):

| 步骤 | 命令 | 结果 |
|---|---|---|
| 基线 | `0x83102120`(全局充电配置,20B) | `E0 00000000 …`(全零尾) |
| 基线 | `0x83102138`(电池详情,83B) | 调用方按 48B 读取返回 122(缓冲不足);83B 读取由 `battery status` 正常解析 |
| 基线 | GBMD `0x831020f8` 子命令 `0xff` | `0x00860004`(bit24=0 无认证充电器,bit15-16=0 Inbox) |
| 基线 | 通用 GET `0x831020c4` cmd 5/6/7/8/14/21 | `0x10/1/1/1/1/1` |
| 写入 | GBMD 子命令 `0x0d`(养护开,gen2) | `ret=0 status=0`(被接受) |
| 回读 | 上述全部只读通道 | **逐字节无变化** |
| 写入 | GBMD 子命令 `0x0f`(养护关,gen2) | `ret=0 status=0`(被接受) |
| 回读 | 上述全部只读通道 | **逐字节无变化** |

结论:表中养护写入被接受，但所列只读通道在该次测量中未显示状态变化；这不能证明其他
独立回读通道不存在。官方 UI 有本地缓存，不能仅凭 UI 回显认定固件状态。因此
`ChargeModeReader` 保持 `Unverified` 并拒绝
把"IOCTL 成功"当作语义生效;`battery charge-mode` 在 `capability` 矩阵中如实报告 `unsupported`。
历史 native-admission/target-read 原始回执保留了旧程序关于“没有任何独立读通道”的过度表述；
该原始输出不修改，以上限于已测通道的结论才是当前判断。
旧记录曾建议先放电至 55% 以下，但该门限没有在目标机得到独立验证。联想 21VG
机型族用户指南将用户可见的“节能模式”描述为 75%–80%;它与旧 Storage/GBMD 命令
是否对应尚未知。效果级验证须先确立目标模式及独立读回契约，再由用户明确安排安全的
现场观察;不以旧 55% 假设或本段作为放电、写入或恢复的许可。

> 表中养护写命令 `0x0d/0x0f` 来自 docs/01 §3.2 的已验证表；本表未展示快充写入回执。
> 未使用 `0x03/0x05`(运输模式)与任何未记录命令字。回读测量仅使用文档化通道,不扫描控制码。

## C3 · Windows 全量测试与能力矩阵

**状态:已验收(可复现)。**

| 项 | 结果 |
|---|---|
| `cargo test --offline --workspace`(Windows,MSVC) | 全量通过,进程退出码 **0**(含 40+ 测试二进制、doc-tests) |
| `vantage capability` | 输出 134 行能力矩阵 + 汇总(available 89 / unsupported 33 / limited 7 / unavailable 1 / external 4) |
| `vantage info` | `platform=windows`,`product=21VG`,`family=ThinkBook 14 G8+ IPH`,`bios=SWCN22WW` |

本轮为让测试在 Windows 主机上真实可跑而修正的**测试夹具**问题(不改生产语义):

1. Linux 后端测试(`lctrl-hal-linux/tests/linux_backend.rs`)在 Windows 上构造 sysfs/Unix 路径夹具
   必然失败 → 以 `#![cfg(unix)]` 明确限定宿主要求。
2. 本地驱动更新夹具使用 Unix 绝对路径,而校验器按宿主语义判定绝对路径 → 夹具改为宿主绝对路径,
   并按平台分别断言 `C:\…`(Windows 合法)与 `/…`(Unix 合法)的接受/拒绝。
3. 驱动更新 fake 把 `after_install/after_rollback` 无条件当作已安装状态返回,导致"安装前回读"取到
   安装后的身份 → fake 增加 `install_done/rollback_done` 门控,`reboot_required` 独立建模。
4. 热区温度断言 41.65 ℃ 与 3152 deci-Kelvin(= 42.05 ℃)不符 → 断言与文档一致,并补充第二个热区
   (3000 deci-Kelvin = 26.85 ℃)的值断言,使"实例↔温度"错配可被捕获。
5. `active_instance` 在固件类返回无实例键的附加行时直接失败 → 改为跳过不可调用的行,取第一条
   可调用实例;全部不可调用时仍 fail-closed(Windows 侧 `bios_controller` 用例回归)。

## C4 · 性能模式(ITS/Dispatcher)读回

**状态:读回已验收;写入为"缺少接口契约"(不是实现遗漏)。**

目标机 Dispatcher 注册表
`HKLM\SYSTEM\CurrentControlSet\Services\LenovoProcessManagement\Performance\PowerSlider`
的实际值名与文档常量拼写不一致(`Version` / `ITS_CurrentSetting` / `ITS_AutomaticModeSetting`
/ `PowerSlider`),且 `RegGetValueW` 对这些混合大小写值名按**大小写敏感**比较:实测
`ITS_CURRENT_SETTING` 返回 2(找不到),`ITS_CurrentSetting` 返回 3。因此读取端对每个值同时尝试
文档拼写与实际拼写。

| 项 | 值(实机) |
|---|---|
| `Version` | `8193`(0x2001 → Dispatcher **V3**) |
| `ITS_CurrentSetting` / `ITS_CurrentSettingV` | `3` → `MmcPerformance` |
| `ITS_AutomaticModeSetting` | `7`(非零 → 自动切换开启) |
| `PowerSlider` | `1` |
| `CURRENT_SETTING` / `CURRENT_STATE` / `AUTOMATIC_MODE_SETTING` | 不存在(该代只写 ITS 值名) |

验收命令与结果:

| 命令 | 结果 |
|---|---|
| `vantage perf mode --json` | `{"active":"performance","automatic":true,"capabilities":{"raw":1},"requested":"performance","version":"v3"}` |
| `vantage tune telemetry --json` | `performance` 槽位返回同一状态对象(不再报 `CURRENT_SETTING` 缺失) |

写入通道评估:WMI `LENOVO_GAMEZONE_DATA` 方法族在本机固件未实现(docs/03 §2.3 实机勘误,已复核);
`LenovoProcessManagement` 的 SCM 控制码数值随 AcpiVpc 版本变化且服务对未知码静默返回成功,
文档未给出可固化的数值 → 实测扫描控制码被排除在授权范围外,写入通道记录为**缺少接口契约**,
`perf mode <mode>` 继续如实返回 `Unsupported`。

附带实测(排除性证据):Windows 电源覆盖层 `PowerSetActiveOverlayScheme` 与
`PowerGetActualOverlayScheme` 可用(actual 值随写入变化),但 `ITS_CurrentSetting` 不随之改变,
`PowerGetEffectiveOverlayScheme` 始终保持 efficiency → 覆盖层不是 ITS 模式的替代写通道。

## C5 · 处理器能效偏好(EPP)真实写入

**状态:已验收** — 真实生效 + 服务回读 + **独立注册表回读** + dry-run 无副作用 + 精确恢复。

独立回读来源:
`HKLM\SYSTEM\CurrentControlSet\Control\Power\User\PowerSchemes\<方案>\54533251-…\36687f9e-…`
的 `ACSettingIndex`/`DCSettingIndex`(Power API 写入的持久化索引,不经我方代码)。

| 步骤 | 命令 | 服务回读 | 注册表回读 |
|---|---|---|---|
| 初始 | `vantage tune telemetry --json` | raw 127 | AC=50 DC=50 |
| dry-run | `vantage tune epp performance --dry-run` | previous 127, requested 0 | AC=50 DC=50(未变) |
| 提交 | `vantage --yes tune epp performance` | actual 0 | **AC=0 DC=0** |
| 恢复 | `vantage --yes tune epp 127` | actual 127 | **AC=50 DC=50** |

`127 ↔ 50%` 的往返证明"raw HWP 值 ↔ Windows 百分比"映射是精确可逆的;无精确反解的 raw 值
(如 128)继续被拒绝,不会静默取整。

## C6 · 处理器睿频策略(PERFBOOSTMODE)真实写入

**状态:已验收** — 真实生效 + 服务回读 + 独立注册表回读 + dry-run 无副作用 + 恢复。

独立回读:`…\54533251-…\be337238-0d82-4146-a960-4f3749d470c7` 的 `ACSettingIndex`/`DCSettingIndex`。

| 步骤 | 命令 | 服务回读 | 注册表回读 |
|---|---|---|---|
| 初始 | 读取 | previous ac=2 dc=2 | AC=(缺省→有效 2) DC=2 |
| 关闭 | `vantage --yes tune turbo off` | actual 0/0 | **AC=0 DC=0** |
| 开启 | `vantage --yes tune turbo on` | actual 1/1 | **AC=1 DC=1** |
| 恢复 | `vantage --yes tune turbo aggressive` | actual 2/2 | **AC=2 DC=2**(该取值当前不可解析,见 C34) |
| dry-run | `vantage --dry-run tune turbo off` | previous 2/2, requested 0/0 | AC=2 DC=2(未变) |

真机暴露并修复的缺陷:`PERFBOOSTMODE` 的 `PowerReadValueMin` 在本机返回不可用,旧实现因此把整条
睿频写入判定为不可用。修复为按文档枚举域(0..4)约束写入,并保留原值恢复能力。
**命令面校对(C34,2026-11-06):**上文曾记"`tune turbo` 由布尔开关扩展为文档模式集
(`off|on|aggressive|efficient|efficient-aggressive`)",该扩展在现行声明中不存在——
`TuneCommand::Turbo` 只接受 `on|off`;本表 `aggressive` 一行是历史运行记录,当前二进制
无法复现。

## C7 · 触控板失效恢复与控制器禁用缺陷修复

**状态:触控板已恢复,用户确认可用;新版控制路径已通过回归与非中断式真机 smoke。
新版禁用/恢复循环尚未做真机验收,不计为触控板功能域完整通过。**

2026-10-27,先前的控制器级禁用测试关闭了
`ACPI\GXTP5100\4&30ed0a95&0`。它的 HID 子节点随之消失,
后续 `touchpad on` 的枚举找不到 Mouse 设备而报 Unsupported,未恢复原状态。
用户随后报告触控板无法使用。故障由本轮测试留下,不是已证实的驱动或硬件损坏。

恢复仅作用于上述已确认的原控制器:

```powershell
pnputil /enable-device 'ACPI\GXTP5100\4&30ed0a95&0'
pnputil /enum-devices /instanceid 'ACPI\GXTP5100\4&30ed0a95&0' /relations
```

原生工具返回 `Device enabled successfully`,退出码 0;独立枚举由 `Disabled`
变为 `Started`,四个集合 Col01(mouse)、Col02(touchpad)、Col03(configuration)、
Col04(vendor-defined)全部重新出现且为 `Started`。用户明确确认触控板已经可以使用。
未重启、卸载/重装驱动或改动其他输入设备;用户设置 `PrecisionTouchPad\Status\Enabled=1`。

永久修复只选择有明确 HID usage 的 touchpad 与同控制器 mouse 输入叶节点,
保留 ACPI 控制器、配置集合和厂商集合。失败恢复包含返回错误但可能已经生效的尝试节点;
已启用时执行 `touchpad on` 为 no-op,不重启输入设备。

| 验证 | 实际结果 |
|---|---|
| Windows `cargo test --offline -j 1 -p lctrl-hal-win --test windows_runtime_devices` | **28 passed**,覆盖新客户端重新发现/恢复、控制器与无关集合隔离、dry-run、已启用 no-op、写入报错后生效的回滚 |
| Windows `cargo build --offline --release -j 1 -p vantage` | 退出码 **0** |
| 新程序 `vantage --dry-run touchpad off --json` | previous=enabled,requested=disabled,mode=dry_run |
| 新程序 `vantage --yes touchpad on --json` | previous=enabled,actual=enabled,mode=commit(no-op) |
| smoke 前后独立 `Get-PnpDevice` / `DEVPKEY_Device_ProblemCode` | 控制器和四个 HID 集合均为 **OK / 0**,前后完全一致 |

已构建程序:`C:\Users\ezra\vantage-replacement-target-r7\release\vantage.exe`。
SHA-256:`8888c6f771c51f86ef7216d856a66a3952eb0b64ab4de0100fe66fe58154bfa5`。
原始恢复/构建/smoke 输出保存在 `evidence/touchpad-recovery-20261027.json`。
源文件往返传输均校验 SHA-256 和长度后才使用。

**使用安全边界:**确认恢复后没有再次提交 `touchpad off`,没有对正在使用的触摸板做故障注入。
新版真实禁用效果及恢复仍需另行安排不会打断用户工作的验收窗口。

## C8 · 新增功能的开发状态(尚未验收)

本节记录 2026-10-27 的实验性工作区,不是发布或完成声明。Windows 工作区测试曾通过,
但新增的 CLI/daemon 路径并非均有行为测试或真实硬件验收;整体状态仍为**开发中**。
`evidence/native-admission-smoke-20261027.json` 仅记录 capability、进程清单和 Wi-Fi
dry-run 的实际运行,不能证明写入、事件、OSD 或自动化完成。

| 域 | 接入内容 | 当前证据 | 仍需的真机验收 |
|---|---|---|---|
| #4 能力矩阵 | `vantage capability/info/doctor` 改为按 `CommandServices` 实际服务绑定评估,输出 read/write/readback/rollback 轴、官方依赖清单与替代就绪报告 | 源码 + 单测 | 目标机重跑矩阵,确认服务绑定与实际执行路径一致 |
| #18 进程调度 | `perf top`:间隔 CPU 采样排序;`perf boost/throttle`:仅优先级(原版为 AboveNormal/BelowNormal),精确 PID + 创建时间身份守卫,写入后独立回读,`--auto-restore` 用同一快照复原。亲和性、工作集收缩、游戏白名单与前台识别尚未接入 | 源码 + 单测(CLI 契约 10 项 + Windows 事务/回读/回滚) | 目标机对自有进程实际读写优先级并恢复;不得对关键系统进程操作 |
| #38 Wi-Fi | `wifi security/history/clear-history`:改用公开 WlanApi 读取当前认证/加密,历史仅存本进程内存且默认脱敏 | 源码 + 单测 | 目标机读取当前连接的认证/加密并与系统显示核对;未连接/权限拒绝需如实报告 |
| #1 事件/watch | 有实验性 WMI 订阅与轮询路径 | 源码,未完成运行验证 | 实际变化、错误传播、断线重连、Ctrl+C/释放及 MagicBay 300 ms 重查 |
| #2 自动策略 | 有求值引擎、配置模型与尚在检查的 daemon 集成 | 引擎单测,运行闭环未验收 | 不得把 UTC 冒充本地时间、硬件前值或未执行的回退当成功;完整场景切换和恢复待验证 |
| #3 OSD | 有实验性 Windows overlay 与事件消费者;Fn OSD 持久偏好已按厂商通道实现(服务控制码 146/147、148/149 + `VantageToast` 读回) | overlay 尚无真实画面或多屏证据;Fn OSD 通道已在目标机做过控制码前后值取证(`evidence/re-osd-20261105.md`),CLI 端到端尚未验收 | overlay 的显示/非夺焦/生命周期待验;Fn OSD 需目标机跑 `osd disable`→`osd enable` 并核对独立读回与恢复 |
| #28 BIOS | `bios apply` 批量事务(复用 HAL 的 `plan_bios_apply`/`apply_bios_transaction`),破坏性项需 `--allow-destructive`,JSON 区分 accepted/saved/pending-reboot。**校对(C34):`bios apply` 与 `--allow-destructive` 当前都没有命令声明,本行是计划而非已落地源码** | 源码 + 单测 | 目标机批量写入后逐项回读并恢复;不得自动重启 |
| #35 诊断 | HAL 有七态结果及取消模型;新 CLI 的跨进程生命周期尚未可靠 | HAL 单测;CLI 生命周期未验收 | 修复扫描结束等待、跨进程状态/取消的身份与并发;真机验证六类检查 |
| #37 清单 | 有实验性只读固件报告 | 尚未有有效的变量读取证据 | 修复命名空间 GUID 和错误分类;结构布局/版本缺契约时只报告原始受限状态 |

接入过程暴露的缺陷和当前边界:

1. **能力矩阵错报**:Windows 输出把 `linux_sysfs` 列为必需依赖,且把已有
   `ProcessorPowerControl` 的 turbo 写成不支持 → 改为按平台投影依赖并按实际服务绑定评估。
2. **配置分层宏类型错误**:`overlay!` 把 `Option<T>` 直接赋给具体字段,
   合并"已显式配置"时又需要相反语义 → 拆成 `overlay!` 与 `overlay_option!` 两套宏。
3. **OSD/UEFI 运行语义尚待验证**:编译修复不等于屏幕上实际可见或固件变量可读;
   不应将上述实验性接口发布为完成结果。

## C9 · 基线 PR #40 与 main 集成

**状态:增量实现基线已合入 `main`;完整替代仍未完成。**

| 证据 | 实际结果 |
|---|---|
| Gitea PR #40 | 源头 `52805ef16c8bcdaaafe9ff2ef52b69fc4e448b6e`;原生 merge 提交 `15766658e16ad6bb98af8d1db707119f79d0b186` |
| 合并对象 | 父提交依次为原 `main` `e5edaf40eebd6b1edf8d3fd9408902f1b2801219` 和上述 PR head;合并树与 PR head 的 Git 树逐对象一致 |
| Gitea Linux CI | 精确 head 的 run **537**:fmt、clippy、workspace tests、release 构建和 artifact 上传均成功 |
| 目标机 Windows | 精确 head 的 162 个归档源文件均经 SHA-256/长度核对;MSVC 全量 workspace 测试和 release 构建均退出 **0**,运行时设备测试 **29 passed** |
| 非中断式触控板 smoke | 新程序 `--dry-run touchpad off` 报 `previous=enabled,requested=disabled`;控制器及四个 HID 集合的独立 PnP 状态前后均为 `OK / 0`,完全一致 |
| Windows 二进制 | `C:\Users\ezra\vantage-replacement-target-r7\release\vantage.exe`;SHA-256 `e9b6cca961846b0db91712633e896197ad903da40c81e884e2e011f0c0596892` |
| 回执 | 目标机原始 JSON SHA-256 `ba9223b799a44564429dbe8a013ccda6b76541d86e2e811f9ea9f590ae8f7454`;投影见 `evidence/pr40-exact-native-20261028.json`,PR 评论 292 保存核对结果 |

该验证没有再次提交 `touchpad off`,没有 BIOS/固件写入或停用任何厂商组件。
触控板叶节点完整禁用/恢复循环、其余 38 域的实际硬件效果和最小依赖切换依然按各自 issue 追踪。

## C10 · BIOS 暂存所有权安全门禁（非写入验收）

PR #42 的 head `a0e3598f8fb7f3188343f817f7aa6b530bffa3f2` 已合入 main
`5998f606edf97809ffd14ea365941b2cf346ce50`。暂存区所有权契约未验证时，Windows
stage/save/discard 被拒绝，能力矩阵报告不可用；只读查询仍可用。精确 head 的 Gitea
run 546 全绿，合并树/父提交与 11 个修改文件的服务端字节已核对。

目标机对 167 个精确源文件完成逐字节 SHA-256/长度及成员核对后，MSVC 增量 build
确认退出 0，二进制 SHA-256 为 `b6b74b2d2496d4a39453caa8b9f5ccab8ff66efe916d40c16e29e8c54b042bca`。
首次 build 日志显示完成，但包装器退出码为 null，故保留该事实并以另一次增量确认捕获真实
退出码。实际 `info`/`capability` 均退出 0，相关 BIOS 写能力显示 unavailable；
`--dry-run bios discard` 退出 0，独立 BIOS 镜像前后各 53 项且完整集合一致。
未执行 BIOS 写入或 discard；镜像一致不等于硬件物理状态验收。完整投影见
`evidence/pr42-exact-native-20261028.json`，PR 评论 305 保存原生验证边界。
合并后的 main push run 547 因 daemon 异步事件测试竞态失败，该失败未被重写；
测试同步修复在 PR #43 的精确 head run 549 通过后合入 main
`3849d93feb88a07dd4e425dc6e5914f57447b438`，其 main push run 550 全绿；
该修复未改变 daemon 生产行为。

## C11 · USB 供电的只读固件默认值

PR #42 精确 Windows 二进制在目标机只读执行 `bios get AlwaysOnUSB` 和
`bios get ChargeInBatteryMode`，均退出 0，两个保存设置镜像均为 `Enable`。
未暂存、保存、丢弃或修改 BIOS；运行时覆盖、关机供电和实际 USB 端口电气效果均未由
该观察证明。机器证据见 `evidence/target-usb-bios-defaults-20261028.json`。

## C12 · 系统清单公开来源预检（非 CLI 验收）

目标机以 PowerShell 对 16 个属性限定的公开 CIM 查询执行只读预检，全部成功；
其中驱动、固件驱动、软件查询分别返回 276、10、185 条。SMBIOS UUID 格式与非全零、
非全 FF 检查通过，但原值未保留。该结果不证明未合入的 Rust 15 模块清单实现或超时
行为；来源、耗时、隐私边界见 `evidence/target-inventory-sources-20261028.json`。

## C13 · 触摸板状态与恢复 dry-run（非实写验收）

PR #44 head `69bd462dc938600f12681cc25d46a148a4bd9e49` 已合入 main
`aaa6825fcfdb7aca14eb21dc4ecd96caea5cf546`；精确 head CI run 551 全绿，合并树、
双亲和 12 个修改文件的服务端字节已核对，main push run 553 全绿。目标机验证 167 个源文件后 Windows release
构建退出 0，binary SHA-256 `3074c7de8e058d70fcbbd8a4cbf95f0841a4b37dc3ba5a22b49bdee7ce383c80`；
Windows fake PnP 测试 33 passed，未操作真实设备。

目标机 `touchpad status` 返回 enabled，`--dry-run touchpad recover` 返回 digitizer
和 mouse 前值均 enabled、请求均 enabled，均未提供 actual。独立查询的控制器和 4 个
HID 节点前后状态/问题码完全一致；未提交 touchpad off/on/recover。真实混合状态恢复
和有独立备用输入条件下的完整 off/on 循环仍待验收。见
`evidence/pr44-touchpad-dryrun-20261028.json` 和 PR 评论 324。

**2026-11-05 契约变更（本条未执行验收）:** `touchpad recover` 现在按观察到的叶状态选择
策略，报告新增 `strategy` 字段，并在全部叶已启用时对输入叶执行 disable→enable 重启；
因此上表 dry-run 的 JSON 形状多了 `strategy`(当时二进制无此字段)，其余结论不变。新契约、
边界与待验收项见 `evidence/re-touchpad-20261105.md`，目标机写入仍未执行。

## C14 · 电池来源分离的只读验证（非充电控制验收）

最终 Rust 构建输入共 138 个文件，Git/文件系统独立成员枚举一致，传输至目标机后逐文件
核对长度和 SHA-256；构建后再次核对成员、源字节及单独的离线依赖配置。源 manifest
SHA-256 为 `912928d66e9f430a2806b9bc38141f3dea42a4430f0b3fe23e10a3af952c7895`。
Windows MSVC release 构建退出 0；电池类解码 12 项和来源/WMI/温度/能力契约 41 项
均通过，均为模拟或解码测试，不代表硬件写入。Linux 受影响包测试、Linux lint 和
Windows 交叉 lint 均在工作树私有 target 中通过。

实际执行的 `battery status --json`、`battery adapter --json`、`perf temp --json`
均退出 0，无超时。观察时系统处于电池供电/放电状态；没有主动切换 AC 或充电模式。

| 观察 | 新程序结果 | 独立只读对照 |
|---|---|---|
| 设计/标称电压 | `device_telemetry.design_voltage_mv=15600`;该来源实时电压、电流、温度均 null | 这是既有已记录的 EnergyDrv 标称字段，不当作瞬时端电压 |
| 实时电压 | `electrical.voltage_mv=16053`、`os_battery.voltage_mv=16053` | 命令前后 `root\WMI BatteryStatus.Voltage` 均 16053 mV |
| 放电功率 | `electrical.rate_mw=-19151`，原始 `power_state=2` | WMI `Discharging=true`、`Charging=false`、原始 `DischargeRate=19151`;容量能力位为绝对单位 |
| 容量 | OS 每包电量 69%、满充/设计容量比 94%，不合成健康等级 | 同一非空 InstanceName 关联的容量为剩余 65240、满充 94090、设计 99900;不与厂商索引绑定 |
| 温度 | ACPI 热区约 37.05 ℃、位置 unknown;电池类温度 null/unavailable | 独立 ACPI 原始值 3102 deci-K;未把未验证的 SMBus 字段当电池温度 |
| 适配器 | 厂商详情/额定功率未知;聚合 `system_power` 单列、AC=false | WMI `PowerOnline=false`;未用它补全厂商适配器字段 |

二进制为 `C:\Users\ezra\vantage-battery-target-912928d66e9f430a\release\vantage.exe`，
SHA-256 `84883d50f423be2d8c933f7fc810117eedab2d9fe7a0bc503095af8d47bdcf65`。
来源间的电池身份仍为 `unverified`；充电模式回读仍明确返回 unsupported。原构建包装器
曾在完成日志后等待不返回，原回执保留为退出码未确认；改为直接等待 Cargo 进程后取得
真实退出码。首份只读回执的 PowerShell 数组投影失败也被保留，未更改源程序。

未执行硬件写入、触摸板操作或 BIOS 操作；本记录不验收充电控制、USB 物理供电、全部
电池指标或全面替代。完整脱敏投影及各原始回执哈希见
`evidence/target-battery-source-separation-20261028.json`。

## C15 · 充电差分实验纠错（非充电控制验收）

2026-10-28 02:44 UTC 的实验对目标机依次发送 `0x08`、`0x07`、`0x0d`、`0x0f`，
finally 又发送 `0x07`。未发送 `0x03`。原脚本把四字节状态按大端计算、未检查保存写
返回状态，且计算出的 baseline 未用于 finally 恢复决策;原输出中的 `gbmd_word` 和
所有 bit 布尔值均不可用。另有一行标为 `after_rapid_off_0x08` 的快照实际发生在写入前。

原始字节在 `0x08` 前为 `04 00 86 00`、其后为 `00 00 86 00`、`0x07` 后为
`04 00 86 00`，最终打印值也为 `04 00 86 00`。按小端分别为 `0x00860004`、
`0x00860000`、`0x00860004`;其相关性不足以证明快充电流、其他充电策略或物理恢复。
实验时 WMI 报告电池放电、AC 未接入。2026-10-28 12:40 UTC 的后续只读 WMI 仍报告
放电及 AC 未接入，但不能据此推断当前充电模式。

原始调用和输出已私下保留，哈希及纠正投影见
`evidence/target-charge-probe-correction-20261028.json`，Issue #6 评论 357 记录了事故边界。
为修正记录没有再次写入。`UnverifiedChargeModeReader` 仍阻止目标机充电模式写入;
后续必须独立验证状态契约、实际效果和恢复，不能把本次结果当作验收通过。

控制器恢复逻辑另经 13 项离线测试及模拟传输 consumer smoke 验证：首个写请求在
模拟设备生效后报错时尝试恢复前值;恢复也失败时保留两个错误且不报告恢复成功。
Linux 与 Windows 交叉 lint 均通过。这些是模拟结果，不是目标机实际恢复验收。

## C16 · 电源方案身份隔离（模拟验证，非真机并发验收）

旧设置事务多次重新读取活动方案：若前值读取与写入间发生切换，可能把前一方案的值
写入或恢复到另一方案。修复后，每次单值事务及其 AC/DC 组合绑定同一个方案 GUID;
观察到活动方案改变时不把写入转移给新方案，必要恢复只针对原 GUID。激活失败时，
若当前方案既非前值也非目标，不自动覆盖该第三方案。

离线 fake Power API 覆盖了读取后切换、写入后切换、AC 与 DC 之间切换、激活失败后
第三方切换，以及原有成功/dry-run/恢复等场景;真实控制器的模拟 consumer smoke 覆盖
前两种切换。Windows 交叉 lint 和 Linux 测试通过。目标机逐文件核对 138 个 Rust 构建
输入后，Windows release 构建退出 0，8 项电源及 11 项调优 fake 测试通过;二进制
SHA-256 `cd8a2e0780a6de9268e69ebb5c262fae96c7af1b5e937c90d0cf76642f463508`，
源 manifest `71cf4741bc0e0099e0eb9a938c4ecb615703d3eba39c11b71a375f72748577a7`。

实际程序对 DC EPP 50 执行 `--dry-run power scheme set`，退出 0、报告 `dry_run`
且无 actual。独立 `powercfg`/注册表前后均为平衡方案 GUID，AC/DC 指数均为 50。
请求值恰等于前值，因此该前后快照本身不能排除同值写入; fake dry-run 测试另验证未
调用写接口。未在目标机执行方案切换或电源设置 commit。
活动 GUID 检查与 Windows `PowerSetActiveScheme` 本身仍非跨进程原子操作；对同一方案
的并发改值及进程崩溃恢复也未证明安全。完整回执哈希见
`evidence/target-power-scheme-isolation-20261028.json`。#9 issue 已关闭，但后续评论
记录的多方案真机验收与并发、崩溃边界仍未完成。

后续发现恢复写入仍可能覆盖已观察到的第三方设置值。新增恢复前读取：仅在当前值是
本次请求值时尝试写回前值，当前已是前值则不写，其他值保留并报告未恢复。单值与
AC/DC 组合均用模拟并发回归覆盖;读取后至恢复写入前仍存在非原子竞争窗口，不能据此
宣称同方案并发已完全安全。此后续修复未执行目标机写入或新 head 原生验证。
模拟 consumer 对前值 50、请求值 70 验证：观察到 70 时恢复为 50;观察到第三值 80 时
保留 80 并报告失败。另有 fake 回归验证前值未变时不会追加恢复写入。

方案名称选择另用模拟 CLI 验证：重复的大小写无关名称被拒绝且不触发激活;
当一个方案名称与另一个方案的精确 ID 相同时，选择后者的 ID。该测试没有操作真机方案。

## C17 · 内置屏亮度与受限真机写入

目标机公开 WMI 只读预检观察到唯一活动内置屏，亮度/方法/连接实例按非空身份精确匹配;
显示器身份未保留。140 个 Rust 输入在目标机构建前后经成员及 SHA-256 核对，Windows
release build 退出 0，11 项模拟后端与 3 项模拟 CLI 原生测试通过。二进制 SHA-256
`3c44a64c62b66d171b646d9534a4607f9b7041bdf5a6f0634a234fa9b8da8304`，
源 manifest `3ed6f8c3514d474dc71a32a09ce39fc7d55776980127368a2938a94caa08f92b`。

真实程序读取亮度为 55%，对不同请求值 54% 执行 `--dry-run`，均退出 0;
独立 WMI 前后仍为同一内置屏、55%，dry-run 不报告 actual。该观察证明限定时间内
状态未变，不证明物理亮度或后续写入。该次观察未绕过冲突门禁或执行实际亮度写入。
WMI 单次调用、外部控制者竞争和进程崩溃恢复仍有边界。完整
脱敏回执及哈希见 `evidence/target-brightness-readonly-20261028.json`; #19 继续开放。

随后用户明确允许在厂商控制进程继续运行时进行 1% 测试。增加 `--expect-current` 后的新
二进制 SHA-256 `4d6a24d8fdb9f1df8bb3cff0e434753ad98c9746ed133ac0f840b88ce58cb247`
由 140 个源文件构建，12 项模拟后端和 4 项模拟 CLI 原生测试通过。55→54 的 dry-run
通过，独立 WMI 不变;实际写入返回 exit 6，错误表明初次 setter 和程序内部恢复 setter
均失败。外层没有重试，独立 WMI 在命令后仍为同一内置屏 55%。不能据此证明期间没有
短暂变化、实际物理亮度或故障恢复已验收。

只读目标 WMI 类元数据报告 `WmiSetBrightness` 返回类型为 Boolean，而 Microsoft 网页
描述 UInt32/0 成功;精确失败阶段及布尔语义仍待确认。完整脱敏失败回执见
`evidence/target-brightness-live-failure-20261029.json`。不重复失败写入以确认故障。
后续只读 `System.Management` 签名检查发现该方法输入对象存在、输出参数对象不存在;
[推断] 这可能解释原封装的无输出错误，但原错误已脱敏，不能证明它就是唯一原因。

修正无输出对象的处理后，以源 manifest
`c9da34756964d8db9606a1cb380589bcd3a9a6ae5cb33277b5c641ffacd56048` 的
140 个文件重新原生构建;二进制 SHA-256
`a867b8548f1acf9c8497477d0d2f2d30f752c230d6bcd15217520029d004601f`。
13 项模拟亮度、14 项 WMI 契约和 4 项模拟 CLI 测试通过。用户授权的后续测试中，
55→54 dry-run 没有改变独立 WMI 值;实际设置返回 54，同一内置屏的独立 WMI 也为 54;
条件恢复返回 55，独立 WMI 再次读到同一屏的 55。厂商进程保持运行。此次证明限定
WMI 状态的实际变化和正常恢复，不单独证明物理亮度、故障路径或进程崩溃恢复。
脱敏回执及原始回执哈希见 `evidence/target-brightness-live-success-20261029.json`。

随后用独立诊断 consumer 链接同一源码快照的生产亮度后端：真实 WMI setter 返回后，
独立 WMI 在同一屏观察到 54%，consumer 注入一次合成 I/O 错误;后端的内部恢复 setter
完成，独立 WMI 再次读到 55%。外层没有追加恢复写入。诊断 consumer 的 4 项模拟及
观察器的 4 个无硬件场景验证了恢复、保留第三值和观察器失联边界。该错误为人为注入，
不代表 WMI 提供者实际故障;测试也不证明物理亮度、真实外部竞争、迟到效果或进程崩溃
恢复。脱敏回执见 `evidence/target-brightness-fault-recovery-20261029.json`。
#19 和完整 `power saver-once` 验收仍未完成。

## C18 · Windows 电源设置的受限只读对照

140 个构建输入在目标机原生构建前后逐项核对;源 manifest SHA-256
`d6c7deec5ee440f60035781cde595ae6922d16e182e554237d1d02023a49ea99`，release
二进制 SHA-256 `7158e4662d09398555c0f850aadc727640864e75e4c13fe2d258063bf33e5b77`。
24 项原生库、14 项模拟电源后端、12 项模拟调优和 5 项模拟 CLI 测试通过。

真实程序对同一活动方案执行五次 `power scheme get NAME --subgroup GUID --setting GUID`;
各次退出 0。下列是读取到的原始 Windows 设置索引，不是物理功耗或频率：

| 设置 | AC 索引 | DC 索引 | 数值范围及步进 |
|---|---:|---:|---|
| EPP | 50 | 50 | 0–100，步进 1 |
| 睿频策略 | 2 | 2 | 不可用;保留两个实际索引 |
| 处理器上限 | 100 | 100 | 0–100，步进 1 |
| 磁盘空闲 | 30 | 30 | 0–4294967295，步进 1 |
| 显示空闲 | 0 | 180 | 0–4294967295，步进 1 |

每次的前后方案和数值稳定，与独立 PowerShell 进程调用同一 Windows Power API 的结果
一致，且本机注册表中存在的 AC/DC 索引均一致。五项设置的两种来源都报告无组策略
覆盖;这不是一般写权限保证。目标睿频的三个范围读取均返回状态 2，程序正确报告范围
不可用而不是猜默认值。未执行电源设置写入或切换方案。

目标机只观察到活动方案;非活动方案绑定及切换由模拟覆盖。独立进程仍使用同一 OS API，
读取不是原子快照，也不证明 CPU 实际频率、磁盘停转或显示器实际行为。外部并发写入、
迟到效果与崩溃恢复未验收。完整脱敏回执见
`evidence/target-power-setting-read-20261029.json`;#39 的电源域仍未全部完成。

## C19 · 只读遥测与温度流的有限运行验收

原先 Linux 实际命令 `--json tune telemetry --interval 1` 在约 0.071 秒后只输出一行
便退出 0,没有实现请求的持续采样。修复后,立即采样并在每次完成后等待指定正整数秒数;
`perf temp watch` 也采用串行采样,默认间隔 1 秒。每行 JSON 独立完整并刷新输出,
不启动 daemon、不积累历史。单结果库 API 拒绝流式请求,不能静默降为一次读取。

140 个 Rust 构建输入在 Windows 原生构建前后逐项核对;源 manifest SHA-256
`d7630bd6164e3d72abc2fb209fc54a84b20465f0b5bc2488510c191325774cdf`,二进制 SHA-256
`aa4eecd3eaf70dbf360895b7cf286d2869c66a52db38cabbf0b1c44edef0b9e7`。
原生 release build 及 13 项 capability、43 项 CLI 模拟契约测试均退出 0。
构建和 Linux 实际流、零间隔拒绝、断开消费者的证据单列于
`evidence/telemetry-stream-build-linux-20261029.json`,不以该文件代替 Windows 验收。

Windows 实际观察时间为 2026-10-29 UTC。两个命令均使用 `--json`,在独立拥有的
SSH 控制台中运行,捕获原始 stdout 后再解析样本:

| 命令 | 完整样本数 | 前两行接收时间差(ms) | 原生中断退出码 | 强制终止 |
|---|---:|---:|---|---|
| `tune telemetry --interval 1` | 2 | 1258.635 | `C000013A` | 否 |
| `perf temp watch --interval 1` | 2 | 997.710 | `C000013A` | 否 |

父子进程同属对应控制台。观察器在子进程启动后才忽略自己的 Ctrl+C,随后通过
`GenerateConsoleCtrlEvent(CTRL_C_EVENT, 0)` 生成事件;不是物理键盘输入测试。
两次原生子进程均退出且 stderr 为空。外层 SSH 退出状态没有被当作子进程退出码。
这些接收时间差含提供者读取与观察器调度,不代表精确周期或单次 WMI 调用截止时间。

各次前后由独立 `Get-CimInstance` 查询 ACPI 热区,私下核对身份集合;对应单个热区的
原始十分之一开尔文值在该次前后稳定,每个 CLI 样本均符合摄氏转换及 f32 舍入。
该热区继续标为 `location: unknown`,没有冒充 CPU/GPU。另一个 Windows 电池类温度
来源为 unavailable/null,没有填零。原始设备身份不随脱敏回执发布。

14 份目标机原始回执经长度与 SHA-256 完整复制核对,摘要见
`evidence/target-telemetry-stream-20261029.json`。早先数次 SSH 分派没有产生目标 intent
或样本回执,未计为产品执行或通过;保留失败证据后,持有 SSH 输入管道的有界观察器
才完成本次验收。所有实际采样均遵守共享 heavy 锁,没有请求硬件写入或停用官方服务。

此次仅证明有限只读采样、该 ACPI 来源及所测控制台中断路径。长期内存稳定性、其他
终端宿主、物理传感器位置、CPU/GPU 包功耗与频率、风扇/模式控制、崩溃恢复及完整
替代仍未完成;#12 和 #39 保持开放。

> **2026-11-05 追补(不影响上述运行记录)**:`perf temp` 的传感器集合此后增加了
> Intel ESIF/DTT 参与者来源(`root\WMI EsifDeviceInformation`,来源 `wmi_esif`,
> 契约与逐条原始值见 `evidence/re-thermal-20261105.md`)。上面的 2026-10-29 运行
> 早于该来源,只覆盖一个 ACPI 热区与电池类温度,不能作为 ESIF 行的验收证据;
> 新增行的验收(实例身份、0 值按无读数处理、`perf.temp` 能力行在只有 ESIF 可读
> 时的可用性)仍需在目标机重跑并单独记录。

## C20 · 自动场景策略、热键与 OSD 的接管验收

**状态:场景策略与恢复路径已验收;OSD 与热键的真实画面/按键效果在交互式会话 1 才可见,
SSH 服务会话 0 如实报告 `gated`,不以无显示路径误判硬件不支持。**

场景策略(commit f38f1bc)已验收:`scene list/test/track` 在目标机实际运行,透明规则对
联想 WMI 事件求值,只允许白名单内的场景动作(电源方案、面板刷新、风扇模式、性能模式),
写后回读,失败按原值恢复。风扇/性能动作在缺少已验证写通道时如实报 `unavailable`,
不以未知控制码或 BIOS 试写绕过。

OSD 与全局热键(commit 65c01a9)是场景策略生效时的可见/可触达通知与触发面:

| 命令 | SSH 服务会话 0 的实际结果 | 语义 |
|---|---|---|
| `osd test` / `osd show <行…>` | `state=gated`,`detail` 指明 `CreateWindowExW failed; this service is not attached to the active desktop session` | overlay 需要活动交互桌面,服务会话无显示拓扑,如实门控 |
| `osd enable` / `osd disable` | 不再绘制 overlay;改为驱动联想 Fn OSD 偏好(`LenovoFnAndFunctionKeys` 用户自定义控制码 146/147、148/149)并读回 `VantageToast\ShowCapslkOSD` / `ShowNumlkOSD` | 持久偏好 + 实际回读;`NumLk` 在 `Capbility\SupportNumlkKey=0` 的机型如实 `not_supported` 且不发控制码;`--dry-run` 只报告当前值与将发送的码 |
| `hotkey register <id> <vk>` | `state=gated`,`detail` 指明 `RegisterHotKey failed; no interactive desktop session / hotkey already owned` | 全局热键注册需要交互桌面,服务会话如实门控 |
| `hotkey list` | `{"registered":[]}` | 注册被门控时列表为空,不伪报已注册 |
| `hotkey status` | `service_running=true`、`utility_running=true`、`fn_lock.state=on`(`HotkeyMode=Disable` + `F1-F12AsPrimaryFunction=Enable`)、`fn_ctrl_swap.state=on`(`FoolProofFnCtrl=Enable`)、`tables` 为 37 个当前用户表 + 3 个机器表 | 只读报告联想 Fn 热键包状态:服务/渲染进程、`Capbility` 标志、固件 FnLock 与 Fn/Ctrl 互换、持久化的按机型映射表;不写注册表、不发服务控制码、不改键位(`docs/evidence/re-hotkeys-20261105.md`) |
| `hotkey trigger <id>` | `{"id":<id>,"live":false}` | 未注册时触发如实报非存活 |
| `hotkey unregister <id>` | `{"id":<id>,"removed":false}` | 未注册时释放如实报未移除 |

overlay 用 GDI 顶层分层窗口自绘(WM_PAINT 重绘),不做无界隐藏定时器;热键用
RegisterHotKey + 消息泵线程,WM_HOTKEY 记录内部 id。两者在交互式会话 1 才真正
绘制/按键,服务会话一律 `gated`,与 C1 显示验收同属"交互式会话"边界。能力矩阵
新增 `osd.show`、`osd.fn.capslock`、`osd.fn.numlock`、`channel.fn-osd` 与
`hotkey.list/status/register/trigger/unregister` 行(`hotkey.status` 为只读复合状态,
`StateLayer=Composite`),`DispatchSupport=Implemented`,
读回如实 `unverified`(硬件效果仅在交互桌面可验证)。daemon 在每次场景策略生效时调用
overlay 标注场景 id、动作与 applied/unavailable 状态。

本轮未请求交互桌面 1 的物理画面或按键,未停用任何官方服务;OSD 实际绘制与热键真实
按键效果仍按 C1 的交互式会话边界另行安排。#3、#4 保持开放。

## C21 · 最小官方依赖与安全切换验收

**状态:历史依赖投影,不是卸载/停服务后的安全切换验收。**
本节 2026-10-30 的矩阵含后来被推翻的缺失判断,不能作为当前能力或卸载依据。
2026-11-02 已实际打开 EnergyDrv 并完成充电/USB 写入验收(C28/C29);
服务清单没有 EnergyDrv 条目不等于设备不存在。客户端不加载厂商 DLL 也不能证明
卸载 Vantage 后服务端仍在。未执行完整必要功能的卸载前后对照,撤销原“安全切换已验收”结论。

2026-10-30 对目标机 `vantage capability --json` 全量矩阵做依赖投影,并只读清点官方
联想服务/驱动:

| 官方联想依赖 | 使用(必需/可选) | 覆盖命令 | 依据 |
|---|---|---|---|
| `lenovo_vantage_service` | **0 必需 / 0 可选** | 无 | CLI 全部经公开 Win32/WMI 直连,不经官方 Vantage RPC → 安全切换的核心证据 |
| `lenovo_firmware_wmi` | 30 必需 | 全部 BIOS 命令 | BIOS 管理只能走固件 WMI 命名空间,直连而非经 Vantage 服务 |
| `smart_sense` | 6 必需 | 全部 sense.* | SmartSense 功能由该组件承载 |
| `energy_driver` | 4 必需(当时投影) | charge-mode、usb.always-on、usb.charge-on-battery、power.saver-once | 原“本机无驱动”判断错误;设备后来实际打开,充电/USB 写入见 C28/C29 |
| `lenovo_process_management` | 1 必需 | perf.mode | 性能模式经 Dispatcher 注册表,非 Vantage 服务 |
| `dolby_audio_sdk` / `noise_cancellation_sdk` | 各 1 必需 | audio.dolby / audio.noise-cancel | 音频控制受 SDK 许可/接口边界 |
| `lenovo_fn_and_function_keys` / `lenovo_utility_service` / `intel_dptf` | 可选 | 键盘回退/调优回退 | 仅作回退,主路径用公开 API |

其余全部命令(电源方案、内屏刷新、亮度、面板、隐私、触控板、键盘、温度遥测、诊断、
清单、Wi-Fi、更新、本地驱动、MachineFix、场景策略、OSD、热键等)仅依赖公开
`win32_*` / `windows_*` / `public_win32_cim` / `local_daemon` / `local_filesystem`,
不触碰官方联想二进制。

目标机官方组件实际状态(只读,未停用/卸载):LenovoFnAndFunctionKeys、
LenovoPcManagerService、LenovoProcessManagement、LenovoVantageService、SmartConnect、
SmartSense 均 Running/Automatic;驱动 lenovoDriverBus Running、SmartSAMD Stopped;
当时据清单得出的“无 EnergyDrv”结论已撤销,不能据此推断充电/USB 硬件不适用。
性能/风扇/PL 的各自能力边界须分别核对,不能由驱动清单整体推断。

安全切换结论尚未建立。上述依赖投影只说明当时客户端声明的调用路径,不证明所有功能
已完成、所有服务端均独立于 Vantage 交付,或卸载不会破坏功能。在全部必要功能与
最小依赖切换验证完成之前,不停用或卸载任何官方驱动/服务。

## C22 · 可获取的 Windows 程序交付

**状态:Windows 程序已发布并可获取。**

release 二进制 `vantage.exe`(SHA-256 `8c299fd2d46c3a84c970a9c27ee06e8485223f812942b74eb4d24d8efad51482`,4627968 字节)随仓库发布于 `dist/windows/vantage.exe`,同目录 `vantage.exe.sha256` 提供核对值。Gitea 与 GitHub main 均已同步该二进制;GitHub release `v0.1.0-windows`(tag `v0.1.0-windows`)作为标签记录交付。

目标机 Lenovo 21VG 上该二进制的实际效果与回读按 C1–C21 逐项验收;缺已验证接口契约或硬件不适用处(充电/USB 写入、性能/风扇/PL 写入、BIOS 更新写入)如实报告 `unavailable`,不以未知控制码、厂商二进制或停服务绕过。完整功能追踪见 tracker #39 的 38 个功能域。

## C23 · 性能模式改由 Windows 原生 overlay 实现（真机验收）

**状态:已消除 `perf.mode.set` 的 `Unsupported`;经 OS 原生 API 实现并真机验收,不经联想厂商层。**

实现:新建 `crates/lctrl-hal-win/src/power_overlay.rs`,运行时经 `GetProcAddress` 解析
`powrprof.dll` 的 `PowerGetActualOverlayScheme` / `PowerGetEffectiveOverlayScheme` /
`PowerSetActiveOverlayScheme`。三个方案映射:

| 模式 | overlay GUID |
|---|---|
| balanced (`perf mode auto`) | `00000000-0000-0000-0000-000000000000` |
| quiet (`perf mode cool`) | `961cc777-2547-4f9d-8174-7d86181b8a7a` |
| performance (`perf mode performance`) | `ded574b5-45a0-4f42-8737-46345c09c238` |
| geek / silent-high-performance / custom | 拒绝（无 OS 等价语义,不近似替代） |

写路径:读 `PowerGetActualOverlayScheme` 作前值 → dry-run 不写 → 提交后读回校验 →
不一致回滚到前值并报 `VerifyMismatch`;前值非本实现拥有的方案时在写之前拒绝。

真机结果（2026-10-30,21VG,SSH 管理员身份,`--yes` 通过并发控制门禁）:

| 步骤 | 程序输出 | 独立 `PowerGetActualOverlayScheme` |
|---|---|---|
| 初始 | — | `00000000-…`（balanced） |
| `--dry-run perf mode performance` | `requested=performance actual=dry-run (previous=balanced)`,exit 0 | 不变 `00000000-…` |
| `perf mode performance` | `requested=performance actual=performance (previous=balanced)`,exit 0 | **`ded574b5-45a0-4f42-8737-46345c09c238`** |
| `perf mode auto` | `requested=balanced actual=balanced (previous=performance)`,exit 0 | 回到 `00000000-…` |
| `perf mode geek` | `feature is unsupported: perf.mode.geek.overlay`,exit 3 | 不变 |

Windows 目标机构建:`cargo build --locked --release -p vantage` 退出 0;
`cargo test -p lctrl-hal-win --test windows_performance_p0` 8 项全过;
`--test overlay_native` 2 项全过（真实 `powrprof.dll` 路径 + HAL 能力记录为 `Limited`）。

能力矩阵:`perf.mode` 依赖改为 `win32_power_api:required`、`win32_registry`/`lenovo_process_management`
降为可选,回读改为 `required`;`status` 为 `unverified`（服务角色在根 HAL 中不可见,与
`tune.epp`/`power.scheme.set` 同口径）,不再为 `unsupported`。

边界:overlay 只覆盖 OS 定义的三个方案;厂商自有的 geek/自定义模式不实现。
PL1/PL2/tau 与风扇模式在 Windows 仍为不可用——唯一在册 MSR 通道是联想厂商驱动,
按"不经厂商厂商层"的约束不采用（详见 `docs/interfaces.md` §9.3）;Linux 侧同类能力由主线内核提供。

## C24 · v1.0.0 首版发布（Windows 二进制交付）

**状态：已交付。README 面向用户重写；CLI 版本 1.0.0；dist 二进制随标签可获取。**

- 版本：`vantage` / `vantage-cli` 升至 1.0.0，`vantage --version` 实测输出 `vantage 1.0.0`。
- README.md 重写为 CLI 产品文风：安装、快速上手、按任务分类的命令表、本机已验证清单、
  已知限制及原因（逐条对应真实缺口，非隐藏 Unsupported）、安全模型。
- 构建：目标机 `cargo build --locked --release -p vantage --offline` 退出 0（37s）。
- 发布物：`dist/windows/vantage.exe`（4,487,680 字节），SHA-256
  `a24592060e174583043ef9739a92e8e17425e6f0362584b81c5f43b64dde2b71`，
  同目录 `.sha256` 文件。
- 发布后冒烟（v1.0.0 二进制）：`capability` 含 `perf.mode` 行；
  `--dry-run --yes perf mode performance` exit 0 且前值如实报告（quiet）；
  `--yes perf mode geek` exit 3 拒绝。
- 标签 `v1.0.0` 打于 main，Gitea 与 GitHub 同步；GitHub Release 页面随发布说明创建。

边界声明：C1–C23 只记录其各自限定的验收范围,并不证明已覆盖 21VG 全部适用功能。
充电、风扇、PL、面板等写入仍须逐项核对门禁与独立物理验收,不得据此停用官方服务。

## C28 · 写入通道真机验收(2026-11-02,dry-run → 真实写 → 独立回读 → 恢复)

方法:每项先 `--dry-run` 规划,再 `--yes`(Vantage 自身进程在运行,须接受并发控制风险)执行真实写,
随后用**与 CLI 无关的读回通道**核对,最后恢复原值并再次核对。独立读回通道:EnergyDrv/GBMD 与 USB 标志字
(`charge_domain_probe` 独立读取)、HPD 注册表(`HKLM\SYSTEM\CurrentControlSet\Services\SmartSense\Parameters\HPD`)、
厂商客户端只读探针(DolbyRpcClient.dll / ElevocControlSDK_x64.dll 的只读导出,供对照)。脚本与原始记录:
`docs/evidence/target-write-acceptance-20261102.ps1`、
`docs/evidence/target-write-acceptance-20261102.txt`、`docs/evidence/target-write-acceptance-round3-20261102.txt`。

| 功能 | 写 | 独立回读 | 恢复 | 结论 |
|---|---|---|---|---|
| 充电模式 `battery charge-mode` | normal → exit 0 | GBMD `0x00860004`→`0x00860000`(Normal) | rapid → exit 0,GBMD 回 `0x00860004` | **已验收** |
| 夜间充电 `battery night-charge` | on → exit 0 | night-charge 状态 `0x01`→`0x11`(enabled=true) | off → exit 0,回 `0x01` | **已验收** |
| USB 供电 `usb always-on` | off → exit 0 | USB 标志字 `0xCEC0`→`0xCE40`(ac off) | on → exit 0,回 `0xCEC0` | **已验收** |
| USB 电池模式 `usb charge-on-battery` | off → exit 0 | USB 标志字 `0xCEC0`→`0x4EC0`(battery-mode off) | on → exit 0,回 `0xCEC0` | **已验收** |
| 杜比 `audio dolby` | movie → exit 0 | 厂商客户端:`GetDolbyEnabled=True`、`GetActiveProfile=0` | dynamic+off → exit 0,`GetDolbyEnabled=False`、`GetActiveProfile=5` | **已验收**(使用者当场听到声音变化) |
| 降噪 `audio noise-cancel` | shared → exit 0 | 厂商 SDK:`GetCurrentMode=1` | off → exit 0,`GetCurrentMode=0` | **已验收** |

修复(此前这些写入静默失败,现已在真机生效):USB 写选择子需要 4 字节输出缓冲(0 长度被驱动以
ERROR_INSUFFICIENT_BUFFER 拒绝);GBMD 写回空的 4 字节槽,须由读回轮询判定;杜比禁用时设备保留预设,
回读只比较 enable 位;降噪端点的能力清单不枚举模式名,误判为"不支持",改为由服务端返回码判定。

未通过(如实记录,不是"已实现"):SmartSense 三项开关(lock-on-leave / wake-on-approach / pause-video)在管道层
被拒(`0x00090101 arg 0/1`、`0x00070101 arg 0`、`0x000d0101 arg 1/2`),HPD 三个值均未改变,写入通道尚不可用;
风扇 `perf fan auto|manual` 在写入前的前置回读检查处被拒(本机型 `GetSmartFanMode` 一律 `WBEM_E_INVALID_OBJECT`,
`SetSmartFanMode` 的 success 不校验取值也无法回读,见 `docs/evidence/re-fan-write-20261105.md`);
`perf pl1|pl2` 报 `unsupported: tune.pl1|pl2`(按依赖裁定不接联想厂商 MSR 驱动)。三者的契约缺口见
docs/interfaces.md §13/§15,属待开发项,不计入已验收。

## C29 · 自动恢复(`--auto-restore`)真机验收(2026-11-02)

新增全局开关 `--auto-restore`:写入在回读确认之后,把写入前的状态写回去,并对恢复
结果再回读一次;恢复无法被证明时命令报错,而不是假装成功。干跑(`--dry-run`)不产生
任何写入。单测:`crates/vantage-cli/tests/cli_contract.rs`
(`auto_restore_puts_the_previous_dolby_state_back`、
`auto_restore_reports_a_state_that_cannot_be_proven`、`auto_restore_stays_dry_in_a_dry_run`、
`auto_restore_puts_the_previous_usb_charging_state_back`)。

目标机实测(脚本 `C:\Users\ezra\acc5.ps1`,记录
`docs/evidence/target-auto-restore-acceptance-20261102.txt`):

| 命令 | 写入 | 自动恢复 | 独立回读(厂商只读探针 / EnergyDrv 探针) |
|---|---|---|---|
| `audio dolby movie` | enabled=true profile=Movie | `restored dolby: enabled=false profile=Dynamic` | `GetDolbyEnabled=False`、`GetActiveProfile=5`(基线) |
| `audio noise-cancel shared` | mic mode=Shared | `restored noise-cancel: mic mode Off` | `GetCurrentMode=0`(基线) |
| `battery charge-mode normal` | actual=normal | `restored charge mode: rapid` | GBMD `0x00860004` → Rapid(基线) |
| `battery night-charge on` | true | `restored night charge: enabled=false` | night-charge status `0x01`(基线) |
| `usb always-on off` | ac off | `restored USB AC charging: enabled=true` | USB 标志字 `0xCEC0`(基线) |
| `usb charge-on-battery off` | battery-mode off | `restored USB battery charging: enabled=true` | USB 标志字 `0xCEC0`(基线) |

每条命令退出码 0,机器最终停在基线。

## C25 · WinMsrDev MSR 通道真机只读验收（写入保持门禁，部分）

**状态：只读路径已验收；写入路径保持门禁，等待独立物理确认。**

- 通道：自有 Rust 客户端 `WinMsrDev`（`crates/lctrl-hal-win/src/msr.rs`）经
  联想签名 `LnvMSRIO` 驱动设备 `\\.\WinMsrDev`，IOCTL 契约按 `interfaces.md` §4。
- 调用身份：`LocalSystem`（经一次性计划任务）；普通用户直开设备如实报
  `permission denied; requires administrator`。
- 握手：真实 `NotifyPlatformInfo(0x36, 0)` 返回成功——**未经驱动白名单修补**，
  与 `interfaces.md` §3.2（当时需修补白名单）不同，证明 0x36 是本机真实平台 ID。
- 选择子读数（2026-11-01，SYSTEM，只读）：

| 选择子 | MSR | 读回值 | `interfaces.md` §3.3 基线 |
|---:|---|---|---|
| 0 | 0x19B | `0x0000000000000010` | `0x10` |
| 1 | 0x19C | `0x0000000088200008` | `0x88250008`(动态位差异) |
| 2 | 0x1B0 | `0x000000008000FF01` | 同 |
| 3 | 0x772 | `0x0000019E7F003601` | `0x19E7F002501`(HWP 动态) |
| 4 | 0x774 | `0x000081E0005B81E0` | 同 |
| 5 | 0x65C | `0x0000000000000009` | 同 |

- 实现修正：vendor 包装返回 `DeviceIoControl` BOOL，驱动可能不回写输出缓冲
  （返回 0 字节即成功）；`require_zero_status` 已按此契约实现并有单测覆盖。
- 写入门禁：`0x65C`/`0x772`/`0x774` 位语义未按本机基线验证前，PL/HWP 写路径
  保持 `unavailable`；能力行 `tune.msr` 动态探测如实报告。

**写入验收（owner 在场时一步执行）：**

```powershell
# 以 SYSTEM 运行(一次性计划任务);默认只读;--write 为等值回写(机器状态可证不变)
cargo build -p lctrl-hal-win --example msr_acceptance --offline
# 经 schtasks /RU SYSTEM 运行;输出作为写路径证据
```

## C26 · EnergyDrv 充电域只读真机验收（2026-11-01，用户态，无写入）

`cargo build -p lctrl-hal-win --example charge_domain_probe --offline` 在 TB14P-EZRA
用户会话直跑(无需 SYSTEM;`\\.\EnergyDrv` 以标准读写打开)。只读输出:

```
gbmd_status: 0x00860004 -> charge mode Rapid     (bit2 express ON;与厂商 HKCU 缓存 "Quick" 一致)
usb_charger: 0x0000cec0 -> ac on (supported), battery-mode on (supported)
night_charge probe: 0x00000011 -> supported=true (probe bit4 ≠ 实时状态,以 status 为准)
night_charge status: 0x00000001 -> enabled=false
temporary_charge: 0x00000001 -> permanent=true (非临时模式)
```

验收点:
- 充电模式解码与厂商注册表缓存相互印证(Rapid=Quick),`decode_gbmd_charge_mode`
  的 bit5/bit2 表在真机成立(C2 读回门禁的实机证据)。
- USB 两种模式支持位与 BIOS `AlwaysOnUSB=Enable` 一致;运行态 AC/电池模式均开。
- 夜间充电支持位为真、状态为关;发现并修正了"探测 bit4=当前状态"的错误推断
  (interfaces.md §11.3 更正)。
- 临时模式原始值 0x01=Permanent,与 docs/02 §5 契约一致。

> 2026-11-05 追加(不修改上面的逐字输出):`GetTemporaryMode`/`DoesSupportTemporaryMode`
> 的精确解码已从导出函数复核(interfaces.md §11.4、`docs/evidence/re-battery-adv-20261105.md`)。
> `0x11` 回包的 bit0 是有效位、模式在 `(raw>>4)&0x07ffffff`,不是位掩码;`0x01` 仍解为
> Permanent(非临时),结论不变,但 `battery temporary-mode` 输出改为
> `supported/valid/mode` 三个字段。

写路径(充电模式 GBMD 03/05/07/08、USB 0x0a/0x0b/0x12/0x13、夜间充电
0x80000012/0x12)契约经实测验证、实现带 dry-run/回滚/读回轮询,但**真机写入
验收待 owner 在场时按各命令执行**(本机记录仍为只读证据)。

## C30 · BIOS 暂存写 `--auto-restore` 真机验收(2026-11-03)

补 C29:此前 BIOS 暂存路径(`kbd fnlock`/`kbd fn-ctrl-swap`/`privacy * --persistent`/
`usb * --persistent`/`bios set --save`)接受 `--auto-restore` 但丢弃该标志,提交后
不恢复、不输出恢复行。已修复:`execute_fnlock`、`execute_persistent_bios_toggle`、
`BiosCommand::Set` 均经 `restore_bios_change` 重新暂存并回写捕获的旧值,恢复
回读不符即报 `FirmwareRejected`,绝不伪装恢复。

同次发现:NVRAM 暂存缓冲为**机器全局单缓冲**——客户端 B 暂存的值被未暂存的
客户端 C 的 SaveBiosSettings 提交,Discard 亦清除所有客户端的待写项。门禁
`require_owned_staging`/`bios.staging.owned` 更名为
`require_staging_contract`/`bios.staging.contract`,NativeWmi 报契约已验证;
确认文案披露"保存会提交全局缓冲(含其他工具的待写项)"。

目标机实测(记录 `docs/evidence/target-bios-autorestore-acceptance-20261103.txt`):

| 命令 | 写入 | 自动恢复(回读) |
|---|---|---|
| `privacy cam off --persistent` | IntegratedCamera=Disable | `restored IntegratedCamera=Enable (verified)` |
| `privacy mic off --persistent` | Microphone=Disable | `restored Microphone=Enable (verified)` |
| `privacy fingerprint off` | FingerprintReader=Disable | `restored FingerprintReader=Enable (verified)` |
| `kbd fnlock off` | HotkeyMode=Enable + F1-F12AsPrimaryFunction=Disable | 两键各 `restored ... (verified)` |
| `kbd fn-ctrl-swap off` | FoolProofFnCtrl=Disable | `restored FoolProofFnCtrl=Enable (verified)` |
| `usb always-on off --persistent` | AlwaysOnUSB=Disable | `restored AlwaysOnUSB=Enable (verified)` |

终态独立回读:HotkeyMode=Disable、F1-F12AsPrimaryFunction=Enable、
FoolProofFnCtrl=Enable、AlwaysOnUSB=Enable,与写前基线一致。

## C31 · 第二批 `--auto-restore` 与修复验收(2026-11-03)

续 C29/C30:补齐此前丢弃 `--auto-restore` 的其余可变命令(commit `00c6477`),
全部经各自的 setter 回放在写入前捕获的旧值并由该 setter 的已验证读回确认;
`docs/evidence/target-restore-batch2-20261103.txt` 记录逐条输出。

| 命令 | 结果 |
|---|---|
| `perf mode auto --auto-restore` | quiet→balanced→restored quiet;修复:restore 改用 setter 自身已验证报告(本机 `performance_state()` 因 PowerSlider 键缺 CURRENT_SETTING 必败) |
| `touchpad off --auto-restore` | Enabled→Disabled→restored Enabled;独立 status=enabled |
| `tune epp performance/power --auto-restore` | 两次均 restored EPP 127;`balance-power`(raw 192)无精确 PERFEPP 映射,诚实拒绝 |
| `tune turbo off --auto-restore` | `PowerReadValueMin` 不可用——本机 PERFBOOSTMODE 无范围元数据,预校验拒猜边界 |
| `panel brightness 40 --auto-restore` | 5→40→restored 5(verified) |
| `kbd fnlock off --auto-restore` | HotkeyMode/F1-F12AsPrimaryFunction 两键各 restored(verified) |
| `audio noise-cancel shared --auto-restore` | Shared→Shared,restored mic mode Shared(纯 Rust Elevoc RPC) |
| `audio dolby movie --auto-restore` | enabled=false Dynamic→true Movie→restored Dynamic/off |
| `battery charge-mode conservation --auto-restore` | rapid→conservation→restored rapid |
| `battery night-charge off --auto-restore` | 原为 off,写 off→restored off |
| `power scheme get/set`(PERFEPP GUID) | get 读回 ac/dc=2;set 因 `PowerReadValueMin` 缺失被拒,边界保持 |
| `bios discard --yes` | 全局暂存缓冲已清除(契约警告已输出) |
| `hotkey register` / `osd` | SSH 会话无交互桌面,`RegisterHotKey`/`CreateWindowExW` gated——环境边界而非通道失败 |
| `sense kbd-light-auto` | 维持 Unsupported:Vantage contract RPC 未恢复 |
| `update local-driver` | 仅 dry-run 可达(需签名 INF+HWID);catalog 类命令诚实 unavailable/unsupported |
| `sense status` / `wifi security` / `scan run battery` / `magicbay detect` / `perf temp` / `perf top` / `perf dbdc get` / `snapshot capture/diff` / `tune telemetry` / `scene list` | 全部真实输出 |

**fix.run 真机缺陷已闭合**(`1f01cb6`):`fix run` 在 `StartServiceW` 后立即
单次读回,SCM 异步过渡态(2=START_PENDING/3=STOP_PENDING)被误判为
VerifyMismatch,而写入实际生效且 ledger 无记录、声明的 rollback 不可达。
现 `await_terminal_state` 轮询至终态(上限 30s);终态校验仍失败则先执行该
规则声明的 rollback 再报错,rollback 亦失败时升级 ChannelUnavailable。
真机:wuauserv 修复→RUNNING→ledger 记账→scan 判已解决;服务已恢复 STOPPED。

## C32 · 运行时麦克风与自动键盘背光边界(2026-11-03)

- `privacy mic --runtime off --auto-restore`(`d51e093`):扩展 `PrivacyControl`
  的 `microphone_state()/set_microphone()` 到既有 `WindowsRuntimeDevices`
  内部麦克风 devnode 选择(INTELAUDIO/HDAUDIO 父节点的捕获端点)与受防护
  PnP 启停事务。真机:Disable→`restored runtime microphone: Some(Enabled)
  (verified)`;`privacy status` 现同时报告 `runtime_microphone`。Linux 无
  已验证 sysfs 通路,诚实 Unsupported。
- `sense kbd-light-auto`(保持 Unsupported,理由修正):`ThinkSmartSenseAddin.dll`
  1.0.0.77 字符串取证显示自动键盘背光经 `LPATSVC_AUTO_KBD_BACKLIGHT_`
  `REGISTRY_PATH`/`LISS_AUTO_KBD_BACKLIGHT_REGISTRY_PATH` 写 LPlatSvc/LITSSvc
  服务参数;21VG 无这两个服务(`Get-CimInstance` 空),写无消费者注册表不构成
  原版行为,不平替。`CommandNameGet/SetAutoKBDBacklight` 契约命令名已存档
  (interfaces.md §13.3)供未来机型恢复。

## C33 · 进程策略与剩余边界盘点(2026-11-03)

- `perf throttle|boost <pid> --auto-restore`:自产测试进程 normal→below_normal/
  above_normal→已恢复,`restored 1 process(es) to their pre-policy state`
  (`99ba6f5` 补上此前静默的恢复输出)。
- `sense attention-tracking`:legacy 设置管道(`com.lenovo.smartsense.settings`)
  对 0x00030101/0x00070101/0x000D0201/0x00100101/0x00160101(arg 1/2)全部返回 0;
  SmartSense.exe 1.0.8.5 分派表与 arg 校验已记录(interfaces.md §13.2),该服务
  版本无可用 legacy 写通道。
- `power.saver-once`:维持 Unsupported——厂商契约只声明"CPU 降频+关 DGPU+降亮度+
  收缩工作集"组合,未指定目标数值;本机又无 DGPU。通用原语(`power scheme set`
  PROCTHROTTLEMAX 范围元数据齐全、`panel brightness`、`perf throttle`)均可达,
  组合编排不编造"一键省电"语义。
- `tune turbo` / `power scheme set`(PERFEPP/PERFBOOST):`PowerReadValueMin`
  在本机无范围元数据,预校验拒猜边界,维持 unavailable。

## C34 · 命令面校对(2026-11-06)

本节只对齐**文档与 clap 声明**的差异,不新增硬件验收。基准为
`crates/vantage-cli/src/lib.rs`(`sha256 1afe6721…`,8078 行,命令面与 `f6f5aa…`/8067 行
逐项一致);双向差异与逐行定位见 [DOCS-TRUTH 校对](evidence/doc-truth-20261105.md)。

| 位置 | 原文 | 现行声明 | 处理 |
|---|---|---|---|
| #6 行(本文件 42 行) | `conservation --dry-run` | 无 `conservation` 子命令;`battery charge-mode conservation`(值枚举 normal\|conservation\|rapid) | 已就地改为完整拼写;回执与结论不变 |
| C6 表(233 行) | `vantage --yes tune turbo aggressive` | `TuneCommand::Turbo { state: Toggle }`,只接受 `on\|off` | 保留历史行,结果列加"当前不可解析"标注 |
| C6 结论(237-238 行) | turbo 扩展为 5 模式集 | 声明仍是 2 值;`grep` 无 `efficient*` 实现 | 已改为"扩展在现行声明中不存在" |
| C8 #38(293 行) | `wifi security/history/policy` | 三叶为 `security`/`history`/`clear-history` | 已就地更正叶名 |
| C8 #28(297 行) | `bios apply` + `--allow-destructive` | `BiosCommand` 无 `apply`,全局无 `--allow-destructive` | 已标注为未实现计划 |

未在本节处理、且属其他 owner 的同源偏差:`docs/03` 的 `perf pl1|pl2 [get|set]`、
`perf oc`、`perf mode set` 形态;`docs/04`/`docs/10` 的 `farfield`;
`docs/11` §9 把 `update download/install/ignore/rollback/schedule` 当可用契约;
`docs/interfaces.md` 对 `sense reset` 的"未暴露子命令"表述。
