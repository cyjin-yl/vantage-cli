# 05 · BIOS 设置读写接口规范（`root\WMI` `Lenovo_*` 族）

> 读者：**实现者**（用 Rust 重写 lctrl、覆盖 BIOS 设置读写功能的人）。
> 前置：`00-cleanroom-charter.md`（术语与文档地图）、`01-hal-interfaces.md`（通道总览与特权要求）、`A-wmi-reference.md`（全量类签名索引）。
> 状态：v1 · 2026-08-27 · 目标机 Lenovo ThinkBook 21VG（Panther Lake，SMBIOS `THINKBOOK_14_G8+_IPH`）。
> 范围：**BIOS 设置读写**（枚举、读、写、保存/回滚/恢复默认、supervisor 密码查询）。不含 EC 直连通道（见 01/02 文档）、不含 Linux 后端（见 09 文档）。

---

## 0. 一页速查

**核心调用序列**（官方软件实测，见 §6.2）：

```text
1. 枚举/读:   SELECT * FROM Lenovo_BiosSetting            → CurrentSetting = "Name,Value"
2. 枚举值域:  Lenovo_GetBiosSelections.GetBiosSelections(Item="Name") → Selections = "A,B,C"
3. 写:        Lenovo_SetBiosSetting.SetBiosSetting(parameter="Name,Value;") → return = "Success"
4. 提交:      Lenovo_SaveBiosSettings.SaveBiosSettings(parameter=";")       → return = "Success"
5. 回滚:      Lenovo_DiscardBiosSettings.DiscardBiosSettings(...)           → 丢弃未提交更改
6. 恢复默认:  Lenovo_LoadDefaultSettings.LoadDefaultSettings(...)           → 全部恢复出厂
7. 密码状态:  Lenovo_BiosPasswordSettings.PasswordState                      → 位域, bit1 = supervisor 密码已设置
```

**不变式**（实现者必读）：

1. 所有类在 `root\WMI` 命名空间；方法类实例为 `ACPI\PNP0C14\ISET_*`；官方代码一律以 `SELECT * FROM <Class>` 取**第一个**实例并按其 `InstanceName` 定位调用。
2. **属性类 `Lenovo_BiosSetting.CurrentSetting` 是只读镜像，写入必须走方法类** `Lenovo_SetBiosSetting`。
3. `CurrentSetting` 的格式是 **`Name,Value`（英文逗号分两段）**，不是 `=` 分割（01 文档 §2.11 的 `=` 说法有误，以本文档为准，见 §4.1）。
4. 写操作返回串 `"Success"` 才表示命令被接受；随后必须 `SaveBiosSettings` 才写入 NVRAM（**部分项需重启后真正生效**）。
5. 值串**区分大小写**，且不同项的拼写习惯不同（`Enable`/`Disable` 与 `Enabled`/`Disabled` 混用），**不得硬编码**，必须以 `GetBiosSelections` 的返回为准。
6. 调用需要管理员或 SYSTEM 权限（`root\WMI` 命名空间 ACL 限制，见 01 文档 §0/§7）。
7. 官方软件（Vantage 产品线）**只读** `Lenovo_BiosPasswordSettings`，从不调用 `Lenovo_SetBiosPassword`；密码写入格式未在本项目样本中实测（见 §5.3 [推断]）。

---

## 1. 接口族全貌

### 1.1 总览

`root\WMI` 下与 BIOS 设置直接相关的类共 10 个（命名空间 `ROOT\WMI`，来源 `目标机 WMI 仓库实机采集`），另加 2 个配套类：

| 类名 | 类型 | 角色 |
|---|---|---|
| `Lenovo_BIOSElement` | 空类 | 占位/基类标记（无属性无方法） |
| `Lenovo_BiosSetting` | 属性类 | **读**：每个 BIOS 项一个实例，`CurrentSetting` 只读 |
| `Lenovo_SetBiosSetting` | 方法类 | **写**：`SetBiosSetting(parameter)` |
| `Lenovo_SaveBiosSettings` | 方法类 | **提交**：`SaveBiosSettings(parameter)` |
| `Lenovo_DiscardBiosSettings` | 方法类 | **回滚**：`DiscardBiosSettings(parameter)` |
| `Lenovo_LoadDefaultSettings` | 方法类 | **恢复默认**：`LoadDefaultSettings(parameter)` |
| `Lenovo_GetBiosSelections` | 方法类 | **值域枚举**：`GetBiosSelections(Item, out Selections)` |
| `Lenovo_SetFunctionRequest` | 方法类 | 功能请求：`SetFunctionRequest(parameter)`（用途未确认，§6.5） |
| `Lenovo_FunctionRequest` | 属性类 | `CurrentSetting` 只读镜像（与 SetFunctionRequest 对应） |
| `Lenovo_BiosPasswordSettings` | 属性类 | supervisor 密码能力/状态声明 |
| `Lenovo_SetBiosPassword` | 方法类 | 密码设置：`SetBiosPassword(Parameter, out Return)` |
| `LENOVO_BIOS_ASSISTANT` | 方法类 | 数值型 BIOS 辅助接口（FnCtrl/SuperBoost/GPUMode 等索引化项） |
| `LENOVO_REPORT_DIRECT_BIOS_DATA` | 属性类 | 直接 BIOS 数据表（`default_num`/`DependOnID`/`Step`，内容未完全解码） |

### 1.2 逐类签名（机器采集，`目标机 WMI 仓库实机采集`）

```text
##### CLASS: Lenovo_BIOSElement
  (无属性、无方法; 仅为类层次占位)

##### CLASS: Lenovo_BiosSetting                       ← 读通道
  PROP: Active : Boolean
  PROP: CurrentSetting : String                       ← "Name,Value", 每实例一项
  PROP: InstanceName : String                         ← "ACPI\PNP0C14\ISET_N"
  (无方法)

##### CLASS: Lenovo_SetBiosSetting                    ← 写通道
  PROP: Active : Boolean
  PROP: InstanceName : String
  METHOD: SetBiosSetting -> Boolean
    PARAM: [ID,in]  parameter : String                ← "Name,Value;"
    PARAM: [ID,out] return    : String                ← "Success" 或错误描述

##### CLASS: Lenovo_SaveBiosSettings                  ← 提交
  PROP: Active : Boolean
  PROP: InstanceName : String
  METHOD: SaveBiosSettings -> Boolean
    PARAM: [ID,in]  parameter : String                ← 官方实测传 ";"
    PARAM: [ID,out] return    : String

##### CLASS: Lenovo_DiscardBiosSettings               ← 回滚
  PROP: Active : Boolean
  PROP: InstanceName : String
  METHOD: DiscardBiosSettings -> Boolean
    PARAM: [ID,in]  parameter : String
    PARAM: [ID,out] return    : String

##### CLASS: Lenovo_LoadDefaultSettings               ← 恢复出厂
  PROP: Active : Boolean
  PROP: InstanceName : String
  METHOD: LoadDefaultSettings -> Boolean
    PARAM: [ID,in]  parameter : String
    PARAM: [ID,out] return    : String

##### CLASS: Lenovo_GetBiosSelections                 ← 值域枚举
  PROP: Active : Boolean
  PROP: InstanceName : String
  METHOD: GetBiosSelections -> Boolean
    PARAM: [ID,in]  Item       : String              ← 设置项名, 如 "GraphicsDevice"
    PARAM: [ID,out] Selections : String              ← 逗号分隔的合法值列表

##### CLASS: Lenovo_SetFunctionRequest                ← 功能请求
  PROP: Active : Boolean
  PROP: InstanceName : String
  METHOD: SetFunctionRequest -> Boolean
    PARAM: [ID,in]  parameter : String
    PARAM: [ID,out] return    : String

##### CLASS: Lenovo_FunctionRequest
  PROP: Active : Boolean
  PROP: CurrentSetting : String                       ← 只读状态镜像
  PROP: InstanceName : String

##### CLASS: Lenovo_BiosPasswordSettings              ← 密码能力/状态声明
  PROP: Active : Boolean
  PROP: InstanceName : String
  PROP: MaxLength : UInt32                            ← 本机 128
  PROP: MinLength : UInt32                            ← 本机 1
  PROP: PasswordMode : UInt32                         ← 本机 1
  PROP: PasswordState : UInt32                        ← 本机 0 (位域, §5.2)
  PROP: SupportedEncodings : UInt32                   ← 本机 1 (ascii)
  PROP: SupportedKeyboard : UInt32                    ← 本机 1 (us)

##### CLASS: Lenovo_SetBiosPassword                   ← 写密码
  PROP: Active : Boolean
  PROP: InstanceName : String
  METHOD: SetBiosPassword -> Boolean
    PARAM: [ID,in]  Parameter : String
    PARAM: [ID,out] Return    : String
```

### 1.3 `LENOVO_BIOS_ASSISTANT` — 数值型辅助接口

标准 `Lenovo_SetBiosSetting` 之外，联想还提供一套**按索引寻址**的数值型 BIOS 接口，Vantage 用它实现 Fn/Ctrl 互换、SuperBoost、GPUMode（`目标机 WMI 仓库实机采集` L473 起；消费端 `WmiAgent.cs`）：

```text
##### CLASS: LENOVO_BIOS_ASSISTANT
  PROP: Active : Boolean
  PROP: InstanceName : String
  METHOD: GetCapabilityValue -> Boolean        [out] Data     : UInt32  能力位域
  METHOD: GetValue         -> Boolean    [in] IndexData       : UInt32  [out] Data : UInt32
  METHOD: SetValue         -> Boolean    [in] IndexData       : UInt32
                                         [in] ValueData       : UInt32  [out] ReturnData : UInt32
```

**已解码的 IndexData（FunctionID 枚举，`FunctionID.cs`）**：

| IndexData | 名称 | GetValue Data 位域 | SetValue ValueData |
|---|---|---|---|
| 1 | `FnCtrlSwap` | bit0 = 是否已互换；bit31 = 命令成功位 | `0`=不互换 `1`=互换 |
| 2 | `SuperBoost` | bit0 = 是否开启；bit31 = 命令成功位 | `0`=关 `1`=开 |
| 3 | `GPUMode` | — | 官方代码实测传 `"01"`/`"02"`（§7.4.5 [推断] 语义未最终确认） |

**GetCapabilityValue 位域（`WmiAgent.cs::GetBiosCapability` / `AdvancedGPUCapbility`，`BiosCapability.cs`）**：

```text
bit 0        (0x00000001)  SupportSuperMode   — 支持 SuperBoost
bit 1..2     (0x00000006)  SupportGPUMode     — 支持的 GPU 模式数量/标志 (UInt32)
bit 3        (0x00000008)  SupportFnCtrlSwap  — 支持 Fn/Ctrl 互换
bit 16..23   (0x00FF0000)  版本字节: bit 20..23 = MajorVer, bit 16..19 = MinorVer
bit 31       (0x80000000)  CommandStatus      — 命令成功位 (1 = 成功)
```

**本机实测**（`目标机 WMI 实例实机采集` L193）：`BA.GetCapabilityValue => Data=2147483656 (0x80000008)`，即 CommandStatus=1、SupportFnCtrlSwap=1、SupportSuperMode=0、SupportGPUMode=0。

**调用语义**（`WmiAgent.cs`）：所有返回值以 **bit31（`0x80000000`）作为命令成功标志**；成功时业务位在低字节。官方代码 `CommandStatus = (value & 0x80000000) == 0x80000000`。

### 1.4 `LENOVO_GAMEZONE_DATA` 中的 GPU 模式方法（配套）

GPU 模式切换在官方实现中横跨 BIOS 项与 GAMEZONE 通道（`WmiAgent.cs::SetGPUMode` / `GetGPUMode`）：

```text
Lenovo_GetBiosSelections.GetBiosSelections("GraphicsDevice") → 合法 GPU 模式名列表
Lenovo_BiosSetting.CurrentSetting 中包含 "GraphicsDevice,<模式>"
LENOVO_GAMEZONE_DATA.IsSupportIGPUMode   [out] Data      — 支持性探测
LENOVO_GAMEZONE_DATA.GetIGPUModeStatus   [out] Data      — 当前模式 (UInt32)
LENOVO_GAMEZONE_DATA.SetIGPUModeStatus   [in] mode (UInt32) [out] Data — 切换 (部分模式需重启)
LENOVO_GAMEZONE_DATA.NotifyDGPUStatus    [in] status (UInt32)          — 上报 dGPU 存在性
LENOVO_GAMEZONE_DATA.IsBIOSSupportOC / SetBIOSOC / GetBIOSOCMode       — BIOS 放行 OC (需重启)
```

**GPUModeStatus 枚举**（`GPUModeStatus.cs`）：`UnKnown=-1, HybirdMode=0, IGPUOnlyMode=1, AutoMode=2, DGPUMode=3`。

**GraphicsDevice 值**：官方代码匹配三种字符串（`WmiAgent.cs::GetGPUMode`）：`SwitchableGraphics`、`DynamicGraphics`、`DiscreteGraphics`；其中前两者语义等同"混合/可切换"，后者为"独显"。

**证据**

- 全部类签名（含 `LENOVO_BIOS_ASSISTANT`）：`目标机 WMI 仓库实机采集` L367-426（`Lenovo_BIOSElement`…`Lenovo_LoadDefaultSettings`）、L473-484（`LENOVO_BIOS_ASSISTANT`）、L610（`LENOVO_REPORT_DIRECT_BIOS_DATA`）、L126-236（`LENOVO_GAMEZONE_DATA`）。
- 消费端实现与位域解码：LenovoProductivitySystemAddin 的 WMI 组件（`GetBiosCapability`、`AdvancedGPUCapbility`、`Get/SetAssistantValue`、`GetInstanceName` 方法语义见附录 A）。
- 枚举定义：同目录 `FunctionID.cs`（L5-7）、`GPUModeStatus.cs`；`…/LenovoProductivitySystemAddin.PayloadTypes/BiosCapability.cs`。
- `BA.GetCapabilityValue` 实测值：`目标机 WMI 实例实机采集` L193。

---

## 2. 通用调用机制

### 2.1 命名空间与实例寻址

- 命名空间：`ROOT\WMI`（大小写不敏感）。
- 实例命名：`ACPI\PNP0C14\ISET_N`（N 为 0 起的索引）、密码类同为 `ISET_0`、资产标签为 `ATAG_N`（`目标机 WMI 实例实机采集` L235-299）。
- **`Lenovo_BiosSetting` 每实例对应一个 BIOS 项**：`CurrentSetting` 是一行 `Name,Value` 文本，Empty 表示该项留空（本机 ISET_5/7/8/9/10 为空）。
- 方法类（Set/Save/Discard/LoadDefault/GetSelections/SetFunctionRequest/SetBiosPassword）在采集中同样呈现为多个实例，但**官方代码一律取第一个实例**（`GetInstanceName` 返回 `SELECT * FROM <Class>` 的首行 `InstanceName`），把项名放在 `parameter`/`Item` 参数里。实现者照此办理即可（以 `SELECT * FROM Lenovo_SetBiosSetting` 首实例为调用目标）。

### 2.2 方法调用协议（CIM）

官方调用骨架（`WmiProvider.cs::CallMethod`，逐字节可对照）：

```text
path   = "Lenovo_SetBiosSetting.InstanceName='ACPI\PNP0C14\ISET_0'"
mo     = ManagementObject(ns="ROOT\WMI", path)
inPar  = mo.GetMethodParameters("SetBiosSetting")
inPar["parameter"] = "IntegratedCamera,Disable;"
outPar = mo.InvokeMethod("SetBiosSetting", inPar)
ret    = outPar.GetPropertyValue("return").ToString()     # "Success" | 错误描述
```

要点：

- 必须先按 `InstanceName` 用对象路径定位，再取方法参数模板；`GetMethodParameters` 可获得参数名（`parameter`/`Item`/`IndexData`/`ValueData`/`Parameter`，注意类不同参数名不同，见 §1.2）。
- 方法返回的 `Boolean` 表示"调用本身是否执行"，**业务成败看 out 参数**（`return`/`Selections`/`Return`/`Data`/`ReturnData` 的字符串或数值）。
- 读取属性用 `SELECT * FROM <Class>` 后取属性（`CurrentSetting`/`PasswordState`/`InstanceName`），空属性值需跳过（官方 `QueryData` 对空串不收集）。

### 2.3 权限

- 管理员或 SYSTEM；普通用户调用会抛 `ManagementException`（`root\WMI` ACL，见 01 文档 §0 决策铁律 1 与 §7 特权表）。
- lctrl 实现：Windows 侧建议以 `runas`/服务方式提权；CLI 在非提权时给出明确错误退出码（§9）。

### 2.4 异常与错误码（官方容错模式）

官方 `LenovoBiosWmiInterface.cs` 对 `LENOVO_CAPABILITY_DATA_00`/`LENOVO_OTHER_METHOD` 的容错模式（同类适用于所有 Lenovo WMI 调用）：

- `ManagementException.ErrorCode == 0x80041010`（-2147217392）：类/查询不存在 → 置 `_capabilityNotExist=true` 并**缓存**，后续调用直接返回空值（不再重复查询）。
- `ManagementException.ErrorCode == 0x8004100F`（-2147217393）：对象无效 → 同样按"无能力"处理返回。
- 其他异常 → 记日志，返回 false/空。

实现者建议：把 WBEM 异常按 `0x80041010`/`0x8004100F`（类或对象缺失）与其余错误分类，前者映射为"该机型不支持此接口"，后者映射为可重试/报错。

**证据**

- 调用骨架：WmiProvider 组件（`CallMethod`、`QueryData`）。
- 实例寻址与索引：`目标机 WMI 实例实机采集` L235-299；`WmiAgent.cs::GetInstanceName` L461-478。
- 容错 HRESULT：LenovoBiosWmiInterface 组件（`GetCapability`/`GetFeatureValue` 的异常分支）。
- 权限：`docs/01-hal-interfaces.md` §0、§7（`root\WMI` 方法类需管理员或 SYSTEM）。

---

## 3. 参数编码与返回码

### 3.1 `SetBiosSetting` — `"Name,Value;"` 串

- 格式：`<项名>,<值>;` —— 项名英文逗号值，**结尾分号**。官方实测构造：`ItemName + "," + ItemValue + ";"`（`WmiAgent.cs::SetIOControlItem` L242）。
- 示例：`"IntegratedCamera,Disable;"`、`"FoolProofFnCtrl,Enable;"`、`"GraphicsDevice,DiscreteGraphics;"`。
- 值未经客户端白名单过滤：`SetIOControlItem` 接受请求里任意 `ItemName/ItemValue`（UI 白名单只影响展示层，见 §8.3）。
- 多项写入官方是**逐个串行调用**，任一项返回非 `Success` 即中止，不再执行 Save。

### 3.2 `SaveBiosSettings` / `DiscardBiosSettings` / `LoadDefaultSettings`

- `SaveBiosSettings` 参数：官方实测传 **`";"`**（`WmiAgent.cs` L249-253：`{"parameter" = ";"}`）；`Vantage 设备组件内部接口说明 亦记录空串写法。实现者建议统一用 `";"`（与实测一致）。
- `DiscardBiosSettings`/`LoadDefaultSettings` 参数：`Vantage 设备组件内部接口说明` 记录为空串；无更细实测。建议传 `";"` 或空串并**以返回 `"Success"` 为准**（实现时两者都尝试兼容 [推断]：不同固件可能二选一接受）。
- 三个方法的 `return` 同为 `"Success"` 或错误描述串。

### 3.3 `GetBiosSelections`

- 入参 `Item` 是项名（不带值、不带分号），如 `"GraphicsDevice"`；出参 `Selections` 为**逗号分隔的合法值列表**。
- 官方解析：`Selections.Split(',')` 后 `Trim()` 每个元素（`WmiAgent.cs::AdvancedGPUCapbility` L84-90；`BiosSettings.cs` L91-99）。
- 注意：返回的列表**可能首元素为空**（示例 `" ,SwitchableGraphics,..."` 的健壮性见 §9.1 的 `SplitRemoveEmpty` 处理）；官方用了 `RemoveEmptyEntries`。

### 3.4 返回码表

| 位置 | 值 | 含义 | 出处 |
|---|---|---|---|
| `return`/`Return` out 串 | `"Success"` | 命令被接受（Set 项合法 / Save 已提交 / 回滚完成） | `WmiAgent.cs` L246/L256 比对 |
| `return` out 串 | 其他字符串 | 错误描述（官方当作文本展示，未解析具体码） | `WmiAgent.cs`（`empty != "Success"` 即失败） |
| WMI 方法 Boolean | `true` | 方法调用执行成功（不代表业务成功） | CIM 语义 |
| `BA` 返回值 bit31 | `1` | BIOS Assistant 命令成功 | `WmiAgent.cs` L333/L421/L449 |
| `PasswordState` | 位域 | §5.2 | `wmi-instances2.txt` L64 |
| WBEM `0x80041010` / `0x8004100F` | 异常 | 类/对象不存在 → 该机型无此接口 | §2.4 |

> 官方代码对失败的处理：`SetIOControlItem` 中项写失败 → 立即中断且**不调用 Save**（防止部分提交）；Save 失败 → 返回 false，界面提示保存失败。这是官方最重要的保护逻辑之一（§8.3）。

**证据**

- 编码构造/返回比对/串行中止：`WmiAgent.cs::SetIOControlItem` L225-261（`ItemName+","+ItemValue+";"`；`if (empty != "Success") { flag=false; break; }`；`SaveBiosSettings` 传 `";"` 且要求 `text == "Success"`）。
- Selections 解析：`WmiAgent.cs::AdvancedGPUCapbility` L79-90；`…/LenovoSnapshotAddin/2.0.0.18/Lenovo.Cdat/Lenovo.Cdat.DataCollection.QuickData.Populators/BiosSettings.cs` L59-118。
- 空串/分号写法：`Vantage 设备组件内部接口说明（`SaveBiosSettings(Empty)`/`DiscardBiosSettings(Empty)`）。

---

## 4. 设置项发现

### 4.1 枚举算法（`lctrl bios list`）

官方快照收集器（`BiosSettings.cs::CaptureThinkPadBiosSettings`）的算法即实现模板：

```text
1. rows = SELECT * FROM Lenovo_BiosSetting (属性 CurrentSetting)
2. 对每行: 若 CurrentSetting 为空 → 跳过
   name, value = CurrentSetting.Split(',')   # 只取前两段
3. （可选增强）对每个 name 调 Lenovo_GetBiosSelections.GetBiosSelections(name)
   得到该项目的合法值列表, 一并展示
```

健壮性细节（官方实测）：

- `Split(',')` 后**少于 2 段则丢弃该行**（有实例的 `CurrentSetting` 为空串，本机 ISET_5/7/8/9/10）。
- `GetBiosSelections` 对单个项失败（某些项不支持枚举）只记警告，**不中断整体枚举**——实现者应对每个项独立 try/catch。
- 项名比较用 `OrdinalIgnoreCase`（`BiosSecuritySettings.cs` 的字典键）；展示时保留原始大小写（`array[0]`）。
- 本机（SMB 平台）与 TPE（ThinkPad）平台的项集合不同：官方用两份清单做 UI 过滤（`WmiAgent.cs` L28-41）：`_smbIOControlItems`（11 项，见下表）与 `_tpeIOControlItems`（14 项，`EthernetLANAccess/WirelessLANAccess/WirelessWANAccess/BluetoothAccess/USBPortAccess/MemoryCardSlotAccess/SmartCardSlotAccess/IntegratedCameraAccess/IntegratedAudioAccess/MicrophoneAccess/FingerprintReaderAccess/ThunderboltAccess/IOAccessTouchPanel/NfcAccess`，ThinkPad 变体，本机未必枚举）。

> ⚠️ 与 01 文档 §2.11 的差异：01 文档写 `CurrentSetting` "以 `=` 分割（形如 `CurrentValue=On`）"。**实测格式是英文逗号**（`WakeOnLan,Disabled`），消费端代码全部按 `,` 分割（`WmiAgent.cs`、`BiosSettings.cs`、`GetIOControlItems`）。本文档以逗号为准。

### 4.2 本机全量设置项表

以下为 21VG 实机 `Lenovo_BiosSetting` 全量枚举（`目标机 WMI 实例实机采集` L1-55，含空实例在内共 36 项）；实例索引 ISET_N 为 `目标机 WMI 实例实机采集`（L235-284，采集截断于 ISET_11）与 `Vantage 设备组件内部接口说明/§6 的记录，**ISET_12+ 的索引编号以 vantage-device.md 为准，未在本项目原始采集中复核 [推断]**。（wmi-instances2 的"FULL"清单不含空行项的实例号。）

| 设置项（枚举原串） | 本机当前值 | 值域（惯例） | 语义/官方用途 |
|---|---|---|---|
| `WakeOnLan` | `Disabled` | `Enabled`/`Disabled` | 有线网卡唤醒（注意此项用过去式拼写） |
| `MacAddressPassThrough` | `Disable` | `Enable`/`Disable` | 网卡 MAC 直通 |
| `WakeOnLanFromDock` | `Disable` | `Enable`/`Disable` | 扩展坞唤醒 |
| `LenovoCloudServices` | `Disable` | `Enable`/`Disable` | 联想云服务开关（Vantage 相关） |
| `ReinstallWindowsFromCloud` | `Disable` | `Enable`/`Disable` | 云端重装 Windows 入口 |
| `WirelessLAN` | `Enable` | `Enable`/`Disable` | 无线网卡硬件开关（IO 控制清单项） |
| `Intel(R)VirtualizationTechnology` | `Enable` | VT-x | Intel 虚拟化（需重启） |
| `Intel(R)VT-dFeature` | `Enable` | VT-d | DMA 直通（需重启） |
| `SecureRollbackPrevention` | `Enable` | `Enable`/`Disable` | BIOS 回滚保护（⚠️ §8.2） |
| `HotkeyMode` | `Disable` | `Enable`/`Disable` | F1-F12 是否"需 Fn"；`Enable`=需按 Fn，`Disable`=直接触发主功能 |
| `FoolProofFnCtrl` | `Enable` | `Enable`/`Disable` | Fn 与 Ctrl 互换（防误触） |
| `AlwaysOnUSB` | `Enable` | `Enable`/`Disable` | USB 关机充电（运行时覆盖走 EnergyDrv，§7.4.2） |
| `ChargeInBatteryMode` | `Disable` | `Enable`/`Disable` | 电池模式下 USB 充电 |
| `FlipToStart` | `Enable` | `Enable`/`Disable` | 开盖开机 |
| `BiosSelfHealing` | `Enable` | `Enable`/`Disable` | BIOS 自愈（备份恢复) |
| `SetStrongPassword` | `Disable` | `Enable`/`Disable` | 强制强密码（与密码机制相关） |
| `EnhancedWindowsBiometricSecurity` | `Enable` | `Enable`/`Disable` | 增强 Windows Hello 生物安全 |
| `IntelPlatformTrustTechnology` | `Enable` | `Enable`/`Disable` | Intel PTT（fTPM） |
| `EthernetLAN` | `Enable` | `Enable`/`Disable` | 有线网卡硬件开关 |
| `Bluetooth` | `Enable` | `Enable`/`Disable` | 蓝牙硬件开关 |
| `USBPort` | `Enable` | `Enable`/`Disable` | USB 口硬件开关 |
| `MemoryCardSlot` | `Enable` | `Enable`/`Disable` | 读卡器硬件开关 |
| `IntegratedCamera` | `Enable` | `Enable`/`Disable` | 摄像头硬断（Vantage 隐私开关） |
| `Microphone` | `Enable` | `Enable`/`Disable` | 麦克风硬断 |
| `FingerprintReader` | `Enable` | `Enable`/`Disable` | 指纹读头硬断 |
| `Thunderbolt(TM)` | `Enable` | `Enable`/`Disable` | Thunderbolt 口开关（项名含括号，见 §9.1 转义） |
| `SecureBoot` | `Enable` | `Enable`/`Disable` | Secure Boot（快照安全收集项） |
| `USBBoot` | `Enable` | `Enable`/`Disable` | USB 启动（⚠️ §8.2） |
| `PXEBootToLAN` | `Enable` | `Enable`/`Disable` | PXE 网络启动（⚠️ §8.2） |
| `IPV4PXEFirst` | `Enable` | `Enable`/`Disable` | IPv4 PXE 优先 |
| `EFI-BootOrder` | `01:02` | 十六进制启动项序 | 启动顺序（⚠️ §8.2；值格式不同：冒号分隔的两位 hex） |
| `F1-F12AsPrimaryFunction` | `Enable` | `Enable`/`Disable` | F1-F12 主功能为标准 F 键（**FnLock 的 BIOS 表达**） |
| `FnAndCtrlKeySwap` | `Disable` | `Enable`/`Disable` | 另一套 Fn/Ctrl 互换项（本机与 FoolProofFnCtrl 并存） |
| `PowerOnWithACAttach` | `Enable` | `Enable`/`Disable` | 插电自动开机 |
| （空项）ISET_5/7/8/9/10 | （空） | — | 未公开/留空槽位 |

**GraphicsDevice（GPU 模式，不在上述枚举的静态清单里出现，但 WriteAgent 实读）**：`CurrentSetting` 含 `GraphicsDevice,<模式>` 行；模式 ∈ `SwitchableGraphics`/`DynamicGraphics`/`DiscreteGraphics`（也可经 `GetBiosSelections("GraphicsDevice")` 拿全量）。

### 4.3 `GetBiosSelections` 用法案例

- `GetBiosSelections("GraphicsDevice")` → Selections 逗号列表 → 每个元素 `Trim()`（`WmiAgent.cs` L79-90）。这是唯一在本项目样本中**观察到真实调用**的 Item——其余项均未在官方代码中调用过 Selections（快照收集器对每项都调，但那是运行时行为，未在本样本采集中留痕）。
- 默认行为：`PcSystemType == "2"`（笔记本？）时快照收集器才调 Selections（`BiosSettings.cs` L85）。

### 4.4 代码引用但本机枚举未见的项

以下名称出现在官方代码中但**不在 21VG 枚举清单**里，存在性因平台/BIOS 版本而异，实现者应将其当作"可能项"，出现则展示，不出现则无需创建：

- 安全收集项（`BiosSecuritySettings.cs` 字典键）：`SecureBoot`（本机有）、`DeviceGuard`、`EnhancedWindowsBiometricSecurity`（本机有）、`BottomCoverTamperDetected`、`BlockSIDAuthentication`、`AMTControl`、`LockBIOSSettings`。
- 快照诊断项：`DashEnabled`（`PotentialIssuesPopulator.cs`，Dash 检测）。
- 盟友建议项：`EffectivePowerModeMaxPerformance`（`hal-services.md` L70 [推断]：`SetBiosSetting` 的某些 key 触发 `LENOVO_DISPATCHER_EVENT`）。

**证据**

- 全量枚举：`目标机 WMI 实例实机采集` L1-55（`##### FULL Lenovo_BiosSetting`）。
- 枚举算法与容错：`BiosSettings.cs::CaptureThinkPadBiosSettings` L59-118（分号 Split、少于 2 段跳过、Selections 每项独立容错）。
- 项名清单（SMB/TPE）：`WmiAgent.cs` L28-41（`_smbIOControlItems`/`_tpeIOControlItems`）、`GetIOControlItems` L263-309。
- 安全/诊断项：`BiosSecuritySettings.cs` L79-129（字典），`PotentialIssuesPopulator.cs` L84-102（`DashEnabled`）。
- 实例索引 ISET_17/18/29/40/41/42/51：`Vantage 设备组件内部接口说明（ISET_18）、§3.2（ISET_17/51）、§6.1（ISET_40）、§6.2（ISET_41）、§6.3（ISET_42/ISET_29）。
- GraphicsDevice 读取：`WmiAgent.cs` L96-121（`GetAdvancedGPUMode`）、L190-223（`GetGPUMode`）。

---

## 5. 密码机制（supervisor password）

### 5.1 `Lenovo_BiosPasswordSettings`（能力/状态声明，读到为主）

本机实测（`目标机 WMI 实例实机采集` L56-70，实例 `ACPI\PNP0C14\ISET_0`）：

```text
MaxLength          : 128
MinLength          : 1
PasswordMode       : 1
PasswordState      : 0
SupportedEncodings : 1
SupportedKeyboard  : 1
```

| 属性 | 含义 | 本机值 |
|---|---|---|
| `MinLength`/`MaxLength` | 密码长度上下限 | 1 / 128 |
| `PasswordMode` | 密码类型模式 | 1 |
| `PasswordState` | 密码状态位域（§5.2） | 0 = 未设置 |
| `SupportedEncodings` | 支持的输入编码 | 1 |
| `SupportedKeyboard` | 支持的键盘布局 | 1 |

### 5.2 `PasswordState` 位域

官方消费端（`WmiAgent.cs::CheckBiosPasswordSet` L349-366）：

```csharp
uint num = Convert.ToUInt32(QueryData("SELECT * FROM Lenovo_BiosPasswordSettings", "PasswordState")[0]);
PasswordState = (num & 2) == 2;   // bit1 = supervisor 密码已设置
```

- **bit 1（0x2）= supervisor 密码已设置**。（bit0 未观察到使用 [推断]：部分文献将 bit0 作为"是否启用密码功能"的机型相关位，本样本 PasswordState=0 无法证实。）
- 查询对象：`SELECT * FROM Lenovo_BiosPasswordSettings` 取 `PasswordState` 列；**属性存在且可枚举**是该类在本机型可用的前提。
- 注意 `PasswordState` 与 WMI 别名 `Win32_ComputerSystem.AdminPasswordStatus`（快照收集器另用其 `==1` 判定 supervisor 密码，`BiosSecuritySettings.cs` L52-66）是两个独立来源，实现者可用后者交叉验证 [推断]（前者为联想通道，后者为 Windows 标准）。

### 5.3 `SetBiosPassword` 编码（【推断】——未在本项目样本中实测）

**事实**：类签名存在（`Lenovo_SetBiosPassword.SetBiosPassword(Parameter:String)→Return:String`），但**本项目所有官方样本中没有任何代码调用它**（全部组件资料中无任何消费端调用；Vantage 只读 `PasswordState`）。以下为**公开生态惯例**（联想 WMI 密码接口在社区文档/开源工具中的通用约定），实现者采用前应在实机验证：

- 设置/修改 supervisor 密码：`Parameter = "<新密码>,ascii,us"` —— 三段英文逗号：明文密码 + 编码（`ascii`）+ 键盘布局（`us`）。与之对应，`SupportedEncodings=1` ↔ `ascii`、`SupportedKeyboard=1` ↔ `us`（数值→关键字映射 [推断]）。
- 清除密码：`Parameter = ",ascii,us"`（空密码段）。
- 若已设密码需要先验旧密码，部分实现要求 `Parameter = "<旧密码>,<新密码>,ascii,us"` 或分两次调用（先验证后修改）[推断]——此细节必须实机验证。
- 成功判定：out `Return` 为 `"Success"`（与 SetBiosSetting 相同的返回串约定 [推断]）。
- 本机 `PasswordMode=1` 对应"supervisor 密码"（联想通常还有 HDD 密码模式，本机未暴露 [推断]）。

> ⚠️ **净室提示**：§5.3 全部内容属于 [推断]，且是**唯一**可能造成访问锁死风险的功能（§8.2）。实现者在本机验证前，应把 `password` 子命令标记为试验性（`--experimental` 门控）。

### 5.4 官方软件行为

- Vantage：只在 `Bios.Assistant` 契约里暴露 `CheckBiosPasswordSet`（读状态，用于 UI 提示），**无设置/清除 UI**。
- 快照：读 `Win32_ComputerSystem.AdminPasswordStatus` + `Lenovo_BiosPasswordSettings`（收集展示）。
- 结论：官方软件对密码接口"只读不写"；`lctrl bios password` 的写功能是超集扩展。

**证据**

- 类签名：`目标机 WMI 仓库实机采集` L394-399（`Lenovo_SetBiosPassword`）、L412-420（`Lenovo_BiosPasswordSettings`）。
- 本机属性实测：`目标机 WMI 实例实机采集` L56-70。
- 位域解码：`WmiAgent.cs::CheckBiosPasswordSet` L349-366。
- 无调用方：全部组件资料检索 `SetBiosPassword` 无命中。
- 交叉验证源：`BiosSecuritySettings.cs::CaptureSupervisorPassword` L52-66（`AdminPasswordStatus`）。

---

## 6. 生效语义

### 6.1 事务模型

联想 BIOS WMI 采用**显式提交/回滚**模型（与电池/背光等"即写即生效"通道不同）：

```text
SetBiosSetting("Name,Value;")   ← 写事务缓冲区（NVRAM staging）
   …多次 Set 累积…
SaveBiosSettings(";")           ← commit：全部写入 NVRAM
   or
DiscardBiosSettings(...)        ← rollback：丢弃本次缓冲区全部未提交更改
```

- 每次 `Set` 不落盘；`Save` 一次性提交**缓冲区里所有**未提交项。
- `Save` 之后的部分项（网络/虚拟化/显卡模式/启动项）**需要重启才真正生效**；官方证据：PCManager 的 `SetBIOSOC`"需重启生效"（`电脑管家电源组件内部接口说明` L144）。
- 官方代码序贯写多项时，任一项失败即**中止且不 Save**（§3.4）——防止"改坏一半"。

### 6.2 官方调用序列（`SetIOControlItem`，逐字节对照）

```text
foreach item in items:
    ret = SetBiosSetting(parameter = item.Name + "," + item.Value + ";")
    if ret != "Success": abort          # 不回滚、不提交
ret = SaveBiosSettings(parameter = ";")
success = (ret == "Success")
```

来源 `WmiAgent.cs` L225-261。这是实现者 `lctrl bios set` 的标准模板。

### 6.3 `Save` 与重启的关系

- `SaveBiosSettings` 返回 `"Success"` = NVRAM 写入成功，**不需要在 Save 后强制重启**（官方调用后不触发重启）。
- 生效时机由项决定：热键/IO 开关类（`IntegratedCamera` 等硬件断供）官方文档语境即插即用或下次设备枚举生效；CPU 相关（VT、OC）需重启。**lctrl 应在 save 成功后提示"部分更改需重启后生效"**。
- `LENOVO_GAMEZONE_DATA.SetBIOSOC` 等通过 BIOS 放行的 OC 项官方标注"需重启生效"（`电脑管家电源组件内部接口说明` L144）。

### 6.4 `Discard` / `LoadDefault`

- `DiscardBiosSettings`：丢弃 Set 缓冲、恢复为上次 Save 的状态（`Vantage 设备组件内部接口说明"回滚"）。
- `LoadDefaultSettings`：**恢复 BIOS 出厂默认**（危险项，§8.2）——会覆盖所有设置包括安全/密码相关项 [推断]（类语义如名，本样本无调用方，未实测）。
- 二者参数与返回约定同 Save（§3.2）。

### 6.5 `SetFunctionRequest`（用途未确认 【推断】）

- 类签名存在（`Lenovo_SetFunctionRequest.SetFunctionRequest`，`Lenovo_FunctionRequest` 提供 `CurrentSetting` 镜像），无任何官方消费端样本。
- 推断用途：BIOS 侧功能请求（如请求重启进 Setup / 触发胶囊更新 / 下次启动行为）。**实现者不应在 v1 暴露该命令**；如实现，先在一台可恢复的机器上验证。

**证据**

- 事务序列：`WmiAgent.cs::SetIOControlItem` L225-261。
- 提交/回滚描述：`Vantage 设备组件内部接口说明（"提交: SaveBiosSettings(Empty)；回滚: DiscardBiosSettings(Empty)"）。
- 重启语义：`电脑管家电源组件内部接口说明` L144（`SetBIOSOC`/`SetBIOSOCMode` 需重启生效）。
- 类签名（Save/Discard/LoadDefault/SetFunctionRequest/FunctionRequest）：`目标机 WMI 仓库实机采集` L374-393、L406-411。

---

## 7. 官方软件用到 BIOS 接口的功能清单

### 7.1 Vantage（LenovoProductivitySystemAddin, `Bios.Assistant` 契约）

| 功能 | BIOS 通道 | 具体项/参数 | 出处 |
|---|---|---|---|
| I/O 开关批量读写（系统设置页） | 读 `Lenovo_BiosSetting`；写 `Lenovo_SetBiosSetting` + Save | `_smbIOControlItems` 11 项 + `_tpeIOControlItems` 14 项 | `WmiAgent.cs` L28-41, L225-309 |
| Fn/Ctrl 互换 | `LENOVO_BIOS_ASSISTANT` Index 1 或 `FoolProofFnCtrl`/`FnAndCtrlKeySwap` | `Get/SetFnCtrlSwap` | `WmiAgent.cs` L301-309；`FunctionID.cs` L5 |
| SuperBoost（超频助燃） | `LENOVO_BIOS_ASSISTANT` Index 2 | `Get/SetSuperBoost` | `WmiAgent.cs` L312-320 |
| GPU 模式（独显/混合/自动/iGPU-only） | `Lenovo_BiosSetting.GraphicsDevice` + `GetBiosSelections("GraphicsDevice")` + `GAMEZONE_DATA.SetIGPUModeStatus` + BA Index 3 | `Set/GetGPUMode`、`Set/GetAdvancedGPUMode`、`AdvancedGPUCapbility` | `WmiAgent.cs` L69-223 |
| BIOS 能力探测 | `LENOVO_BIOS_ASSISTANT.GetCapabilityValue` | 位域 §1.3，本机 `0x80000008` | `WmiAgent.cs` L323-345 |
| supervisor 密码状态 | `Lenovo_BiosPasswordSettings.PasswordState` | bit1 | `WmiAgent.cs` L349-366 |

### 7.2 Vantage 设备设置页（键盘/隐私，vantage-device.md）

| 功能 | BIOS 通道 | 项 | 出处 |
|---|---|---|---|
| 热键模式 | `Lenovo_SetBiosSetting` | `"HotkeyMode,Disable"`（F1-F12 直通） | vantage-device.md §3.2 |
| F 键主功能 | 同上 | `"F1-F12AsPrimaryFunction,Enable"` | vantage-device.md §3.2 |
| Fn/Ctrl 防误触 | 同上 | `"FoolProofFnCtrl,Enable"` | vantage-device.md §3.1 |
| **FnLock 默认值** | 读 `Lenovo_BiosSetting` | 本机默认 `F1-F12AsPrimaryFunction,Enable` + `HotkeyMode,Disable` ⇒ **FnLock 默认锁定（F1-F12 即标准功能键）**；Vantage UI 的 FnLock 开关即映射此组合 | vantage-device.md §3.2；wmi-instances2.txt L18-19, L51-52 |
| 摄像头硬断 | `SetBiosSetting("IntegratedCamera,Disable")` | `ISET_40` | vantage-device.md §6.1 |
| 麦克风硬断 | `SetBiosSetting("Microphone,Disable")` | `ISET_41` | vantage-device.md §6.2 |
| 指纹硬断 | `SetBiosSetting("FingerprintReader,Disable")` | `ISET_42` | vantage-device.md §6.3 |

### 7.3 电源页（vantage-power.md）

| 功能 | BIOS 通道 | 说明 | 出处 |
|---|---|---|---|
| **AlwaysOnUSB** | `Lenovo_BiosSetting.AlwaysOnUSB`（默认 `Enable`） | BI 配置；**运行时覆盖走 `\\.\EnergyDrv` IOCTL（重启后回归 BIOS 配置）** | vantage-power.md §7.4；wmi-instances2.txt L20 |
| **ChargeInBatteryMode** | `Lenovo_BiosSetting.ChargeInBatteryMode`（默认 `Disable`） | 电池时 USB 充电；同上分层 | vantage-power.md §7.4；wmi-instances2.txt L21 |
| 电池页 BIOS 静态信息 | `Lenovo_BiosSetting`/`BiosPasswordSettings`/`BiosSelections` | Bios 版本、两个 USB 充电项读取 | vantage-power.md L43；[推断] 具体读取点在 `BatteryInformationHandler`/`BatteryAgent`（本样本未直接定位到 Lenovo_* 调用） |

### 7.4 快照诊断（LenovoSnapshotAddin / CDAT）

| 功能 | 通道 | 项 | 出处 |
|---|---|---|---|
| 全量 BIOS 设置采集 | `SELECT * FROM Lenovo_BiosSetting` → `CurrentSetting` | 全部项 + 每项 Selections | `BiosSettings.cs` L59-118 |
| 安全项采集 | 同上 | `SecureBoot/DeviceGuard/BottomCoverTamperDetected/BlockSIDAuthentication/AMTControl/LockBIOSSettings/EnhancedWindowsBiometricSecurity` | `BiosSecuritySettings.cs` L79-129 |
| Dash 检测 | 同上 | `DashEnabled` | `PotentialIssuesPopulator.cs` L84-102 |

### 7.5 PCManager（pcmgr-power.md）

| 功能 | 通道 | 说明 | 出处 |
|---|---|---|---|
| BIOS 放行 OC | `SetBIOSOC`/`GetBIOSOCMode`（GAMEZONE）标注走 `Lenovo_SetBiosSetting` | 需重启生效 | pcmgr-power.md L144；[推断] 具体 SetBiosSetting key 未在样本中定位 |

### 7.6 Dispatcher 联动（hal-services.md）

- `LENOVO_DISPATCHER_EVENT.PowerLevel` 可能在 `SetBiosSetting` 某些 key（如 `EffectivePowerModeMaxPerformance`）触发固件侧状态机（`hal-services.md` L68-70，[推断]）。

**证据**

- §7.1/§7.2/§7.4 全部文件与符号见各小节"出处"列；关键源文件：`WmiAgent.cs`、`FunctionID.cs`、`GPUModeStatus.cs`、`BiosSettings.cs`、`BiosSecuritySettings.cs`、`PotentialIssuesPopulator.cs`。
- §7.3：`Vantage 电源组件内部接口说明（AOU/BIOS 配置）与 L43（静态信息读取）、`目标机 WMI 实例实机采集` L20-21。
- §7.5：`电脑管家电源组件内部接口说明` L144。
- §7.6：`Lenovo 系统服务组件内部接口说明` L68-70。

---

## 8. 风险与防护

### 8.1 通道本身的护栏

1. **只读镜像类不可写**：`Lenovo_BiosSetting` 只有属性，没有方法，天然防误写；写改造必须显式选择方法类。
2. **事务缓冲**：Set 不落盘、Save 才提交、Discard 可整体回滚——这就是官方设计的最大保护（§6.1）。
3. **失败即中止**：官方 `SetIOControlItem` 任一 Set 失败立即 break 且不 Save（§3.4）——防止"部分成功"的脏状态。
4. **BIOS Assistant 位域通道**：`LENOVO_BIOS_ASSISTANT` 只暴露 3 个功能索引（v1），且每次返回带 bit31 成功位，失败可预期。

### 8.2 Brick/锁死风险项（`lctrl bios set` 必须重点标注）

| 项 | 风险 | 后果 |
|---|---|---|
| `EFI-BootOrder` | 高 | 写入非法启动序可能导致无法引导操作系统 |
| `USBBoot`/`PXEBootToLAN`/`IPV4PXEFirst` | 中 | 误关后只能用原有启动路径；组合错误会卡在启动介质选择 |
| `SecureBoot` | 中 | 关闭后系统安全属性变化；开启但密钥不匹配会导致无法引导 |
| `SecureRollbackPrevention` | 高 | 关闭后 BIOS 可被回滚到旧版本（降低安全）；与 BIOS 更新流程冲突 |
| `Intel(R)VirtualizationTechnology`/`VT-d` | 低 | 误关影响虚拟化/沙箱软件，不易损坏但影响功能 |
| supervisor 密码（§5.3） | **高** | 设置后遗忘 = 安全启动项被锁；清除失败 = 需要硬件级恢复（短接/官方工具）。官方软件完全不提供该写入口，实现者暴露时必须加门控 |
| `LoadDefaultSettings` | 高 | 一键恢复出厂会重置**全部**设置（含安全/启动项/密码），执行前必须强确认 |
| `GraphicsDevice`/`SetIGPUModeStatus` | 中 | 切独显后部分机型在无外接显示时黑屏风险（官方对进程占用做了前置检查：`GPUProcessInfo` + 有前台进程占用即拒绝，`WmiAgent.cs` L153-169） |

### 8.3 官方的保护逻辑（实现者照抄的最低标准）

1. **UI 白名单 ≠ 通道白名单**：官方 UI 只向用户暴露白名单项（§4.1 两份清单），但 `SetIOControlItem` 对任意 `ItemName/ItemValue` 直接透传——**通道本身不过滤**，lctrl 必须自己维护保护清单。
2. **进程占用检查（GPU 模式）**：从混合模式切 iGPU-only 前检查前台进程列表，非空则拒绝（`WmiAgent.cs` L153-169）。
3. **不自动重启**：Save 成功提示"需重启"，但绝不代用户重启（官方亦然）。
4. **只读密码**：官方从不在软件里写密码（§5.4）。
5. **枚举容错**：查询/枚举单项失败不中断整体（§4.1）。

### 8.4 lctrl 建议的安全策略

- 高危项（§8.2 表"高"）要求 `--yes` 双确认；`defaults`/`password` 子命令加 `--experimental`/交互确认。
- `set` 失败后自动丢弃缓冲区（调 `DiscardBiosSettings`），保持"要么全成功要么无变更"。
- `set` 前对值做存在性校验（`GetBiosSelections` 或至少与枚举清单比对；枚举不到的项拒绝裸写，除非 `--force`）。
- 写前快照当前值，`save` 失败时给出可复原的旧值报告。

**证据**

- 通道护栏/中止逻辑：`WmiAgent.cs` L225-261。
- GPU 模式进程保护：`WmiAgent.cs` L135-189（`SetGPUMode` 中 `GPUProcessInfo(processList)` 判定）。
- 白名单文件位置：`WmiAgent.cs` L28-41。
- 密码无调用方：§5 证据（全树检索）。
- 风险项清单来源：§4.2 表（本机枚举）与官方消费端引用。

---

## 9. `lctrl bios` 子命令设计建议

```text
lctrl bios list [--json]            # 枚举全部项 + 当前值 (+ 每项 Selections, 带 --selections 开关)
lctrl bios get <Item>               # 输出 "Name,Value" 或单值
lctrl bios set <Item> <Value> [--save] [--yes]
lctrl bios save                     # 提交当前缓冲
lctrl bios discard                  # 回滚当前缓冲
lctrl bios defaults                 # 恢复出厂 (--yes 强确认, --experimental)
lctrl bios password status          # 读 PasswordState (bit1)
lctrl bios password set|clear <new> # 写密码 (--experimental, 需实机验证 §5.3)
```

### 9.1 实现要点

1. **值域校验**：`set` 先 `GetBiosSelections(Item)`；失败/无 Selections 时退回枚举清单比对；仍未知 → 拒绝或 `--force`。
2. **大小写与拼写差异**：不做规范化，`set` 原样传值；提示用户以 `list --selections` 输出为准（`Enabled/Enable` 混用）。
3. **项名转义**：项名含括号（`Thunderbolt(TM)`）与逗号（理论上），按原始串处理；CLI 解析用"第一逗号前=名，其后=值"。
4. **缓冲语义**：`set` 默认**不 Save**，可连发多个 `set` 后一次 `save`（与官方一致）；`discard` 丢弃全部未提交项。
5. **退出码**：`0` 成功；`2` 参数/项不存在；`3` 权限不足；`4` BIOS 返回非 `Success`（stderr 打印原样错误串）；`5` 该机型不支持（WBEM 0x80041010/0F）。
6. **幂等与重试**：Set 重试 1 次仍非 `Success` 即失败；不做自动回滚（由用户显式 `discard`），但**失败后不 Save**（官方同款）。
7. **输出**：`--json` 输出 `[{name, value, selections?}]`，`--json` 失败时退出码仍按上述约定，JSON 写入 stderr 的错误对象。
8. Linux 后端：读侧可用 `/sys/firmware/dmi/tables/smbios`（只读，非本接口）；写侧无对等通道，命令应明确报"仅 Windows"（见 09 文档）。

### 9.2 与其它模块的协同

- 键盘 FnLock 开关（04 文档）：读写 `HotkeyMode`/`F1-F12AsPrimaryFunction` 时**两个项一起改并一次 Save**，保持状态一致。
- 隐私开关（04 文档）：`IntegratedCamera`/`Microphone` 硬断与 EC Privacy Guard（`SetPrivacyGuardEnabled`）是两层，勿混用语义。
- 电源页（02 文档）：AlwaysOnUSB 的**运行态**写 EnergyDrv IOCTL，**开机默认**写本接口——lctrl 两个子命令应共享同一抽象并注明层。

---

## 10. 未决项与开放问题（实现前需实机验证）

1. `Lenovo_SetBiosPassword` 参数编码（含旧密码验证格式）——本项目样本无调用方，§5.3 全为 [推断]。
2. `DiscardBiosSettings`/`LoadDefaultSettings` 的 `parameter` 精确取值（";" vs 空串）——未实测。
3. `BA` GPUMode（Index 3）`ValueData` `"01"`/`"02"` 的确切语义——仅记录官方调用模式。
4. `SetFunctionRequest` 的用途。
5. 各方法类（Set/Save/…)实例有多个时，是否不同实例绑定不同项——官方只取第一个，未验证差异。
6. `LENOVO_REPORT_DIRECT_BIOS_DATA`（`default_num`/`DependOnID`/`Step`）与 BIOS 项的对应关系未解码。
7. ISET 索引 ≥12 的实例号（vantage-device.md 记录，未在原采集复核）。
8. 密码设置后 `PasswordMode`/`PasswordState` 两口的值变化规律。

> 这些未决项不影响 `list/get/set/save/discard/defaults` 的实现（均有官方对照）；仅 `password` 的**写**方向受影响。

---

## 附录 A · 证据索引汇总

| 声称 | 证据位置 |
|---|---|
| 10 类 WMI 签名/属性 | `目标机 WMI 仓库实机采集` L367-426、L473-484、L610；`目标机 WMI 实例实机采集` L235-299 |
| `CurrentSetting` 逗号格式与全量键值 | `目标机 WMI 实例实机采集` L1-55；`WmiAgent.cs` L263-309；`BiosSettings.cs` L78-82 |
| Set/Save 编码与 "Success" 判定 | `WmiAgent.cs` L225-261（`SetIOControlItem`） |
| WMI 调用骨架（CIM/实例路径/out 取值） | `WmiProvider.cs`（`CallMethod`/`QueryData`） |
| BA 位域解码 | `WmiAgent.cs` L69-95（AdvancedGPUCapbility）、L323-345（GetBiosCapability）、L403-456（Get/SetAssistantValue）；`FunctionID.cs`；`BiosCapability.cs`；`GPUModeStatus.cs` |
| `BA.GetCapabilityValue=0x80000008` | `目标机 WMI 实例实机采集` L193 |
| GraphicsDevice 三值与模式 | `WmiAgent.cs` L96-121、L190-223 |
| 密码类属性与状态 | `目标机 WMI 实例实机采集` L56-70；`WmiAgent.cs` L349-366（bit1）；`BiosSecuritySettings.cs` L52-66（AdminPasswordStatus） |
| 安全/诊断项名 | `BiosSecuritySettings.cs` L79-129；`PotentialIssuesPopulator.cs` L84-102 |
| AlwaysOnUSB/ChargeInBatteryMode | `Vantage 电源组件内部接口说明；`目标机 WMI 实例实机采集` L20-21 |
| 键盘项与 ISET 索引 | `Vantage 设备组件内部接口说明、§3.2、§6.1-6.3 |
| FnLock 默认（F1-F12 主功能） | `目标机 WMI 实例实机采集` L18-19、L51-52；`Vantage 设备组件内部接口说明 |
| SetBIOSOC 需重启 | `电脑管家电源组件内部接口说明` L144 |
| Dispatcher 联动 key [推断] | `Lenovo 系统服务组件内部接口说明` L68-70 |
| 容错 HRESULT 0x80041010/0F | `LenovoBiosWmiInterface.cs`（GetCapability/GetFeatureValue catch） |
| WBEM 异常语义 | 官方代码意图（类/对象不存在→缓存跳过），本文档 §2.4 |