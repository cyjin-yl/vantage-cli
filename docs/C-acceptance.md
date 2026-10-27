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

## C2 · 电池充电模式(养护/快充):写入被接受,回读通道不存在

**状态:写入可用性已验证;独立回读为"缺少接口契约"(不是实现遗漏)。**

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

结论:21VG 固件接受养护/快充写入,但**不存在**可在秒级读回的可观测状态通道;官方 UI 的回显来自
自身注册表缓存而非固件读取(docs/02 §3.3)。因此 `ChargeModeReader` 保持 `Unverified` 并拒绝
把"IOCTL 成功"当作语义生效;`battery charge-mode` 在 `capability` 矩阵中如实报告 `unsupported`。
真正的效果级验证需要把电池放电到 55% 以下再插电观察停充,属于需要现场物理操作的长周期实验,
当前远程会话无法完成,记录为待现场确认项。

> 写入的子命令全部来自 docs/01 §3.2 的已验证表(gen2 养护 `0x0d/0x0f`、快充 `0x07/0x08`),
> 未使用 `0x03/0x05`(运输模式)与任何未记录命令字。测量脚本只读调用文档化通道,不扫描控制码。

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
