# 能力矩阵终态与通道穷举记录 — 2026-11-04

目标机：TB14P-EZRA（ThinkBook 14 G8+，21VG，Panther Lake，固件 SWCN22WW）。

## 1. 终态矩阵

`vantage capability` 在目标机输出 134 行，汇总：

| 状态 | 行数 |
|---|---|
| available | 89 |
| unsupported | 33 |
| limited | 7 |
| unavailable | 1 |
| external | 4 |

limited 全部为复合项（`perf.mode`、`tune.profile.apply/restore`、`snapshot.capture/diff/restore`、
`magicbay.detect`）：其成员集合含真实死通道，取成员最小状态是正确结论。
unavailable 一项为 `tune.pl1`。

## 2. 本轮补齐的可用行

- `tune.epp` / `tune.turbo`：由 `power.saver-once` 的实机写入—回读—恢复链证明
  （PERFEPP AC/DC 接受 100，PERFBOOSTMODE 接受关闭并复原到索引 2）。此前的
  `Limited` 来自过时的命令期探测，已废弃。

## 3. 通道穷举与死因

每一行为目标机实测，括号内为拒绝码。

| 通道 | 结果 |
|---|---|
| `root\WMI` `LENOVO_*` / `GAMEZONE` / `FAN` / `PANEL` / `UTILITY` / `OTHER` / `SR` 方法 | 仓储调用 `0x8004102f`；内核 WMI `rc=87`；`wmic` 无输出 |
| 同上，实例绑定调用 | 可 dispatch，但 `Get*` 返回 `无效的对象`，`Set*` 无物理效果（见 §4） |
| 内核 WMI `WmiExecuteMethod`（NULL/具名实例/0·WNODE·原始缓冲/四种方法号） | `rc=87`；具名实例时 `rc=5` 且 `needed=0` |
| `EsifExecutePrimitive`（methodId 1..32000 × instanceId 0/1/2/10/100 × 多种 inputData） | 无效参数 |
| `ipfsrv` IPC：命名管道 `ipfsrv.dptf` / `.public` / `.elevated`、TCP 18086、`\\.\IpcClient` | 全部不存在；`ipfumdf` 未绑定任何设备对象 |
| PDO 直开 + `IOCTL_ACPI_EVAL_METHOD`：`ipf_cpu`、`ipf_acpi`(IETM/SEN1-5/CHRG/TPWR)、`ACPI_HAL`、`GMZN`、`DSARDEV` | `err=1`；厂商 IOCTL 全族静默拒绝 |
| `lenovoDriverBus`（`ROOT\LENOVO_VIRTUAL_BUS\0000`） | 可开，全部 ioctl 拒绝，`ret=0` |
| `EnergyDrv` `0x83102150` 选择子 0x10–0x40 与 0x800000xx | 仅 `0x11`/`0x12`/`0x80000011`/`0x80000012` 为电池 GBMD，其余回显 `0x11`（不支持） |
| `Lenovo_BiosSetting`（ISET）、`DiscardBiosSettings`、`SetBiosSetting` | 通道可用（Discard→Success），但 `SetBiosSetting` 对每个设置项均返回 "Not Supported"（平台级写禁用） |
| IGCL PIXTX | `0xC0000374` 堆损坏，5 次 4 崩 |
| `MSAcpi`、`Win32_Fan`、`Win32_TemperatureProbe`、`Intel_ME`、`SetFunctionRequest` | 抽象 / 空 / 仅 AMT / 全拒绝 |

## 4. 写入通道的物理效果判定

`SetKeyboardLight` 以 0/1/2/3 四档、每档 8 秒间隔下发，全部 dispatch 成功，
键盘背光无任何物理变化。`SetTPStatus` 下发后触控板仍为 OK。
结论：`Set*` 系列为无 ACPI 处理器的空实现，不能作为可用能力。
对应的 `Get*`（`GetKeyboardLight`/`GetTPStatus`/`GetSmartFanMode`）返回 `无效的对象`，
与固件未加载 Legion `GMZN`/`FAN`/`PANEL` 对象一致。

## 5. 结论

目标机固件为 `INTC10D4\IETM` 形态，未提供风扇/键盘/面板写、性能功耗墙所需的
ACPI 对象与其 WMI 处理器。其余非 available 行由硬件缺失、策略边界或
已记录的拒绝收据限定，不是实现缺口。
