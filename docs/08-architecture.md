# 08 · Rust CLI 架构设计蓝图

> 读者:实现者。本文是顶层设计:命令树、crate 划分、状态/IPC/错误/探测/安全模型。
> 功能语义与常量一律以 01-07 文档为准,本文只做组织结构设计。
> 交叉引用:调优模型见 `07-pantherlake-tuning.md` §6;配置 schema 见 `10-config-schema.md`。

## 1. 设计原则

1. **无状态优先**:每个 CLI 子命令独立可用,不依赖 daemon;状态只存在于
   固件/注册表/配置文件,daemon 不产生新的「事实源」。
2. **通道抽象**:所有硬件访问收敛到 HAL crate 的 trait,Windows/Linux 后端可互换,
   能力差异经能力探测暴露,不在调用侧 if-else。
3. **读写分离**:任何 `set` 先读缓存旧值、写入、延迟回读、输出「请求值/实际值」
   (官方读写分离语义的显式化,见 02 文档 §3.3)。
4. **最小权限**:读取路径尽量不需要提权;写入路径按需提权(见 §7 安全模型)。
5. **可观测**:所有子命令支持 `--json`;事件域提供 `watch` 流式子命令。

## 2. 命令树

```
vantage
├── info                                   # 机型/固件/通道能力矩阵(§5 探测结果)
├── doctor                                 # 只读通道探测与可行动问题报告
├── capability                             # 本机每条命令的观察状态矩阵(权威支持表来源)
├── cpu status                             # 只读:机型身份、固件标称频率、派生当前频率与处理器功率策略
├── battery
│   ├── status                             # 02 §7(电量+模式回读)
│   ├── health                             # 02 §7.4 设计/满充容量、磨损、循环计数(只读)
│   ├── adapter                            # 02 §8
│   ├── charge-mode {normal|conservation|rapid}      # 02 §3
│   ├── thresholds <start%> <stop%>                  # 02 §4.2
│   ├── extreme-life {on|off}                        # 02 §4.4；完整常驻策略未准入,非一次性 saver
│   ├── night-charge {on|off}                        # 02 §5
│   ├── temporary-mode                                 # 02 §5
│   └── watch [--interval S]                           # 02 §11 事件流(默认 1 s,最小 1 s)
├── usb
│   ├── always-on {on|off} [--persistent]            # 02 §6
│   ├── charge-on-battery {on|off} [--persistent]    # 02 §6
│   └── ports                                        # 只读:控制器/集线器/端口,及端口实际报告的速率与供电
├── power
│   ├── scheme {list|get NAME [--subgroup GUID --setting GUID]|apply NAME}   # 02 §10
│   ├── scheme set <SUBGROUP> <SETTING> {ac|dc} <V>  # 02 §10
│   ├── saver-once                                   # 02 §4.3
│   ├── ac                                           # 只读:线路状态、适配器身份与功率
│   └── draw                                         # 只读:电池包瞬时功率与来源
├── perf
│   ├── mode {auto|cool|performance|geek}            # 03 §2(ITS/Dispatcher);值别名 balanced→auto、quiet→cool
│   ├── fan {status|auto|manual|fullspeed|smart}     # 03 §3
│   ├── fan curve <json|file>                        # 03 §3(Fan_Set_Table)
│   ├── temp [read <id> | watch --interval S]        # 03 §4;裸 temp 列出全部传感器
│   ├── pl1 <W> / pl2 <W>                            # 03 §5.2 功耗墙(写路径按依赖裁定门控)
│   ├── dbdc get                                     # 03 §5.3 电池直放限流表(只读)
│   ├── policy                                       # 03 §2 Dispatcher `Policy_*` PowerSlider 解码(只读)
│   ├── gpu status                                   # 03 §5.4 GPU 超频能力与参数块(只读)
│   ├── boost <PID>... / throttle <PID>...           # 03 §6 进程级
│   └── top                                          # 03 §6 进程功耗视图
├── tune
│   ├── profile {list|show NAME|apply NAME [--dry-run]}   # 07 §5
│   ├── pl1 W [--pl2 W] [--tau S]                    # 07 §3.1
│   ├── epp {performance|balance-performance|balance-power|power|0-255}
│   ├── turbo {on|off}                               # 07 §3.2
│   ├── restore                                      # 07 §7
│   ├── telemetry [--interval S]                     # 07 §4;不带 --interval 为单次采样
│   └── watch [--interval S]                         # 07 §6 触发/回退事件(默认 1 s)
├── kbd
│   ├── backlight {off|low|high|auto|0..3} [--effect static|breath]   # 04 §2
│   ├── fnlock {on|off}                              # 04 §1.2
│   ├── fn-ctrl-swap {on|off}                        # 04 §1.1
│   └── winlock {on|off}                             # 04 §1.3
├── touchpad {on|off|status|recover|caps}              # 04 §1.4;值别名 enable/disable;caps 只读 HID 能力描述符
├── panel
│   ├── info                                         # 只读:内屏身份(EDID 解码:厂商/型号/序列/制造日期/原生分辨率/物理尺寸/连接类型)
│   ├── brightness [0..100] [--expect-current N]     # 04 §3.0;--expect-current 要求同时给目标值
│   ├── rate {60|120|auto}                           # 04 §3.1
│   ├── color {srgb|dci-p3|adobe|custom|movie}       # 04 §3.2
│   ├── super-resolution {on|off}                    # 04 §3.4
│   ├── overdrive {on|off}                           # 04 §3.3
│   └── eye-care {off|mid|high}                      # 04 §3.5
├── display status                                   # 只读:当前模式、逐显示器缩放、HDR/广色域;`displays`(调用会话桌面)与 `monitors`(机器级驱动/EDID)两域分开报告,不合并
├── privacy
│   ├── cam {on|off|status} [--runtime|--persistent] # 04 §4;子命令别名 camera,值别名 enable/disable
│   ├── mic {on|off} [--runtime|--persistent]
│   ├── fingerprint {on|off}
│   └── status                                       # 双层状态(运行时+BIOS)
├── sense                                              # 04 §5(智能感知)
│   ├── status
│   ├── lock-on-leave {on|off} [--distance N] [--wait S]
│   ├── wake-on-approach {on|off} [--distance N]
│   ├── pause-video {on|off}
│   ├── camera-assist {on|off}
│   ├── approach-auto-adjust {on|off}
│   ├── leave-auto-adjust {on|off}
│   ├── attention-tracking {on|off} [--dim] [--ac-only]
│   ├── kbd-light-auto {on|off}
│   └── reset                                        # op7 ResetAllSetting(破坏性;配合 --auto-restore 复原)
├── sensor {list|read <target>}                      # 只读:本机实例化的传感器清单(身份/状态/上报时间/新鲜度)与单次读数;不可用传感器单列,不填 0
├── audio
│   ├── dolby {off|movie|music|voice|game|personalize|dynamic}   # 04 §6.1
│   ├── noise-cancel {off|single|shared|spatial|voice-id} # 04 §6.2(无 farfield 模式)
│   ├── endpoints [--all]                            # OS 音频端点清单(MMDevice)
│   ├── volume [0..100] [--device ID]                # 端点主音量,省略值即读取
│   └── mute [on|off] [--device ID]                  # 端点静音,省略值即读取
├── bios
│   ├── list                                         # 05(枚举 BiosSelections)
│   ├── get <NAME> / set <NAME> <VALUE> [--save]     # 05;提交走 `set --save`
│   ├── save / discard / defaults [--password C]       # 05;`save` 本身拒绝(全局暂存缓冲,见 05 §6)
│   └── password {status|set|clear|verify}             # 05 §3;status/verify 只读可用,写入不支持
├── firmware status                                  # 只读:固件身份、ESRT 条目与最近一次尝试
├── magicbay
│   ├── detect                                       # 06 §3
│   ├── lte {status|connect|disconnect|apn ...}      # 06 §4
│   ├── cam / display                                # 06 §5/§6
│   └── watch                                        # 06 §3 热插拔事件
├── osd {enable|disable|test|show <行…>}             # 04 §7:enable/disable = 联想 Fn OSD 偏好(服务控制码 146/147,148/149 + 注册表读回);test/show = 替代 overlay;裸 `osd` 等同 `osd test`
├── hotkey {list|status|register [id] [vk]|trigger [id]|unregister [id]}   # 04 §7 全局热键;裸 `hotkey` 等同 `hotkey list`
├── update
│   ├── check [--severity critical|recommended|driver|optional]   # 11
│   ├── local-driver {apply <INF> <HWID> [--dry-run]|rollback <实例ID> + 身份/恢复开关 [--dry-run]}   # 11
│   ├── local-install <PACKAGE>                      # 11(UDC 已下载包自带签名 install.ps1;拒绝固件包)
│   ├── download <PKG>... / install <PKG>... [--reboot]   # 11(需已认证清单,当前如实拒绝)
│   ├── history / ignore <ID> / rollback <ID>        # 11
│   └── schedule {daily|weekly|off}                  # 11
├── scan {list|run <item>...}                        # 11;裸 `scan` 无派发,必须带子命令
├── snapshot {capture|diff|restore}                  # 11
├── wifi {status|security|history|clear-history}     # 11(status=当前关联:SSID/BSSID/信号/频段/信道/协商安全,不触发扫描;history 为本进程内存历史)
├── bluetooth list                                   # 只读:本机已知设备(配对/记住/已连接)的地址、类别、状态与最近使用;不发起发现
├── radio {status|off <wifi|bluetooth|wwan> [--force]|on <wifi|bluetooth|wwan>}   # 无线开关,写后回读;承载当前会话的射频默认拒绝
├── security status                                  # 只读:Secure Boot / TPM / BitLocker 三路状态,各自标注来源;不读密钥、不改固件变量
├── storage status                                   # 只读:物理盘清单与各自实际应答的身份/健康字段
├── events [--limit N]                               # 只读:本机事件台账尾部(默认条数由 HAL 常量决定)
├── fix {scan|run <issue>|track}                     # MachineFix 透明规则集;裸 `fix` 等同规则集清单;dry-run 用全局 `--dry-run`
├── scene {list|test <event> [--rule ID]|track}      # 自动场景策略;裸 `scene` 等同规则集清单;dry-run 用全局 `--dry-run`
├── daemon {start|stop|status|install}               # §4(可选常驻)
└── completions {bash|elvish|fish|powershell|zsh}    # 自描述
```

全局开关对全部子命令生效(`--json`、`--dry-run`、`--yes`、`--auto-restore`),`-h`/`--help`
由 clap 提供,`-V`/`--version` 只在根命令。闭包边界(裸调用)有例外:`battery`→`status`、
`bios`→`list`、`fix`/`scene`→规则集清单、`osd`→`test`、`hotkey`→`list`;而
`sense`/`audio`/`snapshot`/`wifi` 要求显式子命令,`scan`/`daemon` 的裸调用如实拒绝。
本树与代码声明的双向校对见 `docs/evidence/doc-truth-20261105.md`。

命令命名与优先级对应 charter §4.4:P0 = battery/perf/kbd/panel.rate/bios;P1 = tune/privacy/
sense/usb/magicbay.lte;P2 = 其余。

## 3. crate 划分(cargo workspace)

```
vantage/                 # 单一 workspace
├── crates/
│   ├── lctrl-core/      # 领域模型:ChargeMode/FanCurve/PerfMode/Profile/Capability/Error
│   │                    # 纯数据+纯逻辑,零 OS 依赖;所有单元测试在此
│   ├── lctrl-hal/       # trait Hal { battery(), thermal(), kbd(), panel(), privacy(),
│   │                    #          bios(), tuning(), events() } + 能力位结构
│   ├── lctrl-hal-win/   # Windows 后端:WMI(wmi crate)/ DeviceIoControl(windows-sys)/
│   │                    # Power API / SCM / 注册表;编译期 gate:cfg(windows)
│   ├── lctrl-hal-linux/ # Linux 后端:sysfs / ideapad_laptop / acpi_call / powercap /
│   │                    # intel_pstate / ModemManager D-Bus;cfg(unix)
│   ├── lctrl-tune/      # 调优引擎:07 §6 模型求值器,触发器、约束、回退、防抖
│   ├── vantage-cli/     # clap 命令树(§2),--json 输出,退出码(§6)
│   ├── vantage-daemon/  # 可选常驻:事件订阅、自动策略、OSD 占位;与 CLI 同库零重复逻辑
│   ├── vantage-gui/     # 可选图形仪表盘
│   └── vantage (bin)    # 薄壳:vantage-cli main
```

依赖方向:`cli/daemon → tune → core ← hal ← hal-{win,linux}`。
**禁止** hal-win 与 hal-linux 互相引用;共享逻辑一律上提到 core。

## 4. 状态与 daemon

### 4.1 状态分布(事实源唯一)

| 状态 | 存放 | 说明 |
|---|---|---|
| 硬件当前值 | 固件(EC/BIOS/WMI) | 永远以回读为准,不缓存为事实 |
| 用户偏好(充电模式等) | 注册表(Win,与官方同键以便互操作)或 `config.toml` | 见 10 §1 |
| 调优 profile/DSL | `config.toml` + `profiles/*.toml` | 10 §2/§3 |
| daemon 运行时(当前 profile/触发计数) | `state.json`(易失) | 10 §4 |
| 恢复出厂用快照 | `state.json` 内 `saved_before_apply` | tune restore |

### 4.2 daemon 唯一职责

1. 事件监听(WMI 事件 / udev / upower)→ 触发 tune 引擎(07 §6);
2. 自动策略施加与回退、外部覆写检测;
3. OSD(daemon 内嵌轻量 OSD,替代 FnHotkeyUtility);
4. `watch` 子命令的事件源(无 daemon 时 watch 直接自行订阅,功能不缺失)。

**CLI 的每个功能在无 daemon 时必须可用**——daemon 只做「自动化」,不做「能力」。

### 4.3 IPC

- 协议:单行 JSON 请求/响应 + 事件流(server-push);schema 与 `--json` 输出同构。
- 通道:Windows `\\.\pipe\vantage.sock`;Linux `$XDG_RUNTIME_DIR/vantage.sock`(fallback `/run/vantage.sock`)。
- 鉴权:Windows pipe ACL = 本机 Administrators + 当前用户;Linux = 同 uid + SO_PEERCRED 校验。
- 事件输入和 IPC 状态请求异步处理；成功发送事件不保证紧接着的 status 已消费该事件。
  消费者须等待可识别的目标事件再判断历史；历史保留最新 64 条，订阅者队列满时移除
  慢订阅者而不阻塞事件摄取。

## 5. 能力探测(`vantage info`)

启动时按矩阵探测并缓存于进程内(每次 CLI 调用重探,无持久缓存):

| 探测项 | 方法 | 降级 |
|---|---|---|
| 机型/平台 | SMBIOS(ProductName/Family/BIOSVersion) | 非 21VG 时按能力位继续,不硬拒绝 |
| EnergyDrv 通道 | CreateFile `\\.\EnergyDrv` | 下发型功能整体不可用,只读保留 |
| WMI Lenovo 类 | 逐类 `Get-CimClass` 等价查询 | 按类粒度禁用子命令 |
| GBMD/快充/养护支持 | 02 §3.5(0xff 查询 + 特征列表 + 39Wh 规则) | 隐藏对应子命令 |
| DTT/IPF | 枚举 INTC10D4/10D5/10D8 设备 + ESIF IPC(ipfsvc) | tune 降级到 RAPL/sysfs |
| RAPL | Linux powercap 存在性 / Win MSR 驱动可用性 | pl1/pl2 子命令报「通道不可用」 |
| 智能感知 | ACPI\IDEA2002 + SmartSense 服务 | sense 整体不可用 |
| MagicBay | USB VID_17EF&PID_7005 存在性 | magicbay 显示「未插入」 |
| Dolby/降噪 | 音频端点 + 对应 SDK 服务 | audio 子命令提示依赖缺失 |

输出:`vantage info --json` 输出完整能力位,供脚本与 daemon 消费。

## 6. 错误模型与退出码

```rust
enum LctrlError {
    Unsupported { feature: &'static str },   // 能力探测否定
    ChannelUnavailable { channel: String },  // 驱动/服务/WMI 不可达
    PermissionDenied { need: &'static str }, // 需要 admin/root
    FirmwareRejected { detail: String },     // WMI false / 状态字非 0
    InvalidArgument { detail: String },      // 参数域错误(如 thresholds 95<60)
    VerifyMismatch { requested: String, actual: String }, // 回读校验失败
    Io(std::io::Error),
}
```

退出码:`0` 成功;`2` 参数错误;`3` 不支持;`4` 通道不可用;`5` 权限不足;
`6` 固件拒绝;`7` 回读不一致(set 提交但值未生效);`1` 其他。
`--json` 时错误也以 JSON 输出 `{"error": {...}}` 并以非零码退出。

## 7. 安全模型

| 级别 | 操作 | 要求 |
|---|---|---|
| L0 只读 | status/info/telemetry/panel get | 任意用户(Windows);Linux 部分节点需 udev 规则(09 §8) |
| L1 用户写 | kbd backlight、panel rate、perf mode(经服务) | 当前用户;Windows 服务路径需服务在场 |
| L2 提权写 | charge-mode、fan、pl1/pl2、usb 开关、privacy 运行时 | Windows 本地管理员;Linux root 或 udev 放权 |
| L3 风险写 | bios set/save/defaults/password、thresholds 极端值、overdrive | L2 + **二次确认**(TTY 交互 `--yes` 跳过),操作前打印影响与恢复路径 |

二次确认内容必须含:将写入的键与值、生效时机(即时/重启)、恢复命令。
`vantage bios defaults` 额外要求输入机型名确认(brick 风险,05 §6)。

## 8. 插件化调优 trait(与 07 §6、10 §2 呼应)

```rust
trait TuningTarget {                    // 一个可写参数面
    fn id(&self) -> &'static str;       // "pl1_w" | "epp" | "fan_mode" | ...
    fn read(&self) -> Result<Value>;
    fn write(&self, v: &Value) -> Result<()>;
    fn supported(&self) -> bool;
}
trait Trigger { fn arm(&self, ctx: &mut EventStream) -> Result<()>; }   // on_ac/process/temp/...
struct Profile { goal: Vec<(TargetId, Value)>, triggers: Vec<Box<dyn Trigger>>,
                 constraints: Constraints, fallback: Fallback }
```

新参数面(如未来机型的 GPU 功率)只需注册新 `TuningTarget`,DSL/CLI/daemon 自动获得能力。

## 9. 测试策略

- core/tune:纯逻辑单测(状态机、DSL 求值、触发优先级、回退计数);
- hal:接口级 mock(trait 实现假后端)覆盖 CLI 行为契约;
- 真机冒烟:`tests/smoke.rs`(仅手工运行,`--ignored`)逐项读操作 + 往返写(set→get→restore);
- 不在 CI 跑任何需要硬件的测试。

## 10. 里程碑建议

1. M1(通道层):hal-win WMI + EnergyDrv 打通,`info`/`battery status`/`perf temp` 只读可用;
2. M2(P0 写路径):charge-mode/perf mode/kbd backlight/panel rate/bios get-set;
3. M3(tune 引擎):profile apply + pl1/pl2/epp + telemetry;
4. M4(daemon 与事件):watch 族、自动策略、OSD;
5. M5(Linux 后端):09 文档映射,功能对等矩阵验收;
6. M6(MagicBay/音频/感知):P1-P2 收尾。
