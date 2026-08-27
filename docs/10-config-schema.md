# 10 · 配置文件 Schema 与调优 DSL 规范

> 读者:实现者。定义全部持久化格式:主配置、调优 DSL、profile 文件、daemon 运行时状态。
> 交叉引用:参数语义见 `07-pantherlake-tuning.md`;执行模型见 `08-architecture.md` §8;
> 充电/外设等字段语义见 02-06 各文档。

## 1. 主配置 `config.toml`

### 1.1 查找与覆盖顺序(后者覆盖前者)

1. 内置默认值(编译进二进制);
2. 系统级:Windows `%ProgramData%\lctrl\config.toml`;Linux `/etc/lctrl/config.toml`;
3. 用户级:Windows `%APPDATA%\lctrl\config.toml`;Linux `${XDG_CONFIG_HOME:-~/.config}/lctrl/config.toml`;
4. CLI 参数 / 环境变量(`LCTRL_*`)。

### 1.2 全字段 schema

```toml
# ---- 通用 ----
[general]
output = "human"            # human | json(等价 --json)
confirm_risky = true        # L3 风险操作是否二次确认(08 §7)
verify_after_set = true     # set 后回读校验(02 §3.3)
verify_delay_ms = 50
verify_retries = 10

# ---- 电池(02) ----
[battery]
charge_mode = "normal"      # normal | conservation | rapid;启动恢复用
# thresholds = { start = 75, stop = 80 }   # 可选;与 charge_mode 互斥(02 §4)
night_charge = "off"        # on | off
[usb]
always_on = true
charge_on_battery = false

# ---- 性能(03) ----
[perf]
mode = "auto"               # auto | cool | performance | geek(按能力位)
fan_mode = "smart"          # auto | manual | fullspeed | smart | off
# fan_curve = [ {temp=40, pct=0}, {temp=60, pct=40}, {temp=80, pct=100} ]  # 03 §3

# ---- 外设(04) ----
[kbd]
backlight = 2               # 0..State_Type_Num-1
backlight_effect = "static" # static | breath | wave | reactive | flash
backlight_auto_off_s = 0    # 0=禁用自动熄灯
fnlock = false
fn_ctrl_swap = false
winlock = false
[touchpad]
enabled = true
[panel]
refresh = "auto"            # 60 | 120 | auto(VRR Mode=1)
color = "srgb"              # srgb | dci-p3 | adobe | custom | movie
super_resolution = false
overdrive = false
eye_care = "off"            # off | mid | high
[privacy]
camera = true               # BIOS 层期望态(重启后生效)
microphone = true
[sense]
lock_on_leave = { enabled = false, distance_cm = 50, wait_s = 10 }
wake_on_approach = { enabled = false, distance_cm = 50 }
pause_video = false
attention_tracking = { enabled = false, dim = false, ac_only = true }
kbd_light_auto = false
[audio]
dolby = "dynamic"           # off|movie|music|voice|game|personalize|dynamic
noise_cancel = "off"        # off|single|shared|spatial|voice-id|farfield

# ---- 调优(07) ----
[tune]
active_profile = "balanced" # 常驻 profile;daemon 施加
on_ac = "plugged-max"       # 可选:AC 插入时切换
on_battery = "long-battery" # 可选:电池时切换
min_dwell_s = 10            # 防抖(07 §6)

# ---- daemon(08 §4) ----
[daemon]
enabled = false
osd = true
events = ["ac", "battery", "process", "thermal"]  # 订阅源白名单
```

校验规则(serde deny_unknown_fields):
- `thresholds.start < thresholds.stop`,5 ≤ start ≤ 95,10 ≤ stop ≤ 100;
- `charge_mode` 与 `thresholds` 同时出现 → 报错(互斥);
- `backlight ≤ State_Type_Num-1`(运行时按能力探测校验);
- 所有枚举未知值 → 报错并列出合法值。

## 2. 调优 DSL(profile 语法)

### 2.1 模型(07 §6 的文件形式)

```toml
# profiles/<name>.toml
[profile]
name = "long-battery"
description = "长续航:低功耗墙 + 保守调度"
priority = 50               # 冲突时高者胜;内置 profile 0-99,用户 100+
inherits = "balanced"       # 可选:字段级覆盖

[goal]                      # 目标参数面(07 §3;键 = TuningTarget id)
ec_mode = 1                 # 0 智能/1 节能/2 野兽
pl1_w = 12
pl2_w = 20
tau_s = 28
epp = "balance-power"       # 或 0-255
turbo = true
fan_mode = "smart"
panel_hz = 60
dgpu = "igp-priority"       # 若平台适用
charge_mode = "conservation"
backlight = 1

[[trigger]]                 # 触发器(见 2.2);无触发器 = 常驻
type = "on_battery"

[constraints]               # 生效前置条件
temp_max_c = 85
battery_only = true
min_dwell_s = 10

[fallback]
on_temp_exceed = "silent-library"   # 温度越限切换
on_conflict = "restore-factory"     # 外部覆写 N 次失败后:restore-factory | give-up
max_reapply = 5
```

### 2.2 触发器类型

| type | 参数 | 事件源(Windows / Linux) |
|---|---|---|
| `on_ac` / `on_battery` | — | LENOVO_AC_PD_EVENT / upower |
| `process_match` | `names = ["game.exe", …]` 或 `paths` | 进程创建轮询/ETW / proc netlink |
| `temp_above` / `temp_below` | `celsius`, `hysteresis = 3` | DTT SEN / hwmon |
| `power_above` / `power_below` | `watts`, `hysteresis` | RAPL 遥测 |
| `time_range` | `from = "22:00", to = "07:00"` | 计时器 |
| `battery_below` | `percent` | 电池事件 |

求值优先级(固定):`process_match` > `temp_*` > `on_ac/on_battery/battery_below` > `time_range` > 常驻。

### 2.3 完整示例

```toml
# ---- silent-library.toml ----
[profile]
name = "silent-library"
priority = 60
[goal]
ec_mode = 1
pl1_w = 9
pl2_w = 15
tau_s = 28
epp = "balance-power"
turbo = false
fan_mode = "smart"
panel_hz = 60
backlight = 1
charge_mode = "conservation"
[goal.background]
workset_trim = true         # 后台进程工作集收缩
priority = "low"
[[trigger]]
type = "time_range"
from = "08:00"
to = "22:00"
[constraints]
temp_max_c = 80
[fallback]
on_temp_exceed = "restore-factory"

# ---- game.toml ----
[profile]
name = "game"
priority = 90
[goal]
ec_mode = 2
pl1_w = 25
pl2_w = 45
epp = "performance"
fan_mode = "performance"
panel_hz = 120
[[trigger]]
type = "process_match"
names = ["cs2.exe", "eldenring.exe"]   # 用户自建名单,替代官方云端白名单
[constraints]
min_dwell_s = 30

# ---- long-battery.toml ----
[profile]
name = "long-battery"
priority = 50
[goal]
ec_mode = 1
pl1_w = 12
pl2_w = 20
epp = "balance-power"
fan_mode = "smart"
panel_hz = 60
dgpu = "igp-priority"
charge_mode = "conservation"
[[trigger]]
type = "on_battery"
[[trigger]]
type = "battery_below"
percent = 30
[fallback]
on_temp_exceed = "silent-library"
```

### 2.4 DSL 字段 → 参数映射(07 文档对照)

| DSL 键 | TuningTarget | 通道(Win / Linux) |
|---|---|---|
| `pl1_w` / `pl2_w` / `tau_s` | RAPL 功耗墙 | DTT TPWR 或 MSR 0x610 / powercap constraint_0/1 |
| `epp` | HWP 提示 | Power GUID ENERGY_PERFORMANCE_PREFERENCE / pstate EPP |
| `turbo` | 睿频开关 | 处理器最大状态 99% 技巧 / `no_turbo` |
| `ec_mode` | EC 模式字节 | AcpiVpc(03 §2)/ ideapad platform_profile |
| `fan_mode` | 风扇模式 | LENOVO_GAMEZONE_DATA / ideapad fan_mode |
| `panel_hz` | 刷新率 | 04 §3.1 / DRM mode |
| `dgpu` | DGPU 模式 | Dispatcher 服务消息 / switcheroo[推断] |
| `charge_mode` | 充电模式 | GBMD(02 §3)/ conservation_mode |
| `backlight` | 键盘背光 | LIGHTING_METHOD / sysfs led |
| `background.workset_trim/priority` | 进程调度 | §08 perf boost/throttle 原语 / cgroup+renice |

## 3. profile 文件管理

- 查找路径:`profiles.d` 目录(系统级/用户级,同 §1.1)+ 内置 5 套(07 §5)。
- `lctrl tune profile list` 合并展示并标来源(builtin/system/user);
- 同名覆盖:用户 > 系统 > 内置;
- schema 版本:`schema = 1` 顶层键,未知版本拒绝解析。

## 4. daemon 运行时状态 `state.json`

路径:Windows `%ProgramData%\lctrl\state.json`;Linux `/run/lctrl/state.json`(tmpfs,易失)。

```json
{
  "pid": 1234,
  "started_at": "2026-08-27T10:00:00+08:00",
  "active_profile": "long-battery",
  "active_trigger": {"type": "on_battery"},
  "reapply_count": 0,
  "saved_before_apply": {
    "pl1_w": 15, "pl2_w": 30, "tau_s": 28, "epp": "balance-performance",
    "ec_mode": 0, "fan_mode": "smart"
  },
  "last_events": [
    {"ts": "...", "kind": "ac_unplugged"},
    {"ts": "...", "kind": "profile_applied", "name": "long-battery"}
  ]
}
```

语义:`saved_before_apply` 在首次施加 profile 前采样,供 `tune restore`;
`last_events` 环形缓冲(≤64 条);文件损坏时 daemon 丢弃重建(易失状态,无恢复义务)。

## 5. 兼容性说明

- 官方 PCManager 的 `cfg.data`(AES 加密)、XML 阈值文件、LiteDB/SQLite ML 库**均不兼容也不读取**;
  本工具配置是唯一事实源(净室边界,charter §2)。
- 与官方共存的互操作键(可选):`battery.charge_mode` 写入时同步官方注册表键
  `HKCU\SOFTWARE\Lenovo\VantageService\AddinData\IdeaNotebookAddin\BatteryChargeMode`
  (02 §3.4),便于卸载本工具后官方软件读回一致状态。默认关闭:`[compat] write_vantage_registry = false`。

## 6. 证据

| 结论 | 证据源 |
|---|---|
| 目标-约束-触发-回退模型与优先级 | `docs/07-pantherlake-tuning.md` §6(本文件为其文件化) |
| 防抖 debounce 对标 | `电脑管家电源组件内部接口说明(官方防抖行为) |
| 官方阈值 XML/cfg.data 不兼容决定 | `电脑管家电源组件内部接口说明 |
| 官方注册表互操作键 | `Vantage 电源组件内部接口说明(HKCU AddinData 键) |
| 进程白名单触发对标 Dispatcher | `电脑管家电源组件内部接口说明/§4.2(游戏白名单+ML 映射) |
| 官方 ML 场景识别(被 DSL 规则替代) | `电脑管家电源组件内部接口说明、`Vantage SmartPerformance 组件内部接口说明 |
