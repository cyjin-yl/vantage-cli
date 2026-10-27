# 06 · MagicBay 磁吸配件协议规范

> 读者:**实现者**(用 Rust 重写 Lenovo 硬件控制中心的 LLM)。
> 作者:接口规格组。
> 状态:v1 · 2026-08-27。
> 依赖:见 00(术语/通道)、01(HAL 接口表)、04(摄像头/麦克风隐私开关)、07(DPTF/热管理)、08(架构)、09(Linux 后端)。

---

## 1. 范围与边界

### 1.1 本规范覆盖

MagicBay 是联想面向 ThinkBook / IdeaPad 系列的**磁吸扩展背壳**产品线,
通过 USB 3.0 物理接口挂载到主机,内含一颗复合 USB 设备(以及可选的 eDP/DP 显示桥接芯片)。
本规范描述实现者如何用 Rust 实现下列 CLI 子命令:

```
vantage magicbay detect
vantage magicbay lte status        # 查询当前 LTE 数据/信号/APN/SIM 状态
vantage magicbay lte connect [apn] # 建立数据会话
vantage magicbay lte disconnect     # 断开数据会话
vantage magicbay cam [status|set <prop=value>]   # MagicBay 摄像头(UVC)
vantage magicbay display [detect|status|set-mode <WxH@R>]  # 扩展屏检测与模式切换
```

### 1.2 净室范围

- **要做**:设备检测、配件识别、LTE 会话状态与开关(走标准 MBIM/MBN 或 ModemManager 路径)、
  摄像头 UVC 控制、扩展屏检测与 EDID/模式查询、热插拔事件监听。
- **不要做**:不复制 MagiCenter 的插件分发(Plugins.xml / 云端下载 / Lenovo 签名校验 / sudo-prompt 提权)、
  不做 OTA 固件升级、不做 LUDP 埋点、不做 OAuth 认证、不做桌面挂件/剪贴板。

### 1.3 术语

| 术语 | 含义 |
|------|------|
| MagicBay | 联想磁吸背壳扩展接口品牌名;物理层为 USB 3.0,逻辑层为一颗 USB 复合设备 |
| MBIM | Mobile Broadband Interface Model(USB 类 0x0E/Network Control Model 的一个 Profile) |
| MBN | Windows Mobile Broadband API,操作系统级 LTE/数据连接管理 |
| UVC | USB Video Class,标准摄像头 |
| QDU | Qualcomm QDU1000 / QFE7260 系列 eDP→DP 桥接芯片 |
| Chipidea | 芯片厂商,提供 MagicBay 内的 USB 功能控制器与 Role-Switch 硬件 |
| DPTF / IPF | Intel 热/功耗框架,扩展屏插入会经此影响热策略 |
| PluginKey | MagiCenter 内部为每个配件类型分配的插件名(`lte2-plugin` 等) |

---

## 2. 硬件拓扑

### 2.1 已知的 MagicBay 配件型号

MagiCenter 主包内存在 `KNOWN_DEVICES` 常量(bundle 行 103–125),穷尽枚举了
**三个**已被联想生态支持过的 MagicBay 配件型号:

| pluginKey       | deviceName | USB VID | USB PID (dec / hex)  | 语义 |
|-----------------|-----------|---------|----------------------|------|
| `tiko-plugin`   | `tiko`    | 0x17EF  | 25253 / 0x62B5       | 早期 4G/LTE 备用型号(2022 前后) |
| `lte2-plugin`   | `lte`     | 0x17EF  | 28677 / **0x7005**   | 当前主流 MagicBay LTE 模块 |
| `hud-plugin`    | `hud`     | 0x17EF  | 4375 / **0x1117**    | MagicBay HUD / 配件 |

> 实现者**必须**内置该常量表(用于检测 + 识别),但不必保留 MagiCenter 的
> `pluginKey` 语义;可映射为本项目自己的 `MagicBayKind` 枚举:
> `TikoLTE`、`LTE2`、`HUD`。

### 2.2 复合设备的接口划分(PID 0x7005)

PID 0x7005 是复合 USB 设备,在 Windows 上由 `usbccgp` (USB Composite Class
Parent) 拆成子接口。**已在目标机上实测枚举到两个子接口**:

| 子接口 InstanceId | Class | Service | 功能 |
|-------------------|-------|---------|------|
| `USB\VID_17EF&PID_7005&MI_00\6&...&0000` | `Net` | `cxwmbclass` | **LTE (MBIM 类, 标准 Mobile Broadband)** |
| `USB\VID_17EF&PID_7005\20080600...`      | `USB` | `usbccgp`   | 复合设备父节点(聚合点) |

**关键观察**:

1. `MI_00` 是**唯一一个已确认暴露给操作系统的功能接口**,由 Microsoft
   `netwmbclass.inf` (`wmbclass.ndi`) 驱动的 `cxwmbclass.sys` 承载。
   这证明 LTE 走的是 **USB 标准 MBIM 类**,不依赖任何联想私有驱动。
2. 规格组在目标机上**没有**观察到额外的 `MI_01`、`MI_02` 摄像头/显示接口;
   原因见 §4(摄像头)和 §5(扩展屏):它们在 Windows 侧**不是**由同一 USB
   复合设备枚举出来的,而是独立的硬件路径。
3. [推断] 硬件设计上,一个 MagicBay 背壳里**同一时间**只会枚举出其中
   一类功能(LTE / 摄像头 / 显示),由硬件跳线决定;MagiCenter 只负责 VID/PID
   匹配,不做 USB interface 协商。

### 2.3 摄像头配件 — 标准 UVC

- MagiCenter 主包内**没有** `camera-plugin`、`cam-plugin` 等条目,
  也没有任何针对摄像头的 VID/PID 检测逻辑(见 `MagiCenter 组件内部接口说明)。
- 如果 MagicBay 提供摄像头,它必然以**标准 UVC 设备**的形式被操作系统
  枚举(Microsoft 的 `uvcvideo` / 内核 `uvcvideo` 驱动),走 USB Class `Video`。
- 实现者**不需要** VID/PID 匹配:通过 `v4l2` / `libuvc` 列出所有视频设备即可。

### 2.4 扩展屏配件 — ACPI QDU 桥接,非 USB 复合接口

扩展屏**不是** USB 复合设备的某个 interface(见 §5 完整分析),
而是通过**ACPI 总线**枚举出来的独立显示桥接芯片,典型硬件 ID:

- `ACPI\QCOM2488` — Qualcomm QDU 系列 eDP→DP 显示桥接控制器
- `ACPI\QCOM24B7` — Chipidea USB Role-Switch

这两条设备记录在 Windows 上由 `ufxchipidea.inf` (`UfxChipidea.sys`)、
`urschipidea.inf` (`urschipidea.sys`) 匹配。显示部分则由 Intel/AMD 集成 GPU
的 eDP/DP 输出直驱,不经过 USB 总线。

### 2.5 拓扑总图

```
MagicBay 背壳
├── USB 3.0 物理连接器 ──┐
│                        ▼
│              USB 复合设备 (VID_17EF)
│              ┌─ MI_00 ──► cxwmbclass ──► MBIM (LTE)
│              │
│              └─ (可选其他 MI_xx, 未在 21VG 上观察到)
│
├── eDP/DP 连接器 ──────► QDU1000/QFE7260 桥接芯片 (ACPI\QCOM2488)
│                         │
│                         └──► GPU eDP/DP 输出 (主屏幕驱动栈)
│
├── 可选摄像头 ─────────► UVC 标准视频设备
│
└── 磁吸供电/机械检测 ──► EC / ACPI 事件
```

---

## 3. 热插拔与协商

### 3.1 Windows 侧(参考,不复制)

MagiCenter 的 USB 事件路径(来源:`winapi_addon.node`,bundle 行 16141–16178, 28115):

1. 应用启动时调用 `getAllDevices()` 初始化设备列表。
2. 调用 `startUSBListener(callback)` 订阅 `DEVICE_NOTIFY_ALL_INTERFACE_CLASSES`
   事件。
3. `handleUSBEvent({ event: "inserted"|"removed", ... })`:
   - 从 `path`(Windows DeviceInstanceID,形如
     `USB\VID_17EF&PID_7005\6&1a...`)中用正则 `/#([^#]+)#\{/` 抽取稳定 ID。
   - 用 `getKnownDeviceKey(device)` 遍历 `KNOWN_DEVICES` 表,
     将 `idVendor`/`idProduct` 匹配为 `lte2-plugin`/`hud-plugin`/`tiko-plugin`。
4. 插入时:
   - 若 `connectedDeviceIdentifier` 已存在且**实例 ID 不同** →
     提示"多个 MagicBay 设备接入时,只支持第一个",忽略后者。
   - 若实例 ID 相同(睡眠唤醒重枚举) → 只更新 `path`,不重复触发。
   - 检查 `Settings.deviceFirstConnection[pid${pid}]`:
     - `undefined` 或 `true`(首次) → 打开窗口 + 跳转介绍页。
     - `false`(已设) → 直接 `loadPlugin`。
5. 拔出时带**睡眠唤醒去抖**:
   - 若 `PowerStatusMonitor.shouldSuppressUsbDetach()` 为真,
     `scheduleSuppressedUsbDetachRecheck(+300 ms)` 复核;
     若 300 ms 后设备仍在,视为睡眠唤醒误报,不卸载。

### 3.2 实现者要做的检测(detect 子命令)

**实现者不复制 MagiCenter 的插件分发、LUDP 埋点、Prisma DB 状态**;
只需要实现以下可观察行为:

```
vantage magicbay detect
  → 枚举当前系统中所有已识别的 MagicBay 配件,输出 JSON
```

**算法**(平台无关,底层见 08 文档的 CLI 分层):

1. 枚举系统上所有 USB 设备(Windows: SetupDi + USB API;Linux: `sysfs` + `libusb`)。
2. 过滤 `VID == 0x17EF`。
3. 对照内置常量表(PID → kind),输出:

```json
{
  "devices": [
    {
      "bus": "usb",
      "path": "/sys/bus/usb/devices/1-1.2" | "USB\\VID_17EF&PID_7005\\...",
      "vid": "17EF",
      "pid": "7005",
      "kind": "lte2",
      "interfaces": ["mbim"],
      "attached": true
    }
  ],
  "acpi_devices": [
    { "hid": "QCOM2488", "kind": "display_bridge", "active": true },
    { "hid": "QCOM24B7", "kind": "usb_role_switch", "active": true }
  ]
}
```

### 3.3 持续监听(可选,daemon 侧)

若 08 文档选择实现常驻 daemon,应订阅:

- Linux: `udev` monitor (`udev_monitor_new_from_netlink`, `udev_monitor_filter_match_tag`)
  监听 `SUBSYSTEM=="usb"` + `DEVPATH` 含 `17EF` 的事件;
  同时监听 `SUBSYSTEM=="acpi"` 中 `QCOM24*` 的出现/消失。
- Windows: `RegisterDeviceNotification` (`DBT_DEVICEARRIVAL`/`DBT_DEVICEREMOVALCOMPLETE`),
  过滤器同 MagiCenter(`DEVICE_NOTIFY_ALL_INTERFACE_CLASSES`)。

---

## 4. LTE 控制

### 4.1 结论:走标准 MBIM,不依赖联想私有协议

**关键事实**:`MagiCenter/main_bundle.js` 全文搜索 `MBIM`/`wwan`/`dataSwitch`/`APN`/`SIM`
**均零命中**。唯一提到 `MBN` 的位置是错误分类:
`_isMbnError(message)` —— "Failed to initialize MBN API"(bundle 行 23433)。

这说明 MagiCenter 主体**不直接操作 MBIM**;
真正的 LTE 控制由 `lte2-plugin` 通过 Electron BrowserView 加载的 HTML/JS
页面调用 **Windows Mobile Broadband (MBN) API** 完成,
或交由操作系统内置的移动热点 UI。

### 4.2 MBIM 控制 CID 子集(MagicBay 已使用)

MagicBay LTE 模块(MI_00)暴露的 MBIM 通道,按 USB MBIM 规范(USB-IF
Mobile Broadband Interface Model R1.4),典型 CID 布局如下:

| CID | 名称         | 用途                                   | MagicBay 观察 |
|-----|-------------|----------------------------------------|---------------|
| 0x00| ECM/MBIM Mgmt | MBIM 控制信道(状态、链路管理)          | **已确认**(cxwmbclass 挂载即使用) |
| 0x01| ECM/MBIM Data  | 数据信道(IP 包)                       | **已确认**(数据会话走此) |
| 0x10| AT Commands  | 兼容 old-school AT 命令                | [推断] 未观察到独立枚举 |

**实现者不必自行构造 MBIM 控制转移** —— 走操作系统封装(MBN / ModemManager)即可。
下表是 Linux 侧对应的网络接口与工具:

| 概念 | Linux 路径 | Windows 路径 |
|------|-----------|--------------|
| MBIM 网络设备 | `/dev/cdc-wdm*` + `wwanX` 接口 | `cxwmbclass.sys` + `WWAN` 网络接口 |
| 状态查询 | `modem-manager` D-Bus API 或 `mmcli` | `mbnapi` (`Mobile Broadband` COM API) 或 WMI `MSFT_LTEConnectivity` |
| APN 配置 | `ModemManager` D-Bus 属性 `3gpp-cellular` / `mmcli -m <id> --apn` | Windows 移动热点设置 / MBN `SetDataProfile` |
| 信号强度 | `mmcli -m <id> --signal-quality` | `MSFT_LTEConnectivity` WMI / MBN `GetConnectionStatus` |

### 4.3 LTE 子命令的语义

```
vantage magicbay lte status
  输出:
  {
    "modem": "/sys/class/net/wwan0" | "WWAN0",
    "radio": "on"|"off",
    "state": "unknown"|"disabled"|"unregistered"|"searching"|"registered"|"connected",
    "signal_pct": 67,
    "signal_dbm": -78,
    "current_apn": "ctnet",
    "carrier": "CHNCT",
    "imei": "...",
    "sim": "inserted"|"absent"|"locked"
  }

vantage magicbay lte connect [--apn <name>] [--user <u>] [--passwd <p>]
  行为:
    1. 若已连接 → 报错 E_ALREADY_CONNECTED
    2. 调用 MM/MBN API 建立数据会话(IPv4/IPv6 均可)
    3. 返回 PDP 上下文:IP 地址、网关
  错误:
    - E_NO_DEVICE   : 未检测到 MagicBay LTE
    - E_SIM_LOCKED  : SIM 需 PIN
    - E_NO_SIGNAL   : 未注册到网络
    - E_APN_FAIL    : APN 拨号失败
    - E_RADIO_OFF   : 无线电被关闭(需先 radio on)

vantage magicbay lte disconnect
  行为:调用 MBN/ModemManager Disconnect(),清理 PDP 上下文。
```

### 4.4 数据开关与 APN 的"非标扩展"—— 没有

分析结论:**MagiCenter 在代码层面对 LTE 没有任何非标扩展**。
数据开关、APN 配置、SIM 状态完全走 Windows MBN 标准路径;
MagiCenter 只做 USB 层设备检测 + 插件分发 + 页面路由跳转。

因此实现者**不需要**实现任何联想私有 LTE 协议,只需包装操作系统的
标准 MBIM/ModemManager/MBN 接口即可。

### 4.5 相关 WMI 类(供 Windows 后端参考)

- `MSFT_LTEConnectivity` — Windows 系统 WMI 类,提供 LTE 连接状态;
  与 §3.3 提及的"上报渠道"吻合 [推断]。
- `MSFT_WwanProvider` — Windows 10/11 移动宽带提供程序状态。

---

## 5. 摄像头配件

### 5.1 架构结论

MagicBay 若含摄像头,**以标准 UVC 设备出现在系统视频设备列表中**,
与 MagiCenter 完全无关(§1.2 证据:主包零命中 `camera-plugin`/`uvc`/`video`
探测关键词;`PhotoGallery`/`Screenshot` Widget 使用的是桌面截图 + `ffmpeg.dll`,
与摄像头枚举无关)。

### 5.2 控制面

实现者走操作系统 UVC 标准控制:

- **Linux**:
  - `v4l2` ioctl: `VIDIOC_ENUMINPUT`/`VIDIOC_S_INPUT`(切换镜头),
    `VIDIOC_ENUM_FMT`/`VIDIOC_S_FMT`(分辨率/帧率),
    `VIDIOC_G_EXT_CTRLS`/`VIDIOC_S_EXT_CTRLS`(曝光、白平衡、增益)。
  - 隐私开关(系统级摄像头禁用):走 04 文档的
    WMI `LENOVO_UTILITY_DATA`/`Lenovo_BiosSetting.IntegratedCamera` 通路;
    MagicBay 摄像头**不受**该开关影响,它是独立硬件。
  - 节点路径:`/dev/videoN`,`/dev/mediaN`;通过 `v4l2-ctl --list-devices` 可
    看到物理端口。
- **Windows**:
  - Windows 10/11 统一视频类;应用侧用 Media Foundation + `MFCreateVideoCaptureDevice`.
  - 隐私开关同样走 WMI 通路;MagicBay 摄像头独立。

### 5.3 私有扩展单元

- **分析结论**:[推断] MagicBay 摄像头**没有**已知的私有 UVC Extension Unit。
  证据:MagiCenter 主体不做 UVC 探测,MagiCenter 组件资料中未发现任何 UVC 相关
  控制代码。若未来发现私有扩展单元,应在 `KNOWN_CAMERAS` 表中追加。

### 5.4 命令语义

```
vantage magicbay cam status
  → 列出所有已挂载的 UVC 视频设备,标注哪些是 MagicBay 配件路径下的。
  输出:
  [
    {
      "path": "/dev/video2",
      "device": "Media: Lenovo MagicBay Cam",
      "parent_path": "usb-...-1.2",
      "formats": ["YUYV 1920x1080@30", "MJPG 1280x720@30"],
      "current": "YUYV 1280x720@30"
    }
  ]

vantage magicbay cam set <prop>=<value>
  prop ∈ {
    input=<idx>,
    fmt=<codec>,
    res=<WxH>,
    fps=<n>,
    exposure=<mode>,
    brightness=<int>,
    contrast=<int>,
    saturation=<int>,
    gain=<int>,
    whitebalance=<mode>,
    privacy=on|off   # MagicBay 摄像头物理遮蔽(若硬件支持,如 e shutter)
  }
```

---

## 6. 扩展屏配件

### 6.1 硬件拓扑(关键结论)

扩展屏**不是** USB 复合设备的 interface。硬件上:

1. MagicBay 背壳内嵌 eDP 连接器。
2. 背壳内置 **Qualcomm QDU1000 / QFE7260 系列 eDP→DP 桥接芯片**(或同类)。
3. 桥接芯片通过 **ACPI 总线** 暴露为:
   - `ACPI\QCOM2488`(显示控制器)
   - `ACPI\QCOM24B7`(Chipidea USB Role-Switch)
4. Windows 侧由 `ufxchipidea.inf` (`UfxChipidea.sys`)、`urschipidea.inf`
   (`urschipidea.sys`) 加载驱动。
5. 显示协商在 **ACPI/GPU** 层面发生,不经过 USB,不做 DP Alt Mode 描述符协商。

### 6.2 枚举路径

实现者做扩展屏检测时,要**同时**观察两个通道:

- **ACPI 通道**(主):
  - Linux:`/sys/bus/acpi/devices/QCOM2488*`、`/sys/firmware/acpi/tables/DSDT`。
    `lshal` 或 `udevadm` 会看到 `ACPI000C`(display)、`QCOM2488` 等设备。
  - Windows:WMI `Win32_PnPEntity` + DeviceID 过滤 `QCOM2488`/`QCOM24B7`。
- **显示热插拔事件**:
  - Linux:DRM `KHD`/`HOTPLUG` 事件(evdev `drm_event`);
    或轮询 `/sys/class/drm/cardX-*/status`(`connected`/`disconnected`)。
  - Windows:WM_DISPLAYCHANGE / `CHANGEBTNSTR` 广播。

### 6.3 EDID 读取

扩展屏接入后,系统 DRM/GPU 驱动会通过 AUX channel 读取 eDP/DP 面板的
EDID 与 DPCD(显示能力数据)。实现者无需自行实现 EDID 协议,
走标准接口:

- **Linux**:`/sys/class/drm/cardX-HDP-A1/edid`(对应扩展屏的输出端口);
  或 `modeinfo -p` / `get-edid` 工具。
  常见 EDID 解析字段:`vendor`、`product_code`、`serial`、
  `supported_detailed_timings[]`(每种分辨率+刷新率)。
- **Windows**:`EnumDisplayDevices` + `DisplayConfigGetDeviceInfo`
  (`DISPLAYCONFIG_DEVICE_INFO_GET_ADVANCED_COLOR_INFO` 等)。

### 6.4 模式设置

扩展屏的模式列表由 EDID 中 `supported_detailed_timings` 决定,
加上 DP/eDP 链路层带宽约束。实现者提供:

```
vantage magicbay display detect
  → 列出:
  [
    {
      "output": "card0-eDP-1",
      "edid_vendor": "LEN",
      "native": "1920x1080@60",
      "modes": ["1920x1080@60", "1920x1080@30", "1280x720@60"],
      "status": "connected"|"disconnected"
    }
  ]

vantage magicbay display status
  → 当前活跃模式。

vantage magicbay display set-mode <WxH@R>
  行为:
    1. 通过 DRM `DRM_MODE_SET_CRTC`/`drmModeSetMode()` 切换模式;
       或 Windows `ChangeDisplaySettingsEx()`.
    2. 若目标模式不在 EDID 列表,回退到最近似模式。
  错误:
    - E_NOT_CONNECTED   : 扩展屏未连接
    - E_UNSUPPORTED_MODE: 面板不支持该模式
    - E_MODESET_FAIL    : 驱动切换失败(回滚)
```

### 6.5 与 DPTF 的耦合

扩展屏插入时,QDU 桥接芯片的功耗会被 DPTF 感知(`ACPI\INTC10D4/10D5/10D8`
设备,见 `目标机 MagicBay/DPTF 实机枚举数据`)。实现者**不必**主动干预;
但如果扩展屏导致热策略切换,可在 07 文档的调优中考虑。

---

## 7. 配件供电与磁吸状态

### 7.1 硬件机制

MagicBay 的供电通过 USB 3.0 总线的 +5V / +5V_SBU 提供;
磁吸状态检测由硬件引脚完成,典型电路:

- 两组磁吸触点分别接触背壳的主电源与数据总线。
- 硬件上存在一个 GPIO 或 EC 引脚,由背壳的**机械接触状态**驱动,
  反映"是否稳定磁吸到位"(防止松脱时被误判为已连接)。

### 7.2 软件观察点

- MagiCenter 软件层面**没有直接读写磁吸 GPIO**;
  它只通过 USB `inserted`/`removed` 事件判定附件是否在线。
  这与笔记本主板的 EC 处理类似 —— EC 负责物理接触判定,软件只见"有/无"信号。
- DPTF 会间接反映附件状态:扩展屏插入后 `IPF` 参与者(见
  `magicbay-dptf.txt` 的 `INTC10D5` 系列)会更新功耗预算。

### 7.3 实现者需要做什么

- 对 **`detect` 命令**:
  输出 `attached` 字段即可(依据 USB/ACPI 设备是否出现)。
- 对**持续监听**:
  不必单独监听"磁吸稳定性";USB 断开即代表物理脱离。
- [推断]**如果**未来版本 MagicBay 提供独立的"松动警告"信号
  (如通过 ACPI _EVTx 或自定义 EC 命令),它应该以
  `WMI\LEN..._MAGICBAY_EVT` 事件的形式暴露,但目前**没有**观察到此类通道。

---

## 8. Linux 对应路径速查

(详细映射见 09 文档,这里列出关键指针供实现者定位。)

| MagicBay 概念 | Linux 通道 | 相关内核驱动 | 相关工具/Daemon |
|---------------|-----------|-------------|-----------------|
| 复合设备枚举 | `/sys/bus/usb/devices/` + `libusb` | `usbcore` | `udev`, `lsusb` |
| LTE (MBIM) 网络接口 | `/sys/class/net/wwan*` | `cdc_mbim` | `ModemManager`, `NetworkManager`, `mmcli` |
| LTE 控制 | ModemManager D-Bus (`org.freedesktop.ModemManager1.Modem`) | `cdc_mbim` | `mmcli`, `qmi`, `mbim` |
| 摄像头(UVC) | `/dev/video*` + `v4l2` ioctl | `uvcvideo` | `v4l2-ctl`, `guvcview` |
| 扩展屏 (eDP/DP) | `/sys/class/drm/card*-*/` | `i915`/`amdgpu` + DPTF | `xrandr`, `libdrm`, `udevadm monitor --drm` |
| ACPI QCOM 设备 | `/sys/bus/acpi/devices/QCOM*` | `acpi` | `acpidump`, `lshal` |
| 热插拔事件 | `udev` netlink | `usbcore`, `acpi` | `udevadm monitor --subsystem-match=usb,acpi` |
| DPTF/IPF 耦合 | `/sys/class/dptf/` 或 `/sys/devices/platform/...` | `dptf`, `ipf_acpi` (Panther Lake) | `ipctl`, `/proc` |

---

## 9. Windows 对应路径速查(参考)

| MagicBay 概念 | Windows 通道 | 相关驱动/API |
|---------------|-------------|--------------|
| 复合设备枚举 | SetupDi / `USB\VID_17EF&PID_*` | `usbccgp`, `usbhub3` |
| LTE (MBIM) | `USB\VID_17EF&PID_7005&MI_00` | `cxwmbclass.sys` (`netwmbclass.inf`) |
| LTE 控制 | WMI `MSFT_LTEConnectivity` / COM `IMbnModem` | MBN API, Windows Mobile Broadband 堆栈 |
| 摄像头 | PnP `VID...&MI_xx` | `uvcvideo.sys` (内置 UVC) |
| 扩展屏 | ACPI `QCOM2488` / `QCOM24B7` | `UfxChipidea.sys`, `urschipidea.sys` |
| DPTF | ACPI `INTC10D4/10D5/10D8` | `ipf_acpi.sys`, `dptftcs.exe` |

---

## 10. 实现要点清单

实现者按下列清单逐项核对:

1. **内置常量表**:`KNOWN_MAGICBAY_DEVICES` 含 `{ vid, pid, kind, description }`
   三项(PID `0x62B5`/`0x7005`/`0x1117`)。
2. **detect**:跨平台枚举 USB(`0x17EF`) + ACPI(`QCOM2488`/`QCOM24B7`);
   输出 JSON,包含 `path`, `vid`, `pid`, `kind`, `interfaces`。
3. **lte status/connect/disconnect**:
   - 走 ModemManager (Linux) 或 MBN API (Windows)。
   - **不**自行构造 MBIM 控制转移。
   - 错误码定义:`E_NO_DEVICE`, `E_SIM_LOCKED`, `E_NO_SIGNAL`,
     `E_APN_FAIL`, `E_RADIO_OFF`, `E_ALREADY_CONNECTED`, `E_NOT_CONNECTED`。
4. **cam**:标准 v4l2 / Media Foundation;不做 VID/PID 匹配,只按
   "设备树父路径是否为 0x17EF 复合设备"区分 MagicBay 与非 MagicBay 摄像头。
5. **display**:
   - 通过 DRM `card*-eDP-*` 或 Windows `DisplayConfig` 枚举输出端口。
   - EDID 走标准读取,不做私有协议。
   - 模式切换前做回滚准备。
6. **热插拔事件**:
   - Linux: `udev` monitor。
   - Windows: `RegisterDeviceNotification`。
   - **实现 300 ms 睡眠唤醒去抖**:见 §3.2,与 MagiCenter 行为对齐,
     避免睡眠唤醒时误判为"拔出"。
7. **错误处理**:所有子命令返回结构化 JSON 错误(`{"error": "...", "detail": ...}`),
   不静默失败。
8. **不做**:MagiCenter 插件分发、OTA、LUDP 埋点、OAuth 登录、桌面挂件。

---

## 11. 交叉引用

- 01 文档:WMI/EC/IOCTL 通道总表(扩展屏与 LTE 都依赖的 DPTF/EC 通道)。
- 04 文档:摄像头隐私开关(系统摄像头开关与 MagicBay 摄像头的区别)。
- 07 文档:扩展屏与 DPTF/IPF 的热耦合。
- 08 文档:CLI 子命令骨架与错误模型。
- 09 文档:Linux 后端完整映射。

---

## 12. 证据索引

| 事实 | 证据 |
|------|------|
| KNOWN_DEVICES 三型号常量 | `MagiCenter 组件内部接口说明, `out/main/index.js` bundle 行 103–125 |
| PID 0x7005 是复合设备 + MI_00 走 MBIM | `目标机 MagicBay/DPTF 实机枚举数据` 表首行;`目标机驱动组件实机清单 device interfaces |
| cxwmbclass / netwmbclass.inf | `目标机驱动组件实机清单` `DEVPKEY_Device_DriverInfSection = wmbclass.ndi` |
| MagiCenter 主包不直接操作 MBIM/MBN/APN/SIM | `MagiCenter 组件内部接口说明,bundle 全文搜索零命中 |
| 唯一 MBN 提及为错误分类 | `MagiCenter 组件内部接口说明,bundle 行 23433 `_isMbnError` |
| 摄像头非 MagicBay USB 接口 | `MagiCenter 组件内部接口说明,无 `camera-plugin` |
| 扩展屏走 ACPI QCOM2488/QCOM24B7 | `MagiCenter 组件内部接口说明–5.2 |
| 驱动:ufxchipidea.inf / urschipidea.inf | `目标机驱动组件实机清单` DriverStore 清单 |
| 热插拔流程 + 300 ms 去抖 | `MagiCenter 组件内部接口说明 |
| 首次连接状态键 `deviceFirstConnection` | `MagiCenter 组件内部接口说明 |
| `winapi_addon.node` 导出 | `MagiCenter 组件内部接口说明 |
| IPF/DPTF 参与者 | `目标机 MagicBay/DPTF 实机枚举数据` DPTF/ESIF 表,`INTC10D4/10D5/10D8` |
| WMI `MSFT_LTEConnectivity` | [推断] 依据 `MagiCenter 组件内部接口说明 文字 |
| 硬件跳线决定暴露哪些 USB 接口 | [推断] 依据 `MagiCenter 组件内部接口说明 |
| MagicBay 摄像头无私有 UVC Extension Unit | [推断] 未观察到任何私有控制代码 |
| 磁吸状态无独立 ACPI/GPIO 通道 | [推断] MagiCenter 软件侧零命中磁吸状态读取 |

**推断条目合计**:5 处。

---

*本规范结束。实现者遇到未覆盖的行为应回问规格组,而非自行分析厂商二进制。*
