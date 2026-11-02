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
`--auto-restore` 让写入在回读确认后自动把设备恢复成写入前的状态(恢复同样回读核对,
不一致会报错而不是假装成功),适合验收与临时试验。
所有子命令支持 `-h`。

## 命令一览

| 用途 | 命令 |
|---|---|
| 性能与电源 | `perf mode`、`tune epp`、`tune turbo`、`power scheme list/get/set`、`perf temp` |
| 电池 | `battery status`、`battery adapter` |
| 屏幕 | `panel rate`(刷新率)、`panel brightness`(亮度);护眼和超分尚未完成 ABI 对齐或真机写入验收,见 interfaces §15.1 |
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
| 充电模式写入(GBMD) | C28 |
| 夜间充电写入 | C28 |
| USB 关机供电运行时开关(两态) | C28 |
| 杜比音效开关/预设写入 | C28 |
| 降噪模式写入 | C28 |

**🔬 逆向接入**(2026-11-01 经目标机二进制逆向验证契约,实现+测试已推送;写入通道的
真机验收见 C28):
充电模式(GBMD 位掩码读回)、USB 关机供电运行时控制(0x831020e8)、夜间充电
(0x83102150)、临时充电模式只读(0x83102148)、DBDC 电池限流表只读
(`perf dbdc get`)、杜比音效开关/预设切换(`audio dolby`)、降噪模式
(`audio noise-cancel`)。SmartSense 的旧管道写入未通过验收,不能列为实测可用。
护眼/超分的 Intel IGCL 结构与显示输出句柄已修正;当前只读探针未找到可写的
像素变换块,超分能力查询返回 INVALID_ARGUMENT,仍不能声称写入可用。
契约见 docs/interfaces.md §11–§15。

Dolby 和降噪客户端不再加载 Vantage 附加组件中的 DLL;它们仍分别要求
`DolbyDAXAPI` 和 `ElevocService` 服务端可用。卸载 Vantage 后服务是否仍在,
尚未验证;不要把客户端 DLL 无依赖等同于服务端无依赖。

**写入通道真机验收(C28,2026-11-02):**充电模式、夜间充电、USB 供电两态、杜比
开关/预设、降噪模式这六条写入路径已按"dry-run 预览 → `--yes` 真实写 → 独立通道
回读 → 恢复原值 → 再回读"的流程在目标机逐项验收通过,独立回读用的是与 CLI 无关的
EnergyDrv/GBMD 标志字、USB 标志字和厂商客户端只读探针。写入不再有额外门禁:
`--dry-run` 给出计划,`--yes` 确认后即写,每次写后回读核对。

养护模式另经生产 CLI `conservation --dry-run` → `--yes --auto-restore` 验证:
养护状态读回成功、自动恢复 Rapid,另启进程读回原始 GBMD。此项只是驱动状态与恢复
验收,不是物理 80% 停充或官方 Vantage UI 验收;见
[生产 CLI 回执](docs/evidence/conservation-cli-native-20261102.md)。

进程通道(`perf top/boost/throttle`)于 2026-11-03 按同一流程验收:dry-run 不改
进程状态,`--yes` 写入后由独立进程观察优先级(AboveNormal / BelowNormal),
`--auto-restore` 复原到该命令自己的快照值。仅覆盖优先级;线程优先级、亲和性
选择、工作集收缩与游戏白名单未实现。见
[真机记录](docs/evidence/native-acceptance-20261103.md)。

**尚未通过写入验收(如实报告,不是"已实现"):**SmartSense 的旧人体感知管道在本机
拒绝请求(命令 0x00090101/0x00070101/0x000d0101),HPD 三个值均不变;
这不能证明固件不支持 HPD RPC 通道。三个 HPD 开关现已改接纯 Rust rpcrt4 客户端,
不回退旧管道;[契约与独立验收步骤](docs/evidence/hpd-rpc-re-20261102.md)
已记录,但新通道的真机写入/独立读回/恢复尚未执行。

**2026-11-03 真机复核(见 [真机记录](docs/evidence/native-acceptance-20261103.md)):**
新写的进程通道(`perf top/boost/throttle`)已按"dry-run 无副作用 → `--yes` 写入 →
独立进程观察优先级 → `--auto-restore` 复原"验收通过;充电模式仍按
dry-run → 写入 → 独立回读 → 复原循环验收,机器已恢复为 Rapid。同一轮复核发现
自建 NDR 描述符的 DAX 与 HPD 通道在封送器内触发访问冲突(0xC0000005),并使整个
CLI 启动即崩溃;两条通道已从服务图摘除(`audio dolby` 与 HPD 写入如实报
Unsupported,HPD 状态改为标注来源的持久化注册表投影),降噪通道(Elevoc)不受影响,
仍可用。官方 Vantage UI 的 CDP 检查已尝试但未完成:该打包应用在自动化启动下
启动即退出,无法建立调试端口,因此**没有任何官方 UI 层面的验收结论**。风扇模式报
`unsupported: LENOVO_GAMEZONE_DATA.GetSmartFanMode`(本机型该提供者不暴露此方法);
PL1/PL2 报 `unsupported: tune.pl1|tune.pl2`(按依赖裁定不接联想专有 MSR 驱动)。
面板色域尚无符合依赖裁定的写入通道;护眼/超分仍无可用写路径(见 interfaces §15.1)。
只读能力查询不等于写入验收。

**⚠️ 有限验收**(部分路径验证,未全覆盖):遥测/温度流(C19)、本地驱动安装
(`update local-driver`)。

**❌ 不可用**(本机无可用通道,命令会如实拒绝):

| 功能 | 原因 |
|---|---|
| 充电阈值(自定义起止百分比) | 目标机无可验证的自定义阈值写入通道(负向验收 U1b);仅能力探测与如实拒绝 |
| 极限续航(`battery extreme-life`) | 完整原版策略已恢复,但等价同步父子进程 Job 生命周期需自有适当签名内核实现,且单一 UI 开关映射尚未绑定目标版本;不以普通节能或仅 EcoQoS 冒充。见 [契约与缺失前提](docs/evidence/extreme-life-re-20261102.md) |
| 风扇模式(GAMEZONE) | 客户端已按逆向契约实现(Get/SetSmartFanMode),`perf.fan.mode` 能力行动态探测;21VG 固件拒绝所有 GAMEZONE 调用,如实报不可用 |
| CPU 功耗墙(PL1/PL2/tau) | Linux 走 RAPL sysfs(标准内核接口,含 dry-run/回滚);Windows 的 WinMsrDev 通道已实现但需 SYSTEM 身份,且 MSR 0x65C 位语义未经物理确认前保持写入门禁 |
| 厂商专属性能模式(极客模式等) | 该模式存在(联想 Vantage 提供),仅经联想专有驱动下发;HWP 请求 MSR 已在 WinMsrDev 选择子表内(0x772/0x774),但本机读回编码与 SDM 通用语义不一致,写入保持门禁直到物理确认 |
| 修改 BIOS 设置 | 固件暂存机制未验证,写入锁定;读取正常 |
| 智能感知键盘灯自动 | 需 Vantage contract RPC 通道,未恢复 |
| 杜比音效(`audio dolby`) | 纯 Rust rpcrt4 通道 2026-11-01 曾通过真机写入验收(C28/C29),但 2026-11-03 同一调用在 NDR 封送器内触发访问冲突并使 CLI 启动即崩;通道已摘除,命令如实报 Unsupported,描述符差异待定位。见 [真机记录](docs/evidence/native-acceptance-20261103.md) |
| HPD RPC 通道(离开锁屏/接近唤醒/视频暂停) | 同上:自生成 NDR 描述符在真机封送器内触发访问冲突,通道已摘除;`sense status` 改为返回标注 `source=hpd-registry-store` 的持久化注册表投影,旧管道路径保留 |
| 面板色域切换 | 原版依赖联想交付的 MONITORCONTROL.dll(SmartColor addin),按依赖裁定不引用;契约已存档(interfaces §15) |
| 面板 overdrive | 未观察到所测 SPBTESTTOOL 设备,BIOSOD 0x1A0000 读 null;其他通道未证实(interfaces §15) |
| 面板护眼/超分(21VG) | IGCL 结构、显示输出句柄与 1.1 ABI 已按公开头文件修正;只读探针当前输出 0 个 pixtx 块,超分 caps 返回 INVALID_ARGUMENT,写路径仍关闭(interfaces §15.1) |
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
