# vantage

在终端里管理联想 ThinkBook 的硬件:性能模式、电池、屏幕、键盘、BIOS、更新与诊断。
不需要厂商控制台应用,不需要账号,没有遥测。

原则:**每一处写入都会回读确认**。当前机器上没有可用通道的功能会直接报告"不可用",
不会伪装成功。

## 风险提示

**This is vibe coded, use at your own risk.**

本项目大部分代码由 AI 生成。下表中标注"实测"的功能在 ThinkBook 14 G8+(21VG)上
经过真机验收;未标注的未经真机验证。请自行评估后使用。

## 安装

**Windows** — 从 [Releases](https://github.com/cyjin-yl/vantage-cli/releases) 下载
`vantage.exe`,放到任意目录即可运行,
无需安装。读取状态不需要管理员权限;修改硬件设置可能需要,且必须加 `--yes` 确认。

**Linux** — 从源码构建(见[开发](#开发))。部分在 Windows 上依赖厂商私有通道的功能,
在 Linux 上使用标准内核接口。

## 快速上手

以下命令均为只读,可放心执行:

```bash
vantage info            # 机器型号与可用能力
vantage doctor          # 只读通道探测与问题报告
vantage capability      # 本机每条命令的支持状态矩阵
vantage battery status  # 电量、健康度、电源适配器
vantage perf temp       # CPU 温度
```

写入前先预览,确认后再执行:

```bash
vantage --dry-run perf mode performance   # 仅演示计划,不写入
vantage --yes perf mode performance       # 执行、回读、确认
```

全局开关:`--json`(机器可读输出)、`--dry-run`(只校验不写)、`--yes`(确认写入)。
所有子命令支持 `-h`。

## 命令一览

| 用途 | 命令 |
|---|---|
| 性能与电源 | `perf mode`、`tune epp`、`tune turbo`、`power scheme list/get/set`、`perf temp` |
| 电池 | `battery status`、`battery adapter` |
| 屏幕 | `panel rate`(刷新率)、`panel brightness`(亮度) |
| 输入设备 | `touchpad status/on/off/recover`、`kbd backlight` |
| 音频 | `audio dolby`(杜比开关/预设切换)、`audio noise-cancel`(降噪模式) |
| 智能感知 | `sense status`、`sense lock-on-leave/wake-on-approach/pause-video/attention-tracking` |
| BIOS | `bios list`、`bios get <名称>` |
| 更新 | `update check`、`update local-driver`(本地驱动安装/回滚) |
| 网络 | `wifi security`(在线与已存网络的安全评估) |
| 诊断 | `doctor`、`fix scan/run`、`scan`、`snapshot capture/diff/restore` |
| 桌面集成 | `osd`、`hotkey`、`scene`、`daemon` |

## 实际支持表

`vantage capability` 是权威来源,请在目标机器上运行确认。在 ThinkBook 14 G8+ 上
的验收结果:

**✅ 实测可用**(真机写入或读取验收通过,见 `docs/C-acceptance.md` C1–C24):

| 功能 | 验收 |
|---|---|
| 性能模式切换(自动/安静/高性能) | C23 |
| CPU 能效偏好与睿频策略 | C5/C6 |
| Windows 电源方案读写 | C3/C16 |
| 内置屏幕亮度、刷新率 | C17/C1 |
| 触控板开关与失效恢复 | C7 |
| 电池多源状态读取 | C14 |
| Wi-Fi 安全评估 | C3 |
| MachineFix 检查/修复/回滚 | C3 |
| OSD、全局热键、自动场景 | C20 |
| BIOS 设置读取 | C11 |
| 硬件快照 | C3 |

**🔬 逆向接入**(2026-11-01 经目标机二进制逆向验证契约,实现+测试已推送;充电域待 C 系列补验收,Dolby 已带实机只读探针):
充电模式(GBMD 位掩码读回)、USB 关机供电运行时控制(0x831020e8)、夜间充电
(0x83102150)、临时充电模式只读(0x83102148)、DBDC 电池限流表只读
(`perf dbdc get`)、杜比音效开关/预设切换(`audio dolby`,经 DolbyRpcClient.dll
ALPC 端点,只切换不重实现 DSP,实机探针:第三方进程可绑定)、降噪模式
(`audio noise-cancel`,经 ElevocControlSDK ALPC 端点)、SmartSense 人体感知
设置(`sense *`,经 com.lenovo.smartsense.settings 管道)。契约见
docs/RE-interfaces.md §11–§14。

**⚠️ 有限验收**(部分路径验证,未全覆盖):遥测/温度流(C19)、本地驱动安装
(`update local-driver`)。

**❌ 不可用**(本机无可用通道,命令会如实拒绝):

| 功能 | 原因 |
|---|---|
| 充电阈值(自定义起止百分比) | 目标机无可验证的自定义阈值写入通道(负向验收 U1b);仅能力探测与如实拒绝 |
| 风扇模式(GAMEZONE) | 客户端已按逆向契约实现(Get/SetSmartFanMode),`perf.fan.mode` 能力行动态探测;21VG 固件拒绝所有 GAMEZONE 调用,如实报不可用 |
| CPU 功耗墙(PL1/PL2/tau) | Linux 走 RAPL sysfs(标准内核接口,含 dry-run/回滚);Windows 的 WinMsrDev 通道已实现但需 SYSTEM 身份,且 MSR 0x65C 位语义未经物理确认前保持写入门禁 |
| 厂商专属性能模式(极客模式等) | 该模式存在(联想 Vantage 提供),仅经联想专有驱动下发;HWP 请求 MSR 已在 WinMsrDev 选择子表内(0x772/0x774),但本机读回编码与 SDM 通用语义不一致,写入保持门禁直到物理确认 |
| 修改 BIOS 设置 | 固件暂存机制未验证,写入锁定;读取正常 |
| 智能感知键盘灯自动 | 需 Vantage contract RPC 通道,未恢复 |
| BIOS 刷写、BIOS 口令 | 安全考虑,明确不支持;本地 `.inf` 驱动经 `update local-driver` 支持 |

## 安全模型

- 只读命令不写入硬件。
- 写入需 `--yes`;`--dry-run` 先给出完整变更计划。
- 每次写入后回读核对,不一致自动回滚并报告。
- 不刷固件,不设置 BIOS 口令,不削弱 VBS/HVCI。

## 不包含的内容

不捆绑厂商二进制,没有应用商店、账号、遥测和广告。`capability` 中的
`unavailable` 表示"本机无已验证通道",是边界声明而非缺陷隐瞒。

## 文档

`docs/` 为工程记录:接口规格、逆向分析、逐项真机验收证据(C1–C24)、Linux
后端映射。验收证据从 `docs/C-acceptance.md` 看起。

## 开发

需要 Rust 1.85+。

```bash
cargo fmt --all -- --check
cargo check --workspace --all-targets
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo build --release -p vantage
```

CI 覆盖 Linux 与 Windows;硬件冒烟测试只在目标机执行,不进 CI。

仓库:[cyjin-yl/vantage-cli](https://github.com/cyjin-yl/vantage-cli)
