# 04 · 外设控制 — 功能与接口规范

> 读者:实现者。覆盖:键盘(背光/Fn 键/Fn-Ctrl 互换/Win 锁)、触控板、面板(刷新率/色彩/超分/OD)、
> 摄像头/麦克风/指纹隐私开关、智能感知(人机感应)、音频(Dolby/智能降噪)。
> 交叉引用:通道细节见 `01-hal-interfaces.md`;BIOS 设置族见 `05-bios-settings.md`;
> Linux 映射见 `09-linux-backend.md`;WMI 签名索引见 `A-wmi-reference.md`。

## 0. 一页速查

| 功能 | 通道 | CLI |
|---|---|---|
| 键盘背光档位/灯效 | WMI `LENOVO_LIGHTING_METHOD` | `kbd backlight {0..3} [--effect]` |
| Fn/Ctrl 互换 | BIOS `FoolProofFnCtrl` | `kbd fn-ctrl-swap {on|off}` |
| F1-F12 主功能 | BIOS `HotkeyMode` + `F1-F12AsPrimaryFunction` | `kbd fnlock {on|off}` |
| Win 键锁定 | WMI `LENOVO_GAMEZONE_DATA.SetWinKeyStatus` | `kbd winlock {on|off}` |
| 触控板开关 | WMI `LENOVO_GAMEZONE_DATA.SetTPStatus` | `touchpad {on|off}` |
| 面板刷新率 | WMI `LENOVO_INTERNAL_PANEL_REFRESH_RATE_DATA` + ChangeDisplaySettingsEx | `panel rate {60|120|auto}` |
| 面板色彩/显示模式 | WMI `LENOVO_PANEL_METHOD` | `panel color MODE` |
| 超分辨率(SR) | 注册表 + SmartEngine | `panel super-resolution {on|off}` |
| Display OverDrive | BiosOD.dll → 注册表/WMI | `panel overdrive {on|off}` |
| 摄像头开关 | BIOS `IntegratedCamera` / PrivacyGuard | `privacy cam {on|off}` |
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

### 1.5 键盘特性位图

`LENOVO_GAMEZONE_DATA.GetKeyboardfeaturelist(out Data: UInt32)` — 按位 OR 的能力掩码:
bit0=Fn/Ctrl swap,bit1=F1-F12 primary,bit2=Win 锁,bit3=触控板禁用,bit4+=背光/G-Sync/OD 等。
能力探测一律先调它。

---

## 2. 键盘背光

### 2.1 首选通道:`LENOVO_LIGHTING_METHOD`(实例 `ACPI\PNP0C14\LLT_0` 族)

```
读: Get_Lighting_Current_Status(Lighting_ID: UInt8,
        out Current_Brightness_Level: UInt8, out Current_State_Type: UInt8) → Boolean
写: Set_Lighting_Current_Status(Current_Brightness_Level: UInt8,
        Current_State_Type: UInt8, Lighting_ID: UInt8) → Boolean
```

`LENOVO_LIGHTING_DATA` 静态属性:`Lighting_Id`(键盘=0)、`Lighting_Type`
(0=无/1=单色/2=RGB/3=RGB 多区 [推断])、`State_Type_Num`(亮度档位数)、
`Default_Brightness_Level`、`Brightness_Level`(当前)。

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

### 3.1 刷新率

`LENOVO_INTERNAL_PANEL_REFRESH_RATE_DATA`(实例 `GMZN_0..GMZN_11`,每 GPU/输出一路,
`Active=true` 为主屏):

| 属性 | 类型 | 目标机实测值 |
|---|---|---|
| InternalPanelHwID | UInt32 | 0x8A000A00(联想内部型号) |
| MinimumRefreshRate / MaximumRefreshRate | UInt16 | 60 / 120 |
| DefaultRefreshRate | UInt16 | 60 |
| Mode | UInt16 | 0=手动固定,1=自适应 VRR,2=性能优先(恒高刷) |

- 写:`LENOVO_BIOS_ASSISTANT.SetValue(IndexData, ValueData, out ReturnData)` 或
  `LENOVO_OTHER_METHOD.SetFeatureValue(IDs, value)`;刷新率项 IDs 形如 `0xA000001..0xA000004`
  (=167968769..167968772,`LENOVO_CAPABILITY_DATA_00` 描述其范围/步长)。
- 即时切刷新率也可用 Windows `ChangeDisplaySettingsEx`(官方 SmartDisplayAddin 的 ARR 即此路径)。
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

> TconSDK/BiosOD 为 Windows 专有闭源组件。净室实现:**Windows 侧优先使用 §3.2 的 WMI
> PANEL_METHOD**(语义等价);Linux 侧见 09 文档 §6。X-Rite 云 profile 生态封闭,标注为可选,
> 建议以本地 ICC 配置替代。

### 3.4 超分辨率(SR,V2001)

- 开关:`HKLM\SOFTWARE\Lenovo\SmartEngine\ModuleSettings\SR\SuperResolutionStatus`;
  SRMapping:0/1/3/4→2(开),2→1(关)。
- 与场景引擎联动(DispatcherConfig.xml V2001)。**Windows 专有特性**,净室标注为可选。

### 3.5 护眼(低蓝光)

- 路径一:BIOS 项 `LowBlueLight,Enable`(若 BiosSelections 枚举存在)。
- 路径二:`Panel_Set_Display_Mode` 的护眼档位(0=关,1=中,2=高)[推断,以 Support_Status 实测为准]。
- 路径三(PCManager 护眼):向 `LenovoPcManagerService` 发 SCM 自定义消息
  `SERVICE_CONTROL_CUSTOM_MESSAGE_OPENEYEGUARDIANS` / `_CLOSEEYEGUARDIANS`
  (承载者 `LenovoMonitorManager.exe`,显示器颜色过滤;证据 `电脑管家组件内部结构说明/§9.4)。
- SmartInteractAddin 的 `EyeCareMode` 属同族封装。
- **实现建议**:路径一/二为 Lenovo 平台通用;路径三仅在安装了 PCManager 服务时可用,
  本工具应以路径一/二为主、路径三为兼容回退。

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

### 4.3 指纹/人脸凭据

Lenovo 只控制硬件使能;凭据注册走 Windows Biometric Framework,不在本工具范围。

---

## 5. 智能感知(HPD / SmartSense)

硬件:`ACPI\IDEA2002`(IR 摄像头+传感器),Windows HPD 框架 + Lenovo SmartSense 服务。

### 5.1 `LENOVO_SETTING_*` 命令字(代码级证据,ThinkSmartSenseAddin `IntelligentSensingPipe.cs`)

经 `ChangeSetting(uint command, string parameter)` 下发(ACPI/EC 通道):

| 命令 | 值 | 语义 |
|---|---|---|
| LENOVO_SETTING_ENABLE_SST | 65793 | 启停 Smart Sense Technology |
| LENOVO_SETTING_ENABLE_BLC | 131329 | 键盘背光(感应联动)启用 |
| LENOVO_SETTING_SET_BLC_AUTO | 196865 | 背光自动 |
| LENOVO_SETTING_ENABLE_MODE | 262401 | 感应模式 |
| LENOVO_SETTING_SET_BROWSING_TIME | 327937 | 浏览时间(秒) |
| LENOVO_SETTING_ENABLE_SCREEN_LOCK | 393473 | HPD 屏幕锁定 |
| LENOVO_SETTING_ENABLE_APPROACH | 459009 | 接近检测 |
| LENOVO_SETTING_SET_APPROACH_DISTANCE | 524545 | 接近距离(cm) |
| LENOVO_SETTING_ENABLE_PRESENTLEAVE | 590081 | 存在/离开检测 |
| LENOVO_SETTING_SET_LEAVE_WAIT | 655617 | 离开等待(秒) |
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

- 距离档位(PrivacyGuardDistanceType):0=15cm,1=30cm,2=50cm,3=80cm。
- 离开动作(PrivacyGuardActionType):0=锁屏+暂停视频,1=仅锁屏,2=提示。
- 持久化注册表:`HKLM\SYSTEM\CurrentControlSet\Services\SmartSense\Parameters\{ScreenLock, AutomaticKeyboardLight, HPD}`;`LPlatSvc\Parameters\AutomaticKeyboardLight`。
- 事件:Windows HPD COM 回调;`LENOVO_AI_SCENARIO_TYPE_EVENT`(场景 Type,见 03/07 文档)。

---

## 6. 音频

### 6.1 Dolby

RPC 客户端族(`DolbyRpcClient.dll` 原生 / `Lenovo.Vantage.DolbyRpcClient.dll` 托管)暴露:

| 接口 | 语义 |
|---|---|
| GetDolbyEnabled / SetDolbyEnabled | 主开关 |
| GetDolbyHSASupported / GetDolbyFusionSupported | 能力(HSA=硬件空间音频) |
| GetDolbyProfile / SetDolbyProfile | 场景配置 |
| GetDolbyState / SetDolbyState | 精细状态 |
| DolbyEffectChangeEvent | 变更事件 |

Profile 映射(SmartDisplayAddin `_dolbyMap`,代码级):Movie=0,Music=1,Games=2,Voip=3,
Personalize=4,Dynamic=5,Off=6。

> Dolby 栈是闭源 DSP 插件;净室实现仅能开关/切 profile(经其 RPC 或音频端点属性),
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
+ `FnHotkeyUtility.exe`(OSD)。**净室实现无需接管热键本身**(EC 固件直接产出 HID/ACPI 事件),
只需提供:① OSD 等价物(daemon);② 各热键动作的可编程绑定。

能力探测:`LENOVO_UTILITY_DATA.GetIfSupportOrVersion(datatype, out Data)`,
目标机实测(目标机 WMI 实例实机采集 L94-131):datatype 1→3(背光能力),3→2(亮度),
4→2(麦克风静音),18/19/20→24/25/25,26→27,29→32,38→36(特殊键)。

事件源:`LENOVO_UTILITY_EVENT`(PressTypeDataVal: UInt32)— 按键事件;
OSD 时序:接收事件 → 执行动作 → OSD 显示 0.8s + 淡出 0.3s → 通知 UI 同步。

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
| GAMEZONE/UTILITY/LIGHTING/PANEL/BIOS_ASSISTANT/SR_DATA 全部方法签名 | `目标机 WMI 仓库实机采集` 对应类;`Vantage 设备组件内部接口说明§5 |
| BIOS 项实例映射(ISET_17/18/29/40/41/42/51) | `目标机 WMI 实例实机采集` L17-L52 |
| 刷新率 60/120/Mode 值、CAPABILITY_DATA IDs 0xA000001-4 | `目标机 WMI 实例实机采集`;`Vantage 设备组件内部接口说明 |
| PANEL_METHOD 方法族与位域 | `Vantage 设备组件内部接口说明 |
| TconSDK/BiosOD/ACM 通道与 GamutType 映射 | `Vantage SmartPerformance 组件内部接口说明 |
| SR 注册表与 SRMapping | `Vantage SmartPerformance 组件内部接口说明 |
| PrivacyGuard 符号集 | `Vantage 设备组件内部接口说明/§6.4(CommonVantage.Shared.dll 字符串) |
| `LENOVO_SETTING_*` 全表与模式枚举 | `Vantage SmartPerformance 组件内部接口说明(`IntelligentSensingPipe.cs` 常量) |
| Dolby profile 映射 `_dolbyMap` | `Vantage SmartPerformance 组件内部接口说明;接口清单 `Vantage 设备组件内部接口说明 |
| 降噪模式值 0/1/2/3/4/10 | `Vantage SmartPerformance 组件内部接口说明(`SEActionMapping` + DispatcherConfig.xml) |
| Fn 热键服务结构与 OSD 时序 | `Vantage 设备组件内部接口说明`(INF + 组件字符串资源) |
| PrivacyGuard 底层路径:UTILITY_DATA 实例调用约定 + datatype 支持表 | **实机探测** `目标机实机接口探测记录`;featuretype↔隐私位映射仍 [推断] §4.2 |
| 护眼档位与 Display_Mode 复用 | [推断] §3.5 |
