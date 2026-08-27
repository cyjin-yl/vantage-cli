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
lctrl
├── info                                   # 机型/固件/通道能力矩阵(§5 探测结果)
├── battery
│   ├── status [--json]                    # 02 §7 + §8(信息+适配器+模式回读)
│   ├── charge-mode {normal|conservation|rapid}      # 02 §3
│   ├── thresholds <start%> <stop%>                  # 02 §4.2
│   ├── extreme-life {on|off}                        # 02 §4.1/4.3
│   ├── night-charge {on|off}                        # 02 §5
│   ├── temporary-mode                                 # 02 §5
│   ├── adapter [--json]                             # 02 §8
│   └── watch                                          # 02 §11 事件流
├── usb
│   ├── always-on {on|off} [--persistent]            # 02 §6
│   └── charge-on-battery {on|off} [--persistent]    # 02 §6
├── power
│   ├── scheme {list|get|apply NAME}                 # 02 §10
│   ├── scheme set <SUBGROUP> <SETTING> {ac|dc} <V>  # 02 §10
│   └── saver-once                                   # 02 §4.3
├── perf
│   ├── mode {auto|cool|performance|geek}            # 03 §2(ITS/Dispatcher)
│   ├── fan {status|auto|manual|fullspeed|smart}     # 03 §3
│   ├── fan curve <json|file>                        # 03 §3(Fan_Set_Table)
│   ├── temp [--json]                                # 03 §4
│   ├── boost <PID>... / throttle <PID>...           # 03 §6 进程级
│   └── top                                          # 03 §6 进程功耗视图
├── tune
│   ├── profile {list|show|apply NAME [--dry-run]}   # 07 §5
│   ├── pl1 W [--pl2 W] [--tau S]                    # 07 §3.1
│   ├── epp {performance|balance-performance|balance-power|power|0-255}
│   ├── turbo {on|off}                               # 07 §3.2
│   ├── restore                                      # 07 §7
│   ├── telemetry [--json] [--interval S]            # 07 §4
│   └── watch                                        # 07 §6 触发/回退事件
├── kbd
│   ├── backlight <0..3> [--effect static|breath]    # 04 §2
│   ├── fnlock {on|off}                              # 04 §1.2
│   ├── fn-ctrl-swap {on|off}                        # 04 §1.1
│   └── winlock {on|off}                             # 04 §1.3
├── touchpad {on|off}                                # 04 §1.4
├── panel
│   ├── rate {60|120|auto}                           # 04 §3.1
│   ├── color {srgb|dci-p3|adobe|custom|movie}       # 04 §3.2
│   ├── super-resolution {on|off}                    # 04 §3.4
│   ├── overdrive {on|off}                           # 04 §3.3
│   └── eye-care {off|mid|high}                      # 04 §3.5
├── privacy
│   ├── cam {on|off} [--runtime|--persistent]        # 04 §4
│   ├── mic {on|off} [--runtime|--persistent]
│   ├── fingerprint {on|off}
│   └── status                                       # 双层状态(运行时+BIOS)
├── sense                                              # 04 §5(智能感知)
│   ├── status
│   ├── lock-on-leave {on|off} [--distance N] [--wait S]
│   ├── wake-on-approach {on|off} [--distance N]
│   ├── pause-video {on|off}
│   ├── attention-tracking {on|off} [--dim] [--ac-only]
│   └── kbd-light-auto {on|off}
├── audio
│   ├── dolby {off|movie|music|voice|game|personalize|dynamic}   # 04 §6.1
│   └── noise-cancel {off|single|shared|spatial|voice-id|farfield} # 04 §6.2
├── bios
│   ├── list [--json]                                # 05(枚举 BiosSelections)
│   ├── get <NAME> / set <NAME> <VALUE>              # 05
│   ├── save / discard / defaults                    # 05
│   └── password {set|clear|verify}                  # 05 §3
├── magicbay
│   ├── detect                                       # 06 §3
│   ├── lte {status|connect|disconnect|apn ...}      # 06 §4
│   ├── cam / display                                # 06 §5/§6
│   └── watch                                        # 06 §3 热插拔事件
├── osd {enable|disable|test}                        # 04 §7 daemon OSD
├── daemon {start|stop|status|install}               # §4(可选常驻)
└── completions <shell>                              # 自描述
```

命令命名与优先级对应 charter §4.4:P0 = battery/perf/kbd/panel.rate/bios;P1 = tune/privacy/
sense/usb/magicbay.lte;P2 = 其余。

## 3. crate 划分(cargo workspace)

```
lctrl/                 # 单一 workspace
├── crates/
│   ├── lctrl-core/    # 领域模型:ChargeMode/FanCurve/PerfMode/Profile/Capability/Error
│   │                  # 纯数据+纯逻辑,零 OS 依赖;所有单元测试在此
│   ├── lctrl-hal/     # trait Hal { battery(), thermal(), kbd(), panel(), privacy(),
│   │                  #          bios(), tuning(), events() } + 能力位结构
│   ├── lctrl-hal-win/ # Windows 后端:WMI(wmi crate)/ DeviceIoControl(windows-sys)/
│   │                  # Power API / SCM / 注册表;编译期 gate:cfg(windows)
│   ├── lctrl-hal-linux/ # Linux 后端:sysfs / ideapad_laptop / acpi_call / powercap /
│   │                  # intel_pstate / ModemManager D-Bus;cfg(unix)
│   ├── lctrl-tune/    # 调优引擎:07 §6 模型求值器,触发器、约束、回退、防抖
│   ├── lctrl-cli/     # clap 命令树(§2),--json 输出,退出码(§6)
│   ├── lctrl-daemon/  # 可选常驻:事件订阅、自动策略、OSD 占位;与 CLI 同库零重复逻辑
│   └── lctrl-ipc/     # CLI↔daemon 协议(§4.3),Unix socket / Windows named pipe
└── lctrl (bin)        # 薄壳:lctrl-cli main
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
- 通道:Windows `\\.\pipe\lctrl.sock`;Linux `$XDG_RUNTIME_DIR/lctrl.sock`(fallback `/run/lctrl.sock`)。
- 鉴权:Windows pipe ACL = 本机 Administrators + 当前用户;Linux = 同 uid + SO_PEERCRED 校验。

## 5. 能力探测(`lctrl info`)

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

输出:`lctrl info --json` 输出完整能力位,供脚本与 daemon 消费。

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
`lctrl bios defaults` 额外要求输入机型名确认(brick 风险,05 §6)。

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
