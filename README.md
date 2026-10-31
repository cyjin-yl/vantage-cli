# vantage

用你的 ThinkBook,但不装联想那套全家桶。

vantage 是一个命令行工具,能帮你管性能模式、电池、屏幕、键盘、BIOS、更新、Wi-Fi 这些硬件设置。没有账号,没有后台服务,没有广告,也不会偷偷传数据。

它有个原则:**写进去的每个设置,都会再读出来确认一遍**。做不到的它会直接说"这台机器上不行",不会假装成功了。

## 怎么用

Windows:去 [Releases](https://github.com/cyjin-yl/vantage-cli/releases) 下载 `vantage.exe`,随便放个地方就能跑,不用安装。

看个状态不需要管理员权限;改设置可能需要,而且一定要加 `--yes` 确认。

先随便看看(这些都是只读的,放心点):

```bash
vantage info            # 看看你这是什么机器,支持哪些功能
vantage doctor          # 检查一下各个通道好不好使
vantage capability      # 每个命令在这台机器上能不能用,一张表
vantage battery status  # 电池电量、健康度、电源适配器
vantage perf temp       # CPU 温度
```

改设置的姿势——先看计划,再真改:

```bash
vantage --dry-run perf mode performance   # 只演示,不动真格
vantage --yes perf mode performance       # 真改,改完会读回来确认
```

全局开关:`--json` 输出机器能读的格式,`--dry-run` 空跑一次不写硬件,`--yes` 确认写入。任何命令加 `-h` 看帮助。

## 都能干啥

| 你想干嘛 | 命令 |
|---|---|
| 切换性能模式 | `perf mode`(可选 auto / quiet / performance) |
| 调省电/性能细节 | `tune epp`、`tune turbo` |
| 管 Windows 电源方案 | `power scheme list / get / set` |
| 看电池 | `battery status`、`battery adapter` |
| 调屏幕 | `panel rate`(刷新率)、`panel brightness`(亮度) |
| 触控板 | `touchpad status / on / off / recover` |
| 键盘灯 | `kbd backlight` |
| 看 BIOS 设置 | `bios list`、`bios get 设置名` |
| 更新 | `update check`、`update local-driver`(装/回滚本地驱动) |
| Wi-Fi 体检 | `wifi security`(检查有没有裸奔的网络) |
| 体检修电脑 | `doctor`、`fix scan / run`、`scan`、`snapshot capture` |
| 桌面小功能 | `osd`(屏幕提示)、`hotkey`(热键)、`scene`(自动场景)、`daemon` |

在 ThinkBook 14 G8+ 上实测好用的:性能模式切换、CPU 省电/睿频策略、电源方案、屏幕亮度和刷新率、触控板救活、电池各项数据、Wi-Fi 安全体检、一键修复规则、OSD/热键/自动场景、读 BIOS 设置、硬件快照。

## 哪些干不了,为啥

这台机器上真没路子,不是偷懒:

- **充电模式 / 充电阈值 / USB 关机充电** —— 这代机器的固件压根没公开能写的口子,vantage 选择明说"不行",不瞎猜。
- **风扇转速 / CPU 功耗墙(PL1/PL2)** —— Windows 上唯一的通道是联想自家驱动,vantage 不走联想私有通道。(Linux 上反而有标准接口。)
- **联想专属性能模式(比如 geek 模式)** —— 系统层面没有对应的东西,只能提供有映射的模式。
- **改 BIOS 设置** —— 读可以,写先锁着,等把固件的写入机制验证清楚再说。
- **智能感知、杜比音效 / 降噪** —— 靠联想授权服务才能用,这边没有。
- **刷 BIOS、设 BIOS 口令** —— 故意不做,太危险。

## 安全吗

- 只读的命令绝不写硬件。
- 改设置必须加 `--yes`,`--dry-run` 会先告诉你打算改什么。
- 改完会读回来核对,对不上就自动改回去并告诉你。
- 永远不刷固件,永远不碰 BIOS 密码,永远不削弱系统安全机制(VBS/HVCI)。

## 想深入了解

`docs/` 里有完整的工程记录:接口规格、逆向分析、每一项功能在真机上的验收证据(C1–C24)。想知道"到底验没验证过",看 `docs/C-acceptance.md`。

想自己编译:装好 Rust 1.85+,然后 `cargo build --release -p vantage`。

仓库:[cyjin-yl/vantage-cli](https://github.com/cyjin-yl/vantage-cli)
