# 附录 B · 来源与验证说明

> 本文档说明全套规格(00-11 + 附录 A)的信息来源分类、关键结论的验证方式、
> 以及截至 2026-08-27 的未决项状态。供实现者评估每条结论的可信度。

## 1. 来源分类

全套规格的信息仅来自以下三类来源:

| 类别 | 说明 | 可信度 |
|---|---|---|
| **S1 · 产品内部接口资料** | Lenovo Vantage / 电脑管家(PCManager)/ MagiCenter 各组件随产品发行的内部接口说明、组件字符串资源、INF/注册表参数、组件常量表 | 高(组件级一手资料) |
| **S2 · 目标机实机只读验证** | 在目标机(Lenovo 21VG,ThinkBook 14+ 2026,Panther Lake)上以只读方式枚举/调用公开管理接口(WMI、设备接口、服务控制、命名管道)得到的实测数据 | 最高(实机行为) |
| **S3 · 公开资料** | Intel 公开 ESIF SDK(intel/dptf,Apache-2.0)、MBIM/USB-UVC/DisplayLink 公开标准、Microsoft WDF/WMI 公开文档、Linux 内核文档(ideapad-laptop、intel_rapl 等) | 高(公开规范) |

标注约定(全套文档统一):
- **[实机验证]**:S2 直接观测,可复现。
- **[推断]**:由 S1/S3 交叉推得、尚未实机确认的语义解释;首次使用前应按文档给出的方法复核。
- 未标注的接口事实(类签名、IOCTL 码、常量表、GUID)来自 S1 组件资料,与 S2 抽样一致。

## 2. 关键结论 → 来源映射(高风险项优先)

| # | 结论(docs 位置) | 来源 | 验证方式 |
|---|---|---|---|
| E1 | GBMD `0x831020f8` 子命令表:3/5/0x0d/0x0f/7/8/0xff(02 §3.2) | PowerBattery 组件六条写入路径接口资料 + CDriverLib 封装规格 | 组件接口表定位,常量 `0x831020f8` 与子命令字节经全部调用点交叉确认 |
| E2 | IOCTL 全表 0x831020e8/f4/f8/2120/212c/2138/214b/214c/2150/215c(02 §9) | 同 E1 + PowerBattery 组件接口说明 | 常量→函数归属逐条人工确认 |
| E3 | AcpiVpc 会话/事件协议与 IOCTL 族(01 §3) | Lenovo 驱动组件内部接口说明 | 驱动组件接口行为分析 + 字符串资源交叉印证 |
| E4 | WMI 65 类签名(附录 A) | 目标机 WMI 仓库实机采集(Get-CimClass) | 实机采集脚本可重跑复现 |
| E5 | 性能模式 ITS/Dispatcher 通道与版本能力矩阵(03 §2) | Vantage/电脑管家电源组件内部接口说明 | 合约枚举 + 注册表键 + 服务控制消息符号三重交叉 |
| E6 | BIOS 接口参数编码 `Name,Value;`/Save `;`/`Success`(05 §3) | Lenovo 系统服务组件内部接口说明 + WmiAgent 组件资料 | 组件级行为分析确认 |
| E7 | `LENOVO_SETTING_*` 感知命令字 65793-1966337(04 §5.1) | Vantage SmartPerformance 组件内部接口说明(IntelligentSensingPipe 常量表) | 组件常量直接提取 |
| E8 | DPTF/IPF 参与者拓扑(07 §1) | 目标机 MagicBay/DPTF 实机枚举数据(Get-PnpDevice) | 实机枚举,非推断 |
| E9 | PCManager `_DT` DTT 直调方法族(07 §2.2) | 电脑管家电源组件内部接口说明(GameSettingsPlugin 组件字符串资源) | 符号名 + 调用链推断(调用链标 [推断]) |
| E10 | DBDC 三档限流 [7500/5000/4500]@[100/40/20](07 §2.3) | 目标机 WMI 实例实机采集 `LENOVO_REPORT_DBDC_DATA` | 实机实例值;语义解释为 [推断] |
| E11 | MagicBay LTE = MBIM 标准类(06 §4) | 目标机实机枚举(cxwmbclass + MI_00 绑定) | 实机驱动绑定证据 |
| E12 | 39 Wh 电池禁用快充(02 §3.5) | Vantage 电源组件内部接口说明(`Is39whBatteryInstalled` 判定逻辑) | 组件行为分析 |
| E13 | 充电模式注册表持久化键(02 §3.4) | Vantage 电源组件内部接口说明(BatteryAgent 组件) | 组件行为分析 |
| E14 | Dolby profile 映射 Movie=0…Off=6(04 §6.1) | Vantage SmartPerformance 组件内部接口说明(`_dolbyMap` 常量表) | 组件常量提取 |
| E15 | 降噪模式值 0/1/2/3/4/10(04 §6.2) | Vantage SmartPerformance 组件内部接口说明 | 组件常量提取 + DispatcherConfig.xml |
| E16 | EnergyDrv 扩展码表 0x831020c0/c4/e8/2120/212c/2130/2134/2138/213c/2150/215c + GBMD 0xFF 语义 + 83B 电池结构 + 通用 GET cmd 支持表(01 §3.3) | 目标机实机接口探测记录 + AdapterInformation/pcm-cli/BatterySetting/WrapPlugin 组件资料 | 实机只读接口调用验证(2026-08-27);全部只读,无写入 |

## 3. 未决项状态(2026-08-27 实机复核后)

### 3.1 已闭环(实机验证)

| # | 项 | 结论 | 来源 |
|---|---|---|---|
| U2 | GetAdapterStatus 通道 | **解决**:适配器状态 = GBMD 子命令 `0xFF`(bit24 能力 / bit15-16 类型枚举);详情 = GAPD `0x8310215c`(10B: PID/VID/SysW/CurW),bit24=0 时 err=87 | AdapterInformation 接口规格 + 实机 `0x00860004`;目标机实机接口探测记录 |
| U3 | DTT WMI 类名 | **解决(否定结论)**:`root\WMI` 无任何 Intel/DTT 类;DTT 控制面 = ACPI INTC10D4/INTC10D5 + `ipfsvc.exe` ESIF IPC;`_DT*` 仅为 PCManager 契约名 | 实机全量类枚举;07 §2.2 已更正 |
| U7(部分) | UTILITY_DATA 调用约定 | **解决**:实例方法(非静态);`GetIfSupportOrVersion` 返回版本号;本机支持表 1→v3/3→v2/4→v2;featuretype↔隐私功能映射仍 [推断] | 实机 Invoke-CimMethod;目标机实机接口探测记录 |
| U1(部分) | 阈值读通道 | **解决**:`0x83102138`(83B 电池结构,完整解析)、`0x83102120`(20B 全局配置)、通用 GET `0x831020c4`(cmd 0-24 支持表测绘);`Lenovo_BatteryInformation` 确认只读无方法 | 实机探测;01 §3.3 / 02 §4.2 |

### 3.2 二轮/三轮实机复核结果(2026-08-27);仅剩 U6 未决

| # | 未决项 | 位置 | 结论/建议验证方法 |
|---|---|---|---|
| U1b | 自定义阈值**写**命令字(终端 cmd) | 02 §4.2 | **已闭环(否定结论)**:ThinkPowerPlugin 阈值写终端为 `ioctl 0x24058`(目标设备接口本机不存在,旧 ThinkPad 栈遗留);SET `0x831020c0 {6,1,x}` 写入 err=0 但无任何可观测状态变化;`C:\config.ini` 不存在。本机无可操作的任意百分比阈值写通道,可用面 = GBMD 养护/快充/存储 + `0x83102134` 模式 |
| U4 | 07 §5 各 profile 的 PL1/PL2 瓦数 | 07 §5 | **已收敛**:三条独立读 PL 通道全部实证不可用——(a) ring0 MSR 被 HVCI 黑名单封锁(U5);(b) ESIF IPC 管道为 IPF-EF 会话/状态通道,R0 客户端无 verb 执行面,JSON 会话协议已探明(07 §3.1.1),verb 请求未证实;(c) DPTF 遥测事件流无可用的公开解码描述(实机采集 7k+ 事件验证)。配方默认值维持 [推断],按 07 §5 `tune telemetry` 流程实测校准;WMI `LENOVO_GAMEZONE_DATA` 方法族确认为固件未实现(03 §2.3 勘误),不可作为替代 |
| U5 | MSR 写是否被 EC 周期覆写 | 07 §3.1 | **已闭环(实机)**:VBS/HVCI 生效,WinRing0x64 被黑名单拒绝(err=183),ring0 MSR 通道在本安全基线下封闭;DPTF 持续重编程 RAPL,直写无意义。结论:Windows 侧不依赖 MSR |
| U6 | SetBiosPassword 编码格式 | 05 §5.3 | ⚠️ **勿在主力机实测**;实机 `--experimental` 门控验证(05 §10),备好 BIOS 恢复手段 |

## 4. 验证环境

- 目标机:Lenovo 21VG(ThinkBook 14+ 2026,Intel Panther Lake-H),Windows 11 24H2,
  VBS/HVCI 启用;官方软件栈:Lenovo Vantage 10.2606.12.0、电脑管家 5.1.200.8201、
  MagiCenter、VantageService 5.1.2607.5、DPTF 9.1.10003.555(ipfsvc 2.3.20300.4810)。
- 验证均在普通用户/管理员权限下以只读或官方已暴露的管理接口完成;
  涉及写入的探测均在文档中逐项标注其安全边界(返回码语义、是否可逆)。
- 附录 A 可由目标机实机重新采集生成(`Get-CimClass -Namespace root\WMI` 全量导出)。
