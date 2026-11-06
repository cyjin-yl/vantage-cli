# 04 · 外设控制 — 功能与接口规范

> 读者:实现者。覆盖:键盘(背光/Fn 键/Fn-Ctrl 互换/Win 锁)、触控板、面板(刷新率/色彩/超分/OD)、
> 摄像头/麦克风/指纹隐私开关、智能感知(人机感应)、音频(Dolby/智能降噪)。
> 交叉引用:通道细节见 `01-hal-interfaces.md`;BIOS 设置族见 `05-bios-settings.md`;
> Linux 映射见 `09-linux-backend.md`;WMI 签名索引见 `A-wmi-reference.md`。

## 0. 一页速查

| 功能 | 通道 | CLI |
|---|---|---|
| 键盘背光档位/灯效 | ACPI `KBLC`(VPC2004,经 `\\.\EnergyDrv`);RGB 机型回退 WMI `LENOVO_LIGHTING_METHOD` | `kbd backlight {off\|low\|high\|auto}`(原始档位 `0..3` 仍接受) |
| Fn/Ctrl 互换 | BIOS `FoolProofFnCtrl` | `kbd fn-ctrl-swap {on|off}` |
| F1-F12 主功能 | BIOS `HotkeyMode` + `F1-F12AsPrimaryFunction` | `kbd fnlock {on|off}` |
| Win 键锁定 | WMI `LENOVO_GAMEZONE_DATA.SetWinKeyStatus` | `kbd winlock {on|off}` |
| 触控板开关/能力 | GAMEZONE WMI;21VG 使用 Windows PnP 输入叶节点 + HID 能力描述符 | `touchpad {status\|caps\|on\|off\|recover}`（`enable`/`disable` 为 `on`/`off` 的别名） |
| 面板刷新率 | WMI `LENOVO_INTERNAL_PANEL_REFRESH_RATE_DATA` + ChangeDisplaySettingsEx | `panel rate {60|120|auto}` |
| 面板身份(EDID) | `root\wmi` `WmiMonitorConnectionParams` 选中内置屏 + `HKLM\SYSTEM\CurrentControlSet\Enum\DISPLAY\...\Device Parameters\EDID` | `panel info` |
| 内置屏亮度 | Windows 公开 WMI 亮度及连接类 | `panel brightness [0..100]` |
| 面板色彩/显示模式 | WMI `LENOVO_PANEL_METHOD` | `panel color MODE` |
| 超分辨率(SR) | 注册表 + SmartEngine | `panel super-resolution {on|off}` |
| Display OverDrive | BiosOD.dll → 注册表/WMI | `panel overdrive {on|off}` |
| 摄像头开关 | BIOS `IntegratedCamera`;运行时为 PnP devnode 启停(`CM_Enable/Disable_DevNode`) | `privacy cam {status|on|off}`(`enable`/`disable` 为别名) |
| 摄像头使用/同意记录(只读) | 注册表 `CapabilityAccessManager\ConsentStore\webcam`(HKCU/HKLM)+ 策略 `Policies\…\AppPrivacy` | `privacy camera inuse`(`in-use` 为别名) |
| 麦克风开关 | BIOS `Microphone` | `privacy mic {on|off}` |
| 指纹使能 | BIOS `FingerprintReader` | `privacy fingerprint {on|off}` |
| 人机感应(亮屏/锁屏/距离) | SmartSense RPC + `LENOVO_SETTING_*` 命令 | `sense {lock-on-leave,wake-on-approach,...}` |
| Dolby 音效 | Dolby RPC | `audio dolby {off|movie|music|voice|game|dynamic}` |
| 智能降噪 | 麦克风阵列 DSP / 软件管道 | `audio noise-cancel {off|single|shared|spatial|voice-id|farfield}` |

---

## 1. 键盘

### 1.1 Fn/Ctrl 互换(BIOS 项 `FoolProofFnCtrl`)

- 通道:`Lenovo_SetBiosSetting.SetBiosSetting("FoolProofFnCtrl,Enable"|"…,Disable")`,
  随后 `Lenovo_SaveBiosSettings.SaveBiosSettings("")` 提交;**重启生效**。
- 回读:`Lenovo_BiosSetting` 枚举 `CurrentSetting` 以 `FoolProofFnCtrl,` 开头的实例(目标机实例 `ISET_18`)。
- 返回值:`"Success"` 或错误描述串(完整约定见 05 文档)。

### 1.2 F1-F12 主功能 / FnLock(`HotkeyMode` + `F1-F12AsPrimaryFunction`)

两条独立 BIOS 项(目标机实例 `ISET_17` / `ISET_51`),组合决定键行为:

| HotkeyMode | F1-F12AsPrimary | 行为 |
|---|---|---|
| Enable | Disable | 直接按 = 多媒体;Fn+Fx = F 键(出厂默认) |
| Disable | Enable | 直接按 = 标准 F1-F12;Fn+Fx = 多媒体 |

- 写:`SetBiosSetting("HotkeyMode,…")` / `SetBiosSetting("F1-F12AsPrimaryFunction,…")` + Save,重启生效。
- 另有 `FnAndCtrlKeySwap` 项与 §1.1 同族(部分机型两名称等价,以 `Lenovo_GetBiosSelections` 枚举为准)。

### 1.3 Win 键锁定(GAMEZONE)

```
前置: LENOVO_GAMEZONE_DATA.IsSupportDisableWinKey(out Data) → Data≠0 支持
读:   GetWinKeyStatus(out Data: UInt32)   0=解锁 1=锁定
写:   SetWinKeyStatus(Data: UInt32) → Boolean
事件: LENOVO_GAMEZONE_KEYLOCK_STATUS_EVENT (KeyLockState: UInt32)
```

实例:`ACPI\PNP0C14\GMZN_0`。**即时生效**,无需重启。

### 1.4 触控板开关

```
前置: IsSupportDisableTP(out Data)
读:   GetTPStatus(out Data: UInt32)   0=启用 1=禁用
写:   SetTPStatus(Data: UInt32) → Boolean
```

即时生效。部分机型触控板状态复用 `LENOVO_GAMEZONE_KEYLOCK_STATUS_EVENT` 推送(按位解析)[推断]。

21VG 的 Windows 实现不依赖未实现的 GAMEZONE 方法,通过公开的
`CM_Disable_DevNode` / `CM_Enable_DevNode` 控制输入叶节点:

- 触控板必须由 HID digitizer/touchpad usage `HID_DEVICE_UP:000D_U:0005`
  和集成 ACPI 父节点共同识别;设备名称、`PNP0C50` 或 ACPI 父节点单独存在都不足以授权写入。
- 仅同时切换该触控板的 digitizer 叶节点及同父节点的 mouse usage
  `HID_DEVICE_UP:0001_U:0002` 叶节点。**绝不禁用 ACPI 控制器**,
  也不切换配置/vendor-defined 集合或其他鼠标。
- dry-run 不写设备;已处于请求状态时不重启输入节点。提交逐节点回读,
  失败时恢复所有可能已发生变化的尝试节点,包括返回错误但可能已经生效的那一次调用。
- `touchpad status` 只读输入叶聚合状态；叶状态分裂或不可读时返回错误，不伪装成单一状态。
- `touchpad caps` 只读精确式触控板顶层集合（`HID_DEVICE_UP:000D_U:0005`）的 HID
  preparsed data：集合声明的同时接触点数、设备能力 feature report 中的
  Contact Count Maximum（`0x000D/0x0055`）与 Button Type（`0x000D/0x0059`）、
  X/Y 的逻辑/物理量程及 unit/exponent、逐接触点声明的 Tip/Confidence/Pressure、
  Haptics 强度与按键压力阈值。该集合只能在 `FILE_READ_ATTRIBUTES` + 共享
  `READ|WRITE` 下打开（`GENERIC_READ` 在本机返回共享冲突 32）；打开失败即报
  `ChannelUnavailable`，不回退到更宽的访问掩码。非 `0x000D/0x0005` 顶层集合
  （配置集合 `0x000D/0x000E`、厂商集合 `0xFF00/0x0001`）一律拒绝解码。
  输出中的毫米尺寸由声明的物理量程换算并标注为 derived；Contact Count Maximum
  与 Button Type 为设备声明值，不构成按压/震动功能保证。详见
  `evidence/re-touchpad-caps-20261105.md`。
- `touchpad recover` 先读取全部输入叶并要求至少一个 mouse 叶（输入叶总数上限 16，
  超限拒绝），再按观察到的叶状态选择策略（`strategy` 字段如实回报，不由调用方指定）：
  (a) `enable_disabled_leaves` — 崩溃后 digitizer/mouse 叶状态分裂时，只启用当前禁用的
  输入叶；(b) `cycle_input_leaves` — 全部叶已启用但触控板无输入响应时，对每个输入叶执行
  `CM_Disable_DevNode`→`CM_Enable_DevNode` 重启（禁用始终为运行时语义，不置持久标志），
  并先确认禁用已实际生效再启用。两条策略都以逐叶独立回读结尾（恢复窗口长于普通切换），
  回读失败的叶会再显式启用一次并重读，绝不留在禁用状态；最终失败为非零退出并列出叶，
  可再次执行。全程不触碰 ACPI 控制器、configuration 集合或 vendor-defined 集合。
  `--dry-run touchpad recover` 只输出策略与各叶前值/目标，不写设备。
  对健康触控板执行 `recover` 会重启其两个输入集合，这是该命令的语义。
  PnP 回读只证明节点已枚举为启用，不证明按压/震动功能；厂商 firmware-tunnel 集合
  （Col04）按上述边界不参与重启，因此 MCU 级挂死可能仍需整机断电重启 [推断]。
  详见 `evidence/re-touchpad-20261105.md`。新路径的目标机真实恢复效果尚未验收。
- 禁用 ACPI 控制器会令 HID 子节点消失,使下一次 CLI 无法重新发现触控板。
  若旧版本留下该状态,用已确认的原控制器完整 instance ID 执行
  `pnputil /enable-device "<instance-id>"`,再检查控制器及各子节点状态;不要扫描或批量启用其他设备。

2026-10-27 的恢复、用户实测和非中断式验证见 `C-acceptance.md` C7。
新版叶节点禁用/恢复的真机完整验收尚未执行,不能把回归测试当成完整硬件验收。

### 1.5 键盘特性位图

`LENOVO_GAMEZONE_DATA.GetKeyboardfeaturelist(out Data: UInt32)` — 按位 OR 的能力掩码:
bit0=Fn/Ctrl swap,bit1=F1-F12 primary,bit2=Win 锁,bit3=触控板禁用,bit4+=背光/G-Sync/OD 等。
能力探测一律先调它。

---

## 2. 键盘背光

### 2.0 首选通道:ACPI `KBLC`(VPC2004,`\\.\EnergyDrv` IOCTL `0x83102144`)

21VG 及同类 IdeaPad/ThinkBook 的键盘背光由虚拟电源控制器设备的 `KBLC` 方法控制,
`AcpiVpc.sys` 把 4 字节参数与 4 字节结果透传给用户态。参数编码(与 Linux
`ideapad-laptop` 的 `KBD_BL_*` 常量一致):

```
0x1                                  查询键盘组 → KBGC|1(5=三态,7=三态+auto)
(group << 4) | 0x2                   读状态     → KBGS|1,状态 = (ret >> 1) & 7
(state << 16) | (group << 4) | 0x3   写状态     → 不带状态字,必须回读
```

`group = (query & ~1) >> 1`(三态=2,三态+auto=3);状态 `0`=关、`1`=低、`2`=高、
`3`=自动(环境光)。组不匹配时固件回 **0**,因此 0 只能当"无状态",不能当"关"。
`kbd backlight` 的 `off|low|high|auto` 即 `0|1|2|3`;`auto` 只在查询返回 `7`
的机型上被接受,其余机型由 `level exceeds max` 拒绝。

**21VG 实机只读测量(2026-11-05)**:`KBLC(0x1)=0x7`、`KBLC(0x32)=0x1`、
`KBLC(0x22)=0x0`;两字节回读与主契约一致,写路径尚未执行。取证与驱动偏移见
[interfaces.md §11.8](interfaces.md#118-键盘背光acpi-kblcvpc2004) 和
[re-kbd-backlight-20261105.md](evidence/re-kbd-backlight-20261105.md)。

### 2.1 回退通道:`LENOVO_LIGHTING_METHOD`(实例 `ACPI\PNP0C14\LLT_0` 族)

```
读: Get_Lighting_Current_Status(Lighting_ID: UInt8,
        out Current_Brightness_Level: UInt8, out Current_State_Type: UInt8) → Boolean
写: Set_Lighting_Current_Status(Current_Brightness_Level: UInt8,
        Current_State_Type: UInt8, Lighting_ID: UInt8) → Boolean
```

`LENOVO_LIGHTING_DATA` 静态属性:`Lighting_Id`(键盘=0)、`Lighting_Type`
(0=无/1=单色/2=RGB/3=RGB 多区 [推断])、`State_Type_Num`(亮度档位数)、
`Default_Brightness_Level`、`Brightness_Level`(当前)。

**目标机实测(2026-10-30)**:TB14P-EZRA 上 `LENOVO_LIGHTING_DATA` 与
`LENOVO_LIGHTING_METHOD` 实例枚举均返回 WMI HRESULT 0x8004100C(Not
supported),该机型无此 WMI 背光通道;`kbd.backlight` 的可用性因此改由
§2.0 的 `KBLC` 探测决定。GAMEZONE 复合通道同样未在目标机验证,不作为替代。

档位:`0`=关,`1..State_Type_Num-1`=逐级亮度(ThinkBook 单色背光为 3 档)。
灯效 `Current_State_Type`:0=静态,1=呼吸,2=波浪,3=反应,4=闪烁(单色机型仅 0 有效)。

### 2.2 传统通道:`LENOVO_GAMEZONE_DATA.SetKeyboardLight`(复合打包参数)

```
IsSupportLightingFeature(out Data)
SetKeyboardLight(Data: UInt32) / GetKeyboardLight(out Data: UInt32)
SetLightControlOwner(Data)   # 抢占控制权(多控制者仲裁)
```

`Data` 位布局:bit0-3=亮度档位,bit4-7=State_Type,bit8-15=Lighting_ID。

### 2.3 事件与自动行为

- `LENOVO_LIGHTING_EVENT`(Key_ID: UInt8)— 用户按 Fn+背光键时 EC 推送,`watch` 用。
- `LENOVO_GAMEZONE_LIGHT_PROFILE_CHANGE_EVENT`(EventId: UInt32)— 背光配置切换。
- 自动熄灯:官方在电池模式下由 IdleTimer 调 0 档;实现者可在 daemon 中复刻(见 10 文档触发器)。

---

## 3. 面板(显示屏)

### 3.0 内置屏亮度

`panel brightness` 只读返回当前百分比和实际公布的可选档位;
`panel brightness N` 设置一个已公布的档位，`--dry-run` 只读并返回前值和请求，不调用 setter。
可附加 `--expect-current M`，若新观察的当前值不是 M 则在写入前拒绝;该检查不是跨进程原子 CAS。
Windows 使用公开 `WmiMonitorBrightness`、`WmiMonitorConnectionParams` 和
`WmiMonitorBrightnessMethods.WmiSetBrightness`：按非空 `InstanceName` 精确关联唯一活动内置屏，
不暴露显示器身份，不把外接或多个候选屏当成目标。设置前重新核对身份/当前值;写后轮询
实际亮度。失败时仅在同一屏仍为本次请求值时尝试恢复前值，观察到第三值或无法确认身份
时不覆盖并报告状态未验证。Microsoft 网页将 `ReturnValue` 描述为 UInt32/0 成功;目标
WMI 类元数据报告 Boolean，而方法签名未提供输出参数对象。若传输成功但没有输出属性，
只能继续等待同一屏的实际读回，不能把空输出直接当作设备成功;未知布尔语义不猜测。

Windows 全局命名互斥量只串行化合作的本程序写入，不能阻止厂商应用、热键或用户设置
并发修改;检查与写入不是原子操作，WMI 单次调用也可能阻塞。命名互斥量不是持久崩溃
恢复日志。目标机 1% 实际写入、独立 WMI 读回、正常恢复及合成故障恢复的有限证据和
未验收事项见 C17;当前亮度功能不等于完整 `power saver-once` 事务。

### 3.1 刷新率

**当前交付边界**:Windows 固定刷新率请求只选择与当前内置屏布局一致的已枚举精确
模式,`--dry-run` 使用公开 `CDS_TEST`,提交使用动态显示模式而不修改用户配置中的
持久模式。活动内部路径必须唯一且不与外接输出共用显示源;提交前重新核对路径和
当前模式。失败恢复时,前值已在则不写,仅在仍观察到本次请求值时尝试恢复前值;
其他模式或不同路径保留并报告未验证。检查和写入不是原子操作,不能据此保证消除
外部控制者竞争或证明进程崩溃恢复。`auto` 当前只是选择固件默认的固定模式,不是
已验收的动态 AC/会话自适应策略。

`--auto-restore` 只回放**观测到的前一固定刷新率**:固件刷新 `Mode`(自适应 VRR)
没有写入通道,任何请求都无法把自适应策略恢复回去;`auto` 解析为固件默认频率而
不是观测值,因此不能用作恢复目标(2026-11-05 实机缺陷与修正见
`evidence/re-panel-refresh-20261105.md` §4)。

目标机在交互控制台对 120→60 Hz 执行真实程序 dry-run,退出 0;独立显示 API 观察
同一内置路径的活动及注册表模式在前后保持不变。该观察不证明物理刷新率切换或
实际写入恢复。140 项源输入经原生构建前后逐字节核对,原生 24 项库测试和 21 项
模拟显示测试通过;完整脱敏回执与边界见
`evidence/target-panel-refresh-dryrun-20261029.json`。#22 的真实切换、自适应行为和
完整替代仍未验收。

目标机随后在交互控制台完成一次真实 120→60 Hz 提交:退出 0,独立 Win32 快照
证实同一显示路径的活动模式只有 refresh 字段变化(120→60),分辨率、位深、
旋转、偏移和注册表模式不变;随后守卫式恢复执行 60→120 Hz,独立快照证实
回到 120 Hz 且同样仅 refresh 变化。全程不修改注册表持久模式,官方服务保持
运行。脱敏回执见 `evidence/target-panel-refresh-commit-20261030.json`;
多显示器拓扑、外部控制者并发竞争和进程崩溃中途恢复仍未验收,#22 的自适应
行为与完整替代仍未验收。

`LENOVO_INTERNAL_PANEL_REFRESH_RATE_DATA`(实例 `GMZN_0..GMZN_11`,每 GPU/输出一路,
`Active=true` 为主屏):

| 属性 | 类型 | 目标机实测值 |
|---|---|---|
| InternalPanelHwID | UInt32 | 0x8A000A00(联想内部型号) |
| MinimumRefreshRate / MaximumRefreshRate | UInt16 | 60 / 120 |
| DefaultRefreshRate | UInt16 | 60 |
| Mode | UInt16 | 0=手动固定,1=自适应 VRR,2=性能优先(恒高刷) |

- 历史记录提及 BIOS/WMI 候选,不是目标机写入准入证明;不扫描或猜测其控制 ID。
- 当前固定模式使用公开 Windows `ChangeDisplaySettingsExW`,不调用厂商自有写入方法。
- 事件:`LENOVO_REPORT_REFRESH_RATE_EVENT`(MaxRefreshRate/MinRefreshRate: UInt16)。

### 3.2 色彩与显示模式(`LENOVO_PANEL_METHOD`,实例 `GMZN_0`)

```
Panel_Get_Support_Status(out Support_Status: UInt32)
  bit0=PIP bit1=LowLatency bit2=GameAid bit3=MPRT bit4=Gamut bit5=GameAidFPS
Panel_Get/Set_Status(Status: UInt32)                    # 全局开关 bit0
Panel_Get/Set_Display_Mode(mode: UInt32)                # 0=sRGB 1=DCI-P3 2=Adobe 3=自定义 4=电影
Panel_Get/Set_Gamut_Switch(mode: UInt32)                # 0=sRGB~65% 1=DCI-P3~100%
Panel_Get/Set_Low_Latency_Mode(mode)                    # 0=关 1=中 2=高
Panel_Get/Set_PIP_Info(PosX,PosY,SizeX,SizeY)           # 画中画,像素
Panel_Get/Set_MPRT(PosX,PosY,SizeX,SizeY)               # 响应时间增强区域
Panel_Get/Set_Game_Aid_* (FPS/Sight_Mode/Timer/Countdown)
```

### 3.3 Tcon / SmartColor / OverDrive(SmartPanelAddin 通道)

| 功能 | 通道 | 取值 |
|---|---|---|
| 色域模式 | TconSDK.dll(GUI 5 模式 → Tcon 索引映射 `[0,2,3,1,0]`) | sRGB/Native/DCI-P3/… |
| 阴影增强 ShadowBoost | TconSDK SetShadowBoost | on/off |
| DynamicOD | TconSDK SetDynamicOD | on/off |
| BeyondVision | TconSDK SetBeyondVision | on/off |
| BIOS Display OverDrive | BiosOD.dll(注册表/WMI) | on/off |
| X-Rite 色彩 profile | SmartColorAddin(云 profile + `HKCU\SOFTWARE\Lenovo\SmartDisplayAddin\Color\user_current_scenery`) | profile 名 |

> TconSDK/BiosOD 为 Windows 厂商闭源组件。本实现:**Windows 侧优先使用 §3.2 的 WMI
> PANEL_METHOD**(语义等价);Linux 侧见 09 文档 §6。X-Rite 云 profile 生态封闭,标注为可选,
> 建议以本地 ICC 配置替代。

### 3.4 超分辨率(SR,V2001)

- 开关:`HKLM\SOFTWARE\Lenovo\SmartEngine\ModuleSettings\SR\SuperResolutionStatus`;
  SRMapping:0/1/3/4→2(开),2→1(关)。
- 与场景引擎联动(DispatcherConfig.xml V2001)。**Windows 厂商特性**,实现边界标注为可选。

**2026-11-05 实机复核(21VG,只读)**:原版链路是 `SmartDisplayAddin →
x64\SESuperResolutionPlugin64.dll → IntelControlLib.dll`(IGCL),场景值为
`set_sr {"F2001":{"F2001":0|1}}`。IGCL 契约:能力枚举两趟(`pFeatureDetails=NULL` 取数量 →
传数组 → 单特性取 116B 负载),get/set 为 120B `ctl_video_processing_feature_getset_t` 配
92B payload(flag@+8:1=关、2=开)。本机驱动对枚举与状态查询均回 `0x4000000b`,故保持
Unsupported;实现与验收见 `docs/evidence/re-panel-20261105.md` §4。

### 3.5 护眼(低蓝光)

- 路径一:BIOS 项 `LowBlueLight,Enable`(若 BiosSelections 枚举存在)。
- 路径二:`Panel_Set_Display_Mode` 的护眼档位(0=关,1=中,2=高)[推断,以 Support_Status 实测为准]。
- 路径三(PCManager 护眼):向 `LenovoPcManagerService` 发 SCM 自定义消息
  `SERVICE_CONTROL_CUSTOM_MESSAGE_OPENEYEGUARDIANS` / `_CLOSEEYEGUARDIANS`
  (承载者 `LenovoMonitorManager.exe`,显示器颜色过滤;证据 `电脑管家组件内部结构说明/§9.4)。
- SmartInteractAddin 的 `EyeCareMode` 属同族封装。
- **实现建议**:路径一/二为 Lenovo 平台通用;路径三仅在安装了 PCManager 服务时可用,
  本工具应以路径一/二为主、路径三为兼容回退。

**2026-11-05 实机复核(21VG,只读)**:路径一不存在(53 条 `Lenovo_BiosSetting` 无
`LowBlueLight`);路径二不存在(`LENOVO_PANEL_METHOD.Panel_Set_Display_Mode` 回 `不支持`);
路径三存在,但承载者不是 SCM 自定义消息,而是 `LenovoMonitorManager.exe`(x86,`-pcmgr
Eye_Protection_Mode` / `-seteyeprotected`;支持位来自 `WrapPlugin.dll!IsSupportColorTemperature`,
状态 `HKCU\SOFTWARE\Lenovo\PcManager\Configs\EyeCareFlag`,实测 `0`)。Vantage 侧为
`SmartInteractAddin` 契约 `EyeCareMode`(`EyeModeState`/`ColorTemperature`/`SetEyeMode` +
`UpdateEachMonitorGammaTemp`)。两者机制相同:系统 `GetDeviceGammaRamp`/`SetDeviceGammaRamp`
施加色温曲线,所以 lctrl-hal-win 用同一系统 API 实现(`GammaRampApi`/`NativeGammaRamp`),
不依赖联想组件。档位 Kelvin 仍是仓库推断值;证据与验收步骤见
`docs/evidence/re-panel-20261105.md` §5。

### 3.6 面板身份(EDID,只读)

`panel info` 报出内置屏自身 EDID 解码出的身份:三字母厂商码、产品码(与厂商码拼成
PnP 显示器 ID,如 `LEN8AD7`)、`0xFC` 名称描述符、`0xFE` 文本描述符、序列号(`0`
表示未使用,不当作序列号 0)、制造年/周(周 `0` 为未指定,`0xFF` 为型号年)、EDID
版本、首选 DTD 的原生分辨率与刷新率、物理尺寸(`cm` 字节换算成 `mm`)、数字输入的
每主色位深与接口码、`0xFD` 范围限制。解码由本仓库实现(`lctrl-core::decode_panel_edid`),
不依赖厂商 DLL。

选屏与取字节分两步,且都必须唯一:先查 `root\wmi` `WmiMonitorConnectionParams`,取
`VideoOutputTechnology == D3DKMDT_VOT_INTERNAL`(`0x80000000`)的唯一实例;再按其
`InstanceName` 去掉尾部显示序号(`_0`)后,从
`HKLM\SYSTEM\CurrentControlSet\Enum\<instance>\Device Parameters\EDID` 读 `REG_BINARY`。
零个或多个内置实例一律拒绝,不挑一个凑答案;候选实例的 PnP 类名(3 字母 + 4 位十六
进制)若与 EDID 解出的厂商码/产品码不一致,同样拒绝。

EDID 校验是硬性要求:长度非 128 整数倍、任一 128 字节块校验和不为 0、基块魔数不符、
或基块声明的扩展块数与实际字节数不符,都整体拒绝而不做部分解码。内置屏与外接屏
记录共存、已拔出的显示器仍保留记录,所以"读第一条 `DISPLAY` 记录"是错误做法。

连接类型只报 EDID 与操作系统能证明的部分:数字接口码 `0x05`(DisplayPort)且被 OS
判为内置即 `edp`;外接 `DisplayPort`/`HDMI`/`DVI` 按接口码报;EDID 没有 LVDS 编码,
所以内置屏若接口码为 0(EDID 1.3 式数字字节),报 `unspecified` 并保留原始接口码,
不猜 LVDS。证据与实机回执见 `docs/evidence/re-panel-edid-20261105.md`。

---

## 4. 摄像头 / 麦克风 / 指纹(隐私)

### 4.1 硬开关(BIOS 层,重启生效)

| BIOS 项 | 目标机实例 | 语义 |
|---|---|---|
| `IntegratedCamera,Enable|Disable` | ISET_40 | 切断摄像头供电/总线 |
| `Microphone,Enable|Disable` | ISET_41 | 硬件级禁用麦克风 |
| `FingerprintReader,Enable|Disable` | ISET_42 | 指纹传感器使能 |
| `EnhancedWindowsBiometricSecurity,Enable` | ISET_29 | 增强 Windows Hello(强制 PIN 后备) |

通道:`Lenovo_SetBiosSetting` + `Lenovo_SaveBiosSettings`(05 文档)。

### 4.2 运行时开关(PrivacyGuard,即时生效)

Vantage 的 Privacy Guard 是独立运行时层:`SetPrivacyGuardEnabled(bool)` /
`LoadPrivacyOptionFromLocal` / `IsPrivacyOptSet` / `ResetPrivacyGuardSetting`
(Lenovo.CommonVantage.Shared.dll)。底层写 EC 寄存器或 `LENOVO_UTILITY_DATA.SetFeature` [推断]。

**实机更新(2026-08-27)**:`LENOVO_UTILITY_DATA` 方法为**实例方法**(必须
`Get-CimInstance | Invoke-CimMethod`,静态调用报"无效的方法参数");
`GetIfSupportOrVersion(datatype)` 返回版本号(0=不支持),本机实测 datatype 1→v3、3→v2、4→v2;
已知 datatype 10=DolbyAudio、18=PrecisionTouchpad(Data≥24 即支持)。
实现建议:`privacy cam|mic off --runtime` 用实例调用 `SetFeatureEx(IDs,Value,Ret)`
先以小范围 IDs 探测(`GetIfSupportOrVersion` 返回非 0 的 datatype 即合法特性域);
`--persistent` 走 §4.1 BIOS 层。

**目标机实测(2026-10-30)**:`privacy cam off|on --runtime` 已在本机完成真实
验收:dry-run 无副作用;提交后独立 `Get-PnpDevice -Class Camera` 观察到两个
USB 摄像头接口由 OK 变为 Error(禁用);恢复后回到 OK。`privacy status` 回读
runtime_camera=enabled、BIOS 三项均为 Enable。运行时麦克风布线未接入;
SmartSense 与 Dolby/降噪为保留的官方厂商通道(见 §5/§6 边界)。回执见
`evidence/target-input-privacy-sensing-20261030.json`。

**补充(2026-11-05)**:`privacy cam status` 只读地同时报告三层:聚合运行时状态、
每个 devnode 的独立状态(拆分组或启动失败的 devnode 按实例归因,不再整体拒绝)、
BIOS `IntegratedCamera` 持久项。`privacy camera enable|disable` 与 `on|off` 等价。
遮蔽(occlusion)通道保持关闭并如实报告 `supported:false`:
`Windows.Media.Devices.CameraOcclusionInfo` 只能从已初始化的 WinRT
`MediaCapture` 会话的 `VideoDeviceController` 取得——读取即打开摄像头、devnode
禁用时不可读;微软明确要求在流式期间才使用该值,且不得当作绝对隐私指示——而本机
recon 未发现物理快门硬件。契约与未跑验收见
`evidence/re-privacy-cam-impl-20261105.md`。

**补充(2026-11-06)**:`privacy camera inuse`(别名 `in-use`)只读地报告 Windows
Capability Access Manager 对摄像头**记录了**什么,不回答"现在是否在采集":
四个容器 `HKCU`、`HKCU\…\webcam\NonPackaged`、`HKLM`、`HKLM\…\NonPackaged`
各自独立报告(机器级 `Deny` 不与用户级 `Allow` 合并),每条应用记录给出
`Value`(`Allow`/`Deny`/`PromptOnFirstUse`,其他文本原样保留)、`LastSetTime`
与 `LastUsedTimeStart`/`LastUsedTimeStop`(FILETIME,同时给出原始 ticks 与 UTC),
`NonPackaged` 的 `#` 编码路径解码为真实路径(无法解码的键原样保留,不丢弃)。
`in_use` 的规则显式写在输出里:**两个时间戳都非零且 Start > Stop**;这是记录到的时间戳
对,不是直播证据,输出同时带 `as recorded by CapabilityAccessManager` 标注。
策略 `HKLM\SOFTWARE\Policies\Microsoft\Windows\AppPrivacy` 独立成字段:本机不存在
该键,因此报 `present:false`,而不是推断成"允许"或"用户控制"。整条路径只做
`KEY_READ`,不打开设备、不调用 `MediaCapture`、不写同意存储。契约、实机只读探测
与未跑验收见 `evidence/re-camera-inuse-20261106.md`。

### 4.3 指纹/人脸凭据

Lenovo 只控制硬件使能;凭据注册走 Windows Biometric Framework,不在本工具范围。

---

## 5. 智能感知(HPD / SmartSense)

硬件:`ACPI\IDEA2002`(IR 摄像头+传感器),Windows HPD 框架 + Lenovo SmartSense 服务。

### 5.1 当前 HPD RPC 与历史 `LENOVO_SETTING_*` 命令字

当前 `sense status` 与离开锁定/靠近唤醒/视频暂停及距离、等待设置走自有
`rpcrt4 → ncalrpc:[HumanPresenceDetectionEndpoint]` 客户端,不依赖 Lenovo 客户端 DLL。
wire opnums 分别为 11/8/16(布尔 0/1)、14/9(距离 0/1/2)、12(等待 0/1/2)。
状态用 RPC capability + aggregate getter 投影;写后用 HPD 注册表独立读回,既有 dry-run/
auto-restore 贯通同一接口。服务端权限不是「任意用户均可写」,验收使用提权 owner seat。
源码切换尚未在本次运行验收;[精确契约与原生 smoke 步骤](evidence/hpd-rpc-re-20261102.md)
区分静态实现、设置生效与真实离开/唤醒/媒体效果。

下表是旧 `ChangeSetting(uint command, string parameter)` 设置**命名管道**的恢复表,
不是 ACPI/EC 直调。只有独立的 privacy-alert / attention-tracking 仍使用其已恢复入口;
HPD 六项不在 RPC 失败后回退到此前返回 0 的管道:

| 命令 | 值 | 语义 |
|---|---|---|
| LENOVO_SETTING_ENABLE_SST | 65793 | 启停 Smart Sense Technology |
| LENOVO_SETTING_ENABLE_BLC | 131329 | 键盘背光(感应联动)启用 |
| LENOVO_SETTING_SET_BLC_AUTO | 196865 | 背光自动 |
| LENOVO_SETTING_ENABLE_MODE | 262401 | 感应模式 |
| LENOVO_SETTING_SET_BROWSING_TIME | 327937 | 浏览时间(秒) |
| LENOVO_SETTING_ENABLE_SCREEN_LOCK | 393473 | HPD 屏幕锁定 |
| LENOVO_SETTING_ENABLE_APPROACH | 459009 | 接近检测 |
| LENOVO_SETTING_SET_APPROACH_DISTANCE | 524545 | 接近距离档位(0/1/2) |
| LENOVO_SETTING_ENABLE_PRESENTLEAVE | 590081 | 存在/离开检测 |
| LENOVO_SETTING_SET_LEAVE_WAIT | 655617 | 离开等待档位(0/1/2) |
| LENOVO_SETTING_ENABLE_VIDEOSTOP | 852225 | 人走暂停视频 |
| LENOVO_SETTING_ENABLE_CAMERADETECT | 852481 | 摄像头检测 |
| LENOVO_SETTING_ENABLE_AUTOADJUST | 917761 | 自动调节 |
| LENOVO_SETTING_HPD_RESET_COMMAND | 983297 | HPD 重置 |
| LENOVO_SETTING_HPD_SET_GLOBAL | 1048833 | HPD 全局开关(2=关,3=开) |
| LENOVO_SETTING_SET_LEAVE_DISTANCE | 1114369 | 离开距离 |
| LENOVO_SETTING_ENABLE_CAMERAASSIST | 1179905 | 摄像头辅助 |
| LENOVO_SETTING_ENABLE_OVERRIDEOSTIMER | 1245441 | 覆盖 OS 屏保计时 |
| LENOVO_SETTING_ENABLE_LEAVE | 1310977 | 离开检测 |
| LENOVO_SETTING_SET_LEAVE_TIMER | 1376513 | 离开计时(秒) |
| LENOVO_SETTING_ENABLE_ATTENTION_TRACKING | 1442049 | 注意力追踪 |
| LENOVO_SETTING_ENABLE_ATTENTION_TRACKING_TIMER | 1507585 | 注意力追踪计时 |
| LENOVO_SETTING_ENABLE_ATTENTION_TRACKING_AC | 1573121 | AC 下才启用注意力追踪 |
| LENOVO_SETTING_ENABLE_ATTENTION_TRACKING_DIM | 1638657 | 注意力不集中的暗屏 |
| LENOVO_SETTING_ENABLE_FACE_SENSING | 1704193 | 面部识别 |
| LENOVO_SETTING_ENABLE_APPROACH_EXTDISP | 1835265 | 外接显示器接近 |
| LENOVO_SETTING_ENABLE_LEAVE_EXTDISP | 1900801 | 外接显示器离开 |
| LENOVO_SETTING_ENABLE_AD_EXTDISP | 1966337 | 外接显示器自动调节 |

感应模式枚举:Browsing=2,Facedown=3,Walking=5。

### 5.2 状态机与配置面

```
[Away] --IR 检测到人脸--> [Present] --锁屏计时未到--> 亮屏/解锁
[Present] --离开 + 超距离阈值--> [Away] --执行离开动作(锁屏/暂停视频/提示)
```

- 距离档位:0=Near、1=Middle、2=Far;没有恢复出可声称的固定厘米值。
- 等待档位:0=Short、1=Middle、2=Long;不是任意秒数。
- 离开动作(PrivacyGuardActionType):0=锁屏+暂停视频,1=仅锁屏,2=提示。
- 持久化注册表:`HKLM\SYSTEM\CurrentControlSet\Services\SmartSense\Parameters\{ScreenLock, AutomaticKeyboardLight, HPD}`;`LPlatSvc\Parameters\AutomaticKeyboardLight`。
- RPC 设置通知通过现有 binding 上的 callback op0 传递;当前自有设置客户端不注册回调,
  不声称提供 live presence stream。`LENOVO_AI_SCENARIO_TYPE_EVENT` 为另一场景事件面(见 03/07 文档)。

---

## 6. 音频

### 6.1 Dolby

RPC 客户端族(`DolbyRpcClient.dll` 原生 / `Lenovo.Vantage.DolbyRpcClient.dll` 托管)暴露:

> 实现说明(2026-11-01):联想交付的客户端 DLL 已被裁定为不可作为运行时依赖,
> `audio dolby` 改为纯 Rust rpcrt4/NDR 客户端直打同一端点
> (契约见 docs/interfaces.md §12),下表描述的是原厂接口面,不是当前依赖。

| 接口 | 语义 |
|---|---|
| GetDolbyEnabled / SetDolbyEnabled | 主开关 |
| GetDolbyHSASupported / GetDolbyFusionSupported | 能力(HSA=硬件空间音频) |
| GetDolbyProfile / SetDolbyProfile | 场景配置 |
| GetDolbyState / SetDolbyState | 精细状态 |
| DolbyEffectChangeEvent | 变更事件 |

Profile 映射(SmartDisplayAddin `_dolbyMap`,代码级):Movie=0,Music=1,Games=2,Voip=3,
Personalize=4,Dynamic=5,Off=6。

> Dolby 栈是闭源 DSP 插件;本实现仅能开关/切 profile(经其 RPC 或音频端点属性),
> 不可重实现其算法。Linux 无对应物(09 文档)。

### 6.2 智能降噪(Dolby Whisper / Elevoc)

- 契约命令:GetNoiseCancelledAbility / GetNoiseCancelledStatus / SetMicrophoneStatus /
  SetSpeakerStatus / SetMeetingStatus / SceneEvent。
- 模式值(代码级):Off=0,Shared=1,Single=2,Spatial=3,VoiceID=4,Farfield=10。
- 双路径判定:**驱动侧**(FMAPOCTLAPI.dll → 麦克风阵列 DSP 硬件降噪)优先;
  **软件侧**(WASAPI Loopback + Elevoc/Aispeech SDK)兜底。
- Linux 替代:PipeWire + RNNoise 链(09 文档 §6)。

---

## 7. Fn 热键体系(参考行为,默认由固件/服务处理)

官方栈:`LenovoFnAndFunctionKeys` 服务(ACPI\LHK2019,无内核驱动,用户态服务)
+ `FnHotkeyUtility.exe`(OSD)。**本实现无需接管热键本身**(EC 固件直接产出 HID/ACPI 事件),
只需提供:① OSD 等价物(daemon);② 各热键动作的可编程绑定。

能力探测:`LENOVO_UTILITY_DATA.GetIfSupportOrVersion(datatype, out Data)`,
目标机实测(目标机 WMI 实例实机采集 L94-131):datatype 1→3(背光能力),3→2(亮度),
4→2(麦克风静音),18/19/20→24/25/25,26→27,29→32,38→36(特殊键)。

事件源:`LENOVO_UTILITY_EVENT`(PressTypeDataVal: UInt32)— 按键事件;
OSD 时序:接收事件 → 执行动作 → OSD 显示 0.8s + 淡出 0.3s → 通知 UI 同步。

### 7.1 Fn OSD 开关(`vantage osd enable|disable`)

Fn OSD(CapsLk/NumLk 指示)的持久偏好不属于 WMI 类,而是 `LenovoFnAndFunctionKeys`
服务键下的两个 DWORD,且**唯一的厂商写入口是用户自定义服务控制码**
(`ControlService`,不是直接写注册表):

| 指示 | 开 | 关 | 服务写入的值 |
|---|---|---|---|
| CapsLk OSD | `146`(0x92) | `147`(0x93) | `VantageToast\ShowCapslkOSD`(并同步 `ControlShowOSD\ShowCapsNumlkOSD`) |
| NumLk OSD | `148`(0x94) | `149`(0x95) | `VantageToast\ShowNumlkOSD` |

读回 = 读上述 DWORD(`1` = 显示);厂商自己也是在控制码之后按 10×200ms 轮询该值确认。
NumLk 额外受 `Capbility\SupportNumlkKey=1` 约束,为 `0` 的机型(21VG)如实报
`not_supported` 且**不发送** 148/149。`144`/`145` 属于 Vantage toast(`NeverPopToast`),
不是 Fn OSD,不在本命令范围。取证见 `evidence/re-osd-20261105.md`。

---

## 8. 错误与边界

1. WMI 方法返回 `false` = 固件拒绝(不支持/参数越界);读取能力位后再暴露 CLI 选项。
2. BIOS 项写入后**必须 Save 才生效且需重启**;CLI 应提示 `--apply-now`(支持运行时的项)vs 重启。
3. `SetKeyboardLight` 与 LIGHTING_METHOD 并存时以「控制者仲裁」(`SetLightControlOwner`)为准;
   建议实现统一只用 LIGHTING_METHOD。
4. PrivacyGuard 运行时层与 BIOS 层可能不一致:CLI `privacy status` 应同时展示两层。
5. 智能感知依赖 IR 摄像头:摄像头被 BIOS 禁用时 sense 子命令整体不可用(先报依赖错误)。

---

## 9. 证据

| 结论 | 证据源 |
|---|---|
| GAMEZONE/UTILITY/LIGHTING/PANEL/BIOS_ASSISTANT/SR_DATA 全部方法签名 | `目标机 WMI 仓库实机采集` 对应类;`官方组件接口说明§5 |
| BIOS 项实例映射(ISET_17/18/29/40/41/42/51) | `目标机 WMI 实例实机采集` L17-L52 |
| 刷新率 60/120/Mode 值、CAPABILITY_DATA IDs 0xA000001-4 | `目标机 WMI 实例实机采集`;`官方组件接口说明 |
| PANEL_METHOD 方法族与位域 | `官方组件接口说明 |
| TconSDK/BiosOD/ACM 通道与 GamutType 映射 | `官方组件接口说明 |
| SR 注册表与 SRMapping | `官方组件接口说明 |
| PrivacyGuard 符号集 | `官方组件接口说明/§6.4(CommonVantage.Shared.dll 字符串) |
| `LENOVO_SETTING_*` 全表与模式枚举 | `官方组件接口说明`(官方组件常量) |
| Dolby profile 映射 `_dolbyMap` | `官方组件接口说明;接口清单 `官方组件接口说明 |
| 降噪模式值 0/1/2/3/4/10 | `官方组件接口说明(`SEActionMapping` + DispatcherConfig.xml) |
| Fn 热键服务结构与 OSD 时序 | `官方组件接口说明`(INF + 组件字符串资源) |
| PrivacyGuard 底层路径:UTILITY_DATA 实例调用约定 + datatype 支持表 | **实机探测** `目标机实机接口探测记录`;featuretype↔隐私位映射仍 [推断] §4.2 |
| 护眼档位与 Display_Mode 复用 | [推断] §3.5 |

## 10. USB 端口清单(`usb ports`,只读)

不复用 §11(interfaces.md)的 EnergyDrv 充电开关:那是"给外设供电的开关位",没有端口/速率/PD
信息。端口清单走 SetupAPI + USB 集线器 IOCTL,实机 21VG 复核(完整字节证据见
`docs/evidence/re-usb-ports-20261105.md`)。

| 层 | 通道 | 实机结果 |
|---|---|---|
| 集线器发现 | `GUID_DEVINTERFACE_USB_HUB` 设备接口 | 2 个:`USB\ROOT_HUB30\4&32F967F1&0&0`(5 口)、`USB\ROOT_HUB30\4&196F3462&0&0`(12 口) |
| 集线器形态 | `IOCTL_USB_GET_NODE_INFORMATION` | `bNumberOfPorts` 5/12,`wHubCharacteristics` 0x0000,`HubIsBusPowered`=0 |
| 端口状态/速率 | `IOCTL_USB_GET_NODE_CONNECTION_INFORMATION_EX` | 端口 4/5/8 有设备,其余 14 口 `ConnectionStatus`=0;速率字节按 `USB_DEVICE_SPEED`(0 Low/1 Full/2 High/3 SuperSpeed)解码,越界值原样输出 |
| 端口→设备 | `IOCTL_USB_GET_NODE_CONNECTION_DRIVERKEY_NAME` 与集线器子 devnode 的 `DEVPKEY_Device_Driver` 精确对齐 | Goodix `27c6:659a`(Full)、相机 `174f:182b`(High)、接收器 `046d:c542`(Full) |
| 设备申报功率 | `IOCTL_USB_GET_DESCRIPTOR_FROM_NODE_CONNECTION`(配置描述符) | 100 mA / 500 mA / 50 mA(`bMaxPower` 按 `bcdUSB` 选 2 mA 或 8 mA 单位) |
| USB4 | `Usb4HostRouter`(PCI `E434`)+ `USB4\ROOT_DEVICE_ROUTER` | 只报"存在";该栈无用户态端口枚举,端口列为不可用 |
| PD / BC1.2 | UCSI `ACPI\USBC000`(`UcmUcsiAcpiClient`,fw 1.2) | **不可读**:UCM 类 GUID 下 0 个设备接口,且全命名空间无 UCM/USB4/Type-C 连接器 WMI 类 → 每个端口 `power_delivery` 为空,并给出该原因 |

明确不做的事:`Win32_PortConnector`(SMBIOS Type 9)在本机是模板行(Keyboard/Mouse/USB/CRT/COM1),
不能作为端口来源;端口颜色/宣传参数推 PD 瓦数同样禁止;`..._INFORMATION_EX_V2` 在本机对所有
输入长度返回 Win32 错误 87,因此不据此声明 SuperSpeedPlus/USB4 速率。命令为只读
(`StateLayer::Runtime`,`read_only`),打开集线器句柄时 desired access=0。
