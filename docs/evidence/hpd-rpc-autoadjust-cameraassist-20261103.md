# HPD RPC 剩余 setter 实测验收(2026-11-03,TB14P-EZRA / 21VG)

对 `docs/evidence/hpd-rpc-re-20261102.md` §4 恢复出的、此前未接线的 HPD RPC op 做真机
写探测。探针为 `lctrl-hal-win/examples/hpd_probe.rs`(`hpd_probe <op> <arg>`:一次
`NdrClientCall3`,随后读回 op5 capability + op6 settings),传输为自有 NDR32 表
(`ndr/hpd.rs`,emit ops 7/10/13/15/17,PROC_COUNT=18),不调用任何 Lenovo 运行时库。

## 结果

| op | 语义(§4) | arg | 服务端返回 | 注册表持久化 | RPC status |
|----|----------|-----|-----------|--------------|------------|
| 10 | SetApproachDistanceAutoAdjust | 0→1 | 0 (accepted) | `AutoAdjust` 1→2 | `values[4]` 0→1 |
| 13 | SetPresenceLeaveCameraAssist | 0→1 | 0 (accepted) | `CameraDetect` 1→2 | `values[7]` 0→1 |
| 15 | SetPresenceLeaveDistanceAutoAdjust | 0→1 | 0 (accepted) | `LeaveAutoAdjust` 1→2 | `values[9]` 0→1 |
| 17 | SetHPDEnabledYogaModeV2 | 1 | **4317 ERROR_INVALID_DATA** | 无 Yoga 键 | `values[12]` 不变 |

## 结论

- 三个 auto-adjust/camera-assist setter 接受 0/1 并持久化为 `arg + 1`(1=off / 2=on),
  与已实测的 `SetVideoAutoPauseResume` 编码一致;`CameraDetect` 的旧表语义"1开2关"
  被实测更正为 RPC arg+1。op-5 capability 位:AutoAdjust=0x08、CameraAssist=0x40、
  LeaveAutoAdjust=0x100,advertised=0x7ff,三位均置位。
- 新增 CLI:`sense approach-auto-adjust`、`sense camera-assist`、
  `sense leave-auto-adjust`(on/off),均走 RPC 写 + `Parameters\HPD` 独立读回轮询。
- op17 拒绝 4317,YogaModeV2 在本机服务上维持边界,不实现。
- op7 ResetAllSetting 的 NDR 表已发出但属破坏性全量重置,不在真机验证、不暴露子命令。
- `AdjustDistance` 在服务自身自动调整下漂移(455→843),与探测写无关,不视为副作用。
- `SenseState.privacy_alert` 与 `camera_assist` 同读 `CameraDetect`;legacy 肩窥管道
  (§13.2)在本机全拒,RPC 是本机唯一活写者,故两者统一按 RPC arg+1 解码。

## 2026-11-03 后续:op7 ResetAllSetting + 枚举键 arg+1 更正

**op17 参数扫描**:arg ∈ {0,1,2,4,8} 全部 4317 ERROR_INVALID_DATA——拒绝与值无关,
21VG(非 Yoga 翻盖)无 Yoga 模式设置面,维持边界。

**op7 ResetAllSetting 实测**:context-only 调用被接受;重置 **可写用户字段** 为服务默认
(CameraDetect→off、LeaveWait→Middle、Video→on),只读键(AdjustDistance/FaceRegistered/
Version/SensorType/UIInstalled)不动。新增 `sense reset` 子命令(context-only op7);
`--auto-restore` 预检并捕获全部 RPC 字段,重置后逐项重写回原值——真机验证
camera-assist=on、pause-video=off、leave_wait=Short 全部还原。

**枚举键 arg+1 更正**:op14 arg0/1/2 → `LeaveDistance` =1/2/3,op12 arg0 → `LeaveWait`=1,
即注册表枚举值也是 `arg + 1`(1=第一项)。`NativeHpdStore` 的 `leave_wait` /
`leave_distance` / `approach_distance` 解码已改为 `raw-1` 再 `from_raw`,与 RPC status
`values[6]/[8]/[3]` 和写入参数一致——此前按原值解码导致枚举写读回 mismatch。
`PresentLeave`/`Approach` 仍为裸 `arg`(0/1),`Legacy01` 不变。
