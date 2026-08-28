# HANDOFF · vantage 净室实现交接文档

> 致实现者(GPT 5.6 Sol):本包是 **Lenovo 21VG(ThinkBook 14+ 2026,Panther Lake)硬件控制中心**
> 的完整净室接口规格。你的任务:用 Rust 实现跨平台(Windows/Linux)CLI `vantage`,
> 覆盖 Lenovo Vantage + 联想电脑管家 + MagiCenter 的全部硬件控制功能,
> 并为 Panther Lake 低功耗调优提供一等支持。

## 1. 先读这三份

1. **`docs/00-cleanroom-charter.md`** — 净室属性声明、法律边界、文档地图、全局技术事实。
   **必读**。本包全部信息来自:产品内部接口资料、目标机实机只读验证、公开规范(Intel ESIF SDK /
   USB-IF MBIM / Linux 内核文档)。**不要**获取或分析任何 Lenovo/Intel 二进制;文档自足。
2. **`docs/08-architecture.md`** — Rust 架构设计(命令树、daemon 取舍、错误模型、crate 划分)。
3. **`docs/B-evidence.md`** — 每条关键结论的来源分类与验证方式、未决项清单。

## 2. 建议实现顺序

| 阶段 | 文档 | 产出 |
|---|---|---|
| 0 | 00, 08 | 工程骨架、cargo workspace、错误模型 |
| 1 | 01(HAL), 附录 A | Windows 后端:WMI/IOCTL/设备接口封装 |
| 2 | 02(电源电池), 03(散热性能) | P0 功能:性能模式、充电模式、风扇、电池信息 |
| 3 | 04(外设), 05(BIOS) | 背光/Fn 键/面板/摄像头/BIOS 设置 |
| 4 | 09(Linux 后端) | Linux 平台层(ideapad_laptop/sysfs/RAPL) |
| 5 | 07(调优), 10(DSL) | Panther Lake 调优 profile 与配置 DSL |
| 6 | 06(MagicBay), 11(更新/诊断) | P1/P2 功能收尾 |

## 3. 标注约定(全套文档统一)

- **[实机验证]** — 在目标机上实测确认的行为,可直接依赖。
- **[推断]** — 由组件资料/公开规范交叉推得,语义未实机确认;首次使用前按文档给出的方法复核。
- 未标注的接口事实(类签名、IOCTL 码、常量、GUID)来自产品内部接口资料,并经实机抽样印证。

## 4. 已证实的"死路"(不要重复探索 — 均为 2026-08-27 实机结论)

1. **ring0 MSR 读写不可行**:目标机 VBS/HVCI 启用,易受攻击驱动黑名单生效
   (WinRing0x64 加载 err=183)。Windows 侧 PL 控制走 EC 模式字节 / ITS 契约,不走 MSR(07 §3.1)。
2. **WMI `LENOVO_GAMEZONE_DATA` 方法族固件未实现**:全部方法返回 `Invalid object`;
   本机 BIOS 的 GMZN 作用域只有数据块(GameZone 为 Legion 特性)。模式切换走
   `LenovoProcessManagement` SCM 控制消息或 ITS 服务契约(03 §2.3 勘误)。
3. **任意百分比充电阈值写入无通道**:官方阈值写终端(`ioctl 0x24058`)目标设备本机不存在;
   可用面 = GBMD 养护/快充/存储 + 模式写 `0x83102134 {0,1,9}`(02 §4.2)。
4. **ESIF IPC 管道(`\\.\pipe\DttServerPipe`)是 IPF-EF 会话/状态通道**,随驱动发行的
   `IpcClient.dll` R0 接口无 verb 执行面;DPTF 遥测事件流无公开解码描述(07 §3.1.1)。
5. **SCM 控制码"调用成功 ≠ 语义生效"**:Dispatcher 对 `0x80..0x8F` 全部返回成功,
   未识别码被静默吞掉;语义判定必须依赖可观测遥测(03 §2.3)。

## 5. 未决项(仅一项)

- **U6 · `Lenovo_SetBiosPassword` 参数编码**(05 §5.3):类签名存在但无任何官方组件调用它;
  仅公开生态惯例可参考。⚠️ 实现时置于 `--experimental` 门控后,**勿在主力机实测**,
  备好 BIOS 恢复手段。除此项外全部闭环(见 `docs/B-evidence.md` §3)。

## 6. 安全边界(实现时必须保留的保护)

- 写 BIOS 设置:逐项确认 `Lenovo_SetBiosSetting` 返回串语义(05 §3),失败不盲目重试。
- EC/GBMD 写:仅使用文档列出的已验证命令字;未知命令字一律先只读探测(01 §3.3 方法论)。
- PL 写入:PL1 ≤ PL2;写前读默认值记录以便恢复;PL1 低于 ~7W 显著掉速(07 §3.1)。
- 风扇表写:先 `Fan_Get_Table` 备份(03 §4)。
- 互斥:Vantage / PCManager 与 `vantage` 同时控制同一通道会冲突,启动检测与提示逻辑见 03 §1。

## 7. 验收口径(规格组建议)

- P0 功能(00 §5.4)在目标机 Windows 侧全部可用且有回读验证;
  Linux 侧按 09 文档映射达到"可用/受限/不可用"的如实标注。
- 每个写操作具备:干跑(`--dry-run`)、回读确认、失败语义化报错(08 §6)。
- 配置 DSL(10)能表达 07 §5 的全部内置 profile。
- 不包含官方软件中的应用商店/广告/账号/遥测(00 §1 排除范围)。

## 8. 问题回传

若某接口描述不清或与实测不符:**不要**转而分析厂商二进制;将具体现象
(调用序列、返回码、期望/实际)回传规格组,由规格组补充实机验证后更新文档。
