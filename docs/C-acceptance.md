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

逐项保存的工单原始验收条件、111 个原生命令能力观测、已有 C 节证据索引、远端分支 SHA
和全部 38 项覆盖检查见 `evidence/replacement-baseline-20261027.json`。
程序的只读基线见 `evidence/target-read-baseline-20261027.json`:
info、电池状态/适配器、电源计划、性能模式、风扇清单、温度、调优遥测、隐私状态、
BIOS 密码元数据、MagicBay 检测和 scan 清单共 **12 次实际运行,均退出 0,无超时**。
它们只证明相应读取路径,不证明控制或深度诊断已完成。

| 工单 | 当前准入结论与未完成项 |
|---|---|
| #1 事件/watch | 原生 WMI 源已有源码;CLI watch 尚未闭环,不得以 daemon IPC 代替 |
| #2 自动策略 | parser/catalog/手动计划已有;自动求值、配置恢复与执行闭环待完成 |
| #3 OSD | 当前未附接真实 overlay/热键动作消费者 |
| #4 能力/切换检查 | 原生矩阵可运行;平台依赖投影、实际服务绑定与替代就绪报告仍需修正 |
| #5 电池/适配器 | C14:来源分组及实时电压/功率只读对照通过;健康/寿命、适配器额定功率等仍待逐字段验收 |
| #6 充电/阈值 | C2:限定通道差分未获得独立状态回读;任意阈值仍无已验证写通道 |
| #7 夜间/临时充电 | 当前入口 Unsupported;支持性、状态及写契约待完成,不据此判硬件不适用 |
| #8 USB 供电 | 运行时覆盖未接通;BIOS 默认值需经现有事务路径暴露并单独验收 |
| #9 电源计划 | 实际枚举成功;高级字段/名称一致性仍须按工单逐项验证 |
| #10 性能模式 | C4:ITS V3 可读;写入缺版本绑定的已确认控制契约 |
| #11 风扇 | 清单读取成功;真实 RPM/手动策略/曲线未完成;不可把元数据当实时转速 |
| #12 温度/功耗 | 读取入口成功;有界 interval、源定位和实际功率指标待完成 |
| #13 DBDC | 当前命令矩阵未覆盖;读表、单位与受门控写入待完成 |
| #14 超频/SuperBoost | 当前命令矩阵未覆盖;条件能力与精确适用性待核实 |
| #15 GPU 模式/功耗墙 | 独立枚举仅见 Intel Arc B390 实体 GPU 和两个 ROOT 虚拟显示适配器;不得将虚拟适配器算独显,控制面按具体硬件继续门控 |
| #16 EPP/睿频 | C5/C6 已有真实写入/独立注册表回读/恢复;矩阵的 turbo 声明仍与执行器不一致 |
| #17 PL1/PL2/tau | 尚无可供精确写入与独立回读的目标机契约;不绕过 HVCI |
| #18 进程管理 | 原生事务执行器已有源码;CLI、恢复日志和真实拓扑策略未闭环 |
| #19 profile/省电事务 | 现有部分手动执行/restore;完整目标、配方及复合动作仍须完成 |
| #20 键盘灯 | 当前目标机方法不可用;不得以静态灯光元数据提升写能力 |
| #21 Win 锁/触控板 | C7 仅确认触控板恢复及非中断 smoke;新版禁用循环与 Win 锁尚未完成验收 |
| #22 内屏刷新率 | C1 已验收;要求交互式会话,不以 SSH 无显示路径误判硬件不支持 |
| #23 色彩/显示增强 | 当前写入口 Unsupported;按硬件/接口/授权分别核实,不是统一判不适用 |
| #24 运行时隐私 | 状态实际读取成功;逐层写入、独立读回及恢复证据须按版本保存 |
| #25 SmartSense/HPD | 当前无控制执行器;设备存在性与可调用契约需分开确认 |
| #26 Dolby | 当前无已授权控制执行器;保留许可/接口边界,不分析私有 SDK |
| #27 音频降噪 | 当前无受支持控制执行器;不把普通音频设备存在当作降噪支持 |
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
刷新率能力与 auto 语义取自固件 WMI 数据块,不写任何私有 IOCTL。

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
真正的效果级验证需要把电池放电到 55% 以下再插电观察停充,属于需要现场物理操作的长周期实验,
当前远程会话无法完成,记录为待现场确认项。

> 表中养护写命令 `0x0d/0x0f` 来自 docs/01 §3.2 的已验证表；本表未展示快充写入回执。
> 未使用 `0x03/0x05`(运输模式)与任何未记录命令字。回读测量仅使用文档化通道,不扫描控制码。

## C3 · Windows 全量测试与能力矩阵

**状态:已验收(可复现)。**

| 项 | 结果 |
|---|---|
| `cargo test --offline --workspace`(Windows,MSVC) | 全量通过,进程退出码 **0**(含 40+ 测试二进制、doc-tests) |
| `vantage capability` | 输出 111 行能力矩阵 + 汇总(available 14 / external 4 / limited 16 / unsupported 62 / unverified 15) |
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
| 恢复 | `vantage --yes tune turbo aggressive` | actual 2/2 | **AC=2 DC=2** |
| dry-run | `vantage --dry-run tune turbo off` | previous 2/2, requested 0/0 | AC=2 DC=2(未变) |

真机暴露并修复的缺陷:`PERFBOOSTMODE` 的 `PowerReadValueMin` 在本机返回不可用,旧实现因此把整条
睿频写入判定为不可用。修复为按文档枚举域(0..4)约束写入,`tune turbo` 也由布尔开关扩展为文档
模式集(`off|on|aggressive|efficient|efficient-aggressive`),并保留原值恢复能力。

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
| #18 进程调度 | `perf top/boost/throttle/restore/trim`:精确 PID + 创建时间身份守卫、拓扑派生的 P/E 亲和性掩码、用户态可恢复日志、显式一次性内存整理 | 源码 + 单测 | 目标机对自有进程实际读写优先级/亲和性并恢复;不得对关键系统进程操作 |
| #38 Wi-Fi | `wifi security/history/policy`:改用公开 WlanApi 读取当前认证/加密,历史仅存本地且默认脱敏 | 源码 + 单测 | 目标机读取当前连接的认证/加密并与系统显示核对;未连接/权限拒绝需如实报告 |
| #1 事件/watch | 有实验性 WMI 订阅与轮询路径 | 源码,未完成运行验证 | 实际变化、错误传播、断线重连、Ctrl+C/释放及 MagicBay 300 ms 重查 |
| #2 自动策略 | 有求值引擎、配置模型与尚在检查的 daemon 集成 | 引擎单测,运行闭环未验收 | 不得把 UTC 冒充本地时间、硬件前值或未执行的回退当成功;完整场景切换和恢复待验证 |
| #3 OSD | 有实验性 Windows overlay 与事件消费者 | 尚无真实画面或多屏证据 | 显示、非夺焦、生命周期、持久偏好及只显示实际回读均需修复并验证 |
| #28 BIOS | `bios apply` 批量事务(复用 HAL 的 `plan_bios_apply`/`apply_bios_transaction`),破坏性项需 `--allow-destructive`,JSON 区分 accepted/saved/pending-reboot | 源码 + 单测 | 目标机批量写入后逐项回读并恢复;不得自动重启 |
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
触控板叶节点完整禁用/恢复循环、其余 38 域的实际硬件效果和最小依赖切换依然按各自工单追踪。

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
`evidence/target-power-scheme-isolation-20261028.json`。#9 工单已关闭，但后续评论
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
