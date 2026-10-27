use std::{
    collections::VecDeque,
    io::{self, Read},
    process::{Child, ChildStdout, Command, Stdio},
    sync::Mutex,
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use lctrl_core::{LctrlError, Result};
use lctrl_hal::{
    WifiConnectionState, WifiHistoryPolicy, WifiHistorySnapshot, WifiInterfaceSnapshot,
    WifiLiveNetwork, WifiSavedProfile, WifiSecurityControl, WifiSecurityReport,
    assess_wifi_security,
};

const DEFAULT_COMMAND_TIMEOUT: Duration = Duration::from_secs(10);
const DEFAULT_OUTPUT_LIMIT: usize = 4 * 1024 * 1024;
const POLL_INTERVAL: Duration = Duration::from_millis(10);

/// Read-only netsh operations used by the Windows implementation. The trait
/// intentionally has no key/content option: saved-profile keys are never read.
pub trait WlanNetshRunner: Send + Sync {
    fn show_interfaces(&self) -> Result<String>;
    fn show_profiles(&self) -> Result<String>;
    fn show_profile(&self, name: &str, interface: Option<&str>) -> Result<String>;
}

/// Bounded, direct `netsh` process execution. No command shell is involved,
/// stderr is discarded, and output is never included in errors.
#[derive(Clone, Debug)]
pub struct NetshCommandRunner {
    timeout: Duration,
    output_limit: usize,
}

impl NetshCommandRunner {
    #[must_use]
    pub const fn new(timeout: Duration) -> Self {
        Self {
            timeout,
            output_limit: DEFAULT_OUTPUT_LIMIT,
        }
    }

    #[must_use]
    pub const fn with_output_limit(mut self, output_limit: usize) -> Self {
        self.output_limit = output_limit;
        self
    }

    fn execute(&self, mut command: Command, operation: &'static str) -> Result<String> {
        command
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        let mut child = command
            .spawn()
            .map_err(|error| contextual_io(error, format_args!("failed to start {operation}")))?;
        let stdout = match child.stdout.take() {
            Some(stdout) => stdout,
            None => {
                terminate_and_reap(&mut child);
                return Err(LctrlError::ChannelUnavailable {
                    channel: format!("{operation} did not provide captured stdout"),
                });
            }
        };
        let output_limit = self.output_limit;
        let reader = match thread::Builder::new()
            .name("vantage-netsh-stdout".into())
            .spawn(move || capture_stdout(stdout, output_limit))
        {
            Ok(reader) => reader,
            Err(error) => {
                terminate_and_reap(&mut child);
                return Err(contextual_io(
                    error,
                    format_args!("failed to capture {operation} output"),
                ));
            }
        };

        let status = wait_for_child(&mut child, self.timeout, operation)?;
        let captured = match reader.join() {
            Ok(Ok(captured)) => captured,
            Ok(Err(error)) => {
                return Err(contextual_io(
                    error,
                    format_args!("failed to read {operation} output"),
                ));
            }
            Err(_) => {
                return Err(LctrlError::Io(io::Error::other(format!(
                    "{operation} output reader panicked"
                ))));
            }
        };
        if !status.success() {
            return Err(LctrlError::ChannelUnavailable {
                channel: format!("{operation} exited unsuccessfully ({status})"),
            });
        }
        if captured.truncated {
            return Err(LctrlError::ChannelUnavailable {
                channel: format!(
                    "{operation} output exceeded the {output_limit}-byte safety limit"
                ),
            });
        }
        decode_output(captured.bytes, operation)
    }
}

impl Default for NetshCommandRunner {
    fn default() -> Self {
        Self::new(DEFAULT_COMMAND_TIMEOUT)
    }
}

impl WlanNetshRunner for NetshCommandRunner {
    fn show_interfaces(&self) -> Result<String> {
        let mut command = Command::new("netsh");
        command.args(["wlan", "show", "interfaces"]);
        self.execute(command, "netsh wlan show interfaces")
    }

    fn show_profiles(&self) -> Result<String> {
        let mut command = Command::new("netsh");
        command.args(["wlan", "show", "profiles"]);
        self.execute(command, "netsh wlan show profiles")
    }

    fn show_profile(&self, name: &str, interface: Option<&str>) -> Result<String> {
        let mut command = Command::new("netsh");
        command
            .args(["wlan", "show", "profile"])
            .arg(format!("name={name}"));
        if let Some(interface) = interface {
            command.arg(format!("interface={interface}"));
        }
        // Deliberately no `key=clear`: netsh's default profile output includes
        // security configuration but never the saved key content.
        self.execute(command, "netsh wlan show profile")
    }
}

/// Serializable Windows Wi-Fi assessment service with bounded memory-only
/// history. Constructing or using this service never writes history to disk,
/// the registry, a daemon, or an operating-system credential store.
pub struct WindowsWifiSecurity<R = NetshCommandRunner> {
    runner: R,
    history_policy: WifiHistoryPolicy,
    history: Mutex<VecDeque<WifiInterfaceSnapshot>>,
}

impl<R: WlanNetshRunner> WindowsWifiSecurity<R> {
    #[must_use]
    pub fn new(runner: R) -> Self {
        Self {
            runner,
            history_policy: WifiHistoryPolicy::default(),
            history: Mutex::new(VecDeque::new()),
        }
    }

    pub fn with_history_policy(runner: R, history_policy: WifiHistoryPolicy) -> Result<Self> {
        history_policy.validate()?;
        Ok(Self {
            runner,
            history_policy,
            history: Mutex::new(VecDeque::new()),
        })
    }

    #[must_use]
    pub const fn history_policy(&self) -> WifiHistoryPolicy {
        self.history_policy
    }

    pub fn assess(&self) -> Result<WifiSecurityReport> {
        let observed_at_unix_ms = now_unix_ms()?;
        let interfaces_output = self.runner.show_interfaces()?;
        let interfaces = parse_interfaces(&interfaces_output, observed_at_unix_ms)?;

        let profiles_output = self.runner.show_profiles()?;
        let profile_index = parse_profile_index(&profiles_output)?;
        let mut saved_profiles = Vec::with_capacity(profile_index.len());
        for entry in profile_index {
            // Do not retain or parse SSID names, key-presence fields, or any
            // unexpected key-content lines from the profile detail document.
            let detail = self
                .runner
                .show_profile(&entry.name, entry.interface_name.as_deref())?;
            let (authentication, cipher) = parse_profile_security(&detail)?;
            saved_profiles.push(WifiSavedProfile {
                assessment: assess_wifi_security(authentication.as_deref(), cipher.as_deref()),
                name: entry.name,
                interface_name: entry.interface_name,
                profile_type: entry.profile_type,
                authentication,
                cipher,
            });
        }

        let mut history = self.lock_history();
        record_observations(&mut history, self.history_policy, &interfaces);
        drop(history);
        Ok(WifiSecurityReport {
            interfaces,
            saved_profiles,
        })
    }

    #[must_use]
    pub fn history(&self) -> WifiHistorySnapshot {
        WifiHistorySnapshot {
            policy: self.history_policy,
            entries: self.lock_history().iter().cloned().collect(),
        }
    }

    /// Clears only this service instance's bounded in-memory observations.
    /// There is no persisted history to delete.
    pub fn clear_history(&self) {
        self.lock_history().clear();
    }

    fn lock_history(&self) -> std::sync::MutexGuard<'_, VecDeque<WifiInterfaceSnapshot>> {
        // VecDeque mutation has no user code and no cross-field invariant that
        // a panic could invalidate, so recovering the owned history is safe.
        self.history
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

impl Default for WindowsWifiSecurity<NetshCommandRunner> {
    fn default() -> Self {
        Self::new(NetshCommandRunner::default())
    }
}

impl<R: WlanNetshRunner> WifiSecurityControl for WindowsWifiSecurity<R> {
    fn wifi_security(&self) -> Result<WifiSecurityReport> {
        self.assess()
    }

    fn wifi_security_history(&self) -> Result<WifiHistorySnapshot> {
        Ok(self.history())
    }

    fn clear_wifi_security_history(&self) -> Result<()> {
        self.clear_history();
        Ok(())
    }
}

fn record_observations(
    history: &mut VecDeque<WifiInterfaceSnapshot>,
    policy: WifiHistoryPolicy,
    interfaces: &[WifiInterfaceSnapshot],
) {
    for interface in interfaces {
        let mut observation = interface.clone();
        if let Some(live) = observation.live_network.as_mut() {
            // A profile name commonly equals the SSID, and BSSID can identify a
            // location just as precisely. Neither is needed for diagnostics.
            live.bssid = None;
            live.profile_name = None;
            if !policy.retain_ssid {
                live.ssid = None;
            }
        }
        if history.len() == policy.capacity {
            history.pop_front();
        }
        history.push_back(observation);
    }
}

fn now_unix_ms() -> Result<u64> {
    let elapsed = SystemTime::now().duration_since(UNIX_EPOCH).map_err(|_| {
        LctrlError::Io(io::Error::other(
            "system clock is before the Unix epoch; cannot timestamp Wi-Fi history",
        ))
    })?;
    u64::try_from(elapsed.as_millis()).map_err(|_| {
        LctrlError::Io(io::Error::other(
            "system clock timestamp exceeds the Wi-Fi history representation",
        ))
    })
}

struct CapturedOutput {
    bytes: Vec<u8>,
    truncated: bool,
}

fn capture_stdout(mut stdout: ChildStdout, limit: usize) -> io::Result<CapturedOutput> {
    let mut bytes = Vec::with_capacity(limit.min(8192));
    let mut truncated = false;
    let mut buffer = [0u8; 8192];
    loop {
        let count = stdout.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        let retained = limit.saturating_sub(bytes.len()).min(count);
        bytes.extend_from_slice(&buffer[..retained]);
        truncated |= retained != count;
        // Continue draining after the retention limit so netsh cannot block on
        // a full pipe while the bounded waiter polls it.
    }
    Ok(CapturedOutput { bytes, truncated })
}

fn wait_for_child(
    child: &mut Child,
    timeout: Duration,
    operation: &'static str,
) -> Result<std::process::ExitStatus> {
    let started = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return Ok(status),
            Ok(None) => {}
            Err(error) => {
                terminate_and_reap(child);
                return Err(contextual_io(
                    error,
                    format_args!("failed while waiting for {operation}"),
                ));
            }
        }
        let elapsed = started.elapsed();
        if elapsed >= timeout {
            terminate_and_reap(child);
            return Err(LctrlError::ChannelUnavailable {
                channel: format!(
                    "{operation} exceeded its {}-ms bounded wait",
                    timeout.as_millis()
                ),
            });
        }
        thread::sleep(POLL_INTERVAL.min(timeout.saturating_sub(elapsed)));
    }
}

fn terminate_and_reap(child: &mut Child) {
    let _ = child.kill();
    let _ = child.wait();
}

fn contextual_io(error: io::Error, context: std::fmt::Arguments<'_>) -> LctrlError {
    LctrlError::Io(io::Error::new(error.kind(), format!("{context}: {error}")))
}

fn decode_output(bytes: Vec<u8>, operation: &'static str) -> Result<String> {
    if bytes.starts_with(&[0xff, 0xfe]) {
        return decode_utf16(&bytes[2..], true, operation);
    }
    if bytes.starts_with(&[0xfe, 0xff]) {
        return decode_utf16(&bytes[2..], false, operation);
    }
    if looks_like_utf16(&bytes, true) {
        return decode_utf16(&bytes, true, operation);
    }
    if looks_like_utf16(&bytes, false) {
        return decode_utf16(&bytes, false, operation);
    }
    match String::from_utf8(bytes) {
        Ok(text) => Ok(text),
        Err(error) => {
            #[cfg(windows)]
            {
                decode_oem(error.into_bytes(), operation)
            }
            #[cfg(not(windows))]
            {
                let _ = error;
                Err(unrecognized_output(operation))
            }
        }
    }
}

fn looks_like_utf16(bytes: &[u8], little_endian: bool) -> bool {
    if bytes.len() < 4 || bytes.len() % 2 != 0 {
        return false;
    }
    let pairs = bytes.len() / 2;
    let (text_offset, zero_offset) = if little_endian { (0, 1) } else { (1, 0) };
    let text_zeroes = bytes
        .iter()
        .skip(text_offset)
        .step_by(2)
        .filter(|byte| **byte == 0)
        .count();
    let high_zeroes = bytes
        .iter()
        .skip(zero_offset)
        .step_by(2)
        .filter(|byte| **byte == 0)
        .count();
    high_zeroes * 2 >= pairs && text_zeroes * 2 < pairs
}

fn decode_utf16(bytes: &[u8], little_endian: bool, operation: &'static str) -> Result<String> {
    if bytes.len() % 2 != 0 {
        return Err(unrecognized_output(operation));
    }
    let units = bytes.chunks_exact(2).map(|pair| {
        if little_endian {
            u16::from_le_bytes([pair[0], pair[1]])
        } else {
            u16::from_be_bytes([pair[0], pair[1]])
        }
    });
    let mut output = String::with_capacity(bytes.len() / 2);
    for character in std::char::decode_utf16(units) {
        let character = character.map_err(|_| unrecognized_output(operation))?;
        output.push(character);
    }
    Ok(output)
}

#[cfg(windows)]
fn decode_oem(bytes: Vec<u8>, operation: &'static str) -> Result<String> {
    use std::ptr;

    use windows_sys::Win32::Globalization::{CP_OEMCP, MultiByteToWideChar};

    if bytes.is_empty() {
        return Ok(String::new());
    }
    let input_length = i32::try_from(bytes.len()).map_err(|_| unrecognized_output(operation))?;
    let required = unsafe {
        MultiByteToWideChar(
            CP_OEMCP,
            0,
            bytes.as_ptr(),
            input_length,
            ptr::null_mut(),
            0,
        )
    };
    if required <= 0 {
        return Err(contextual_io(
            io::Error::last_os_error(),
            format_args!("failed to decode {operation} using the Windows OEM code page"),
        ));
    }
    let mut wide = vec![0u16; required as usize];
    let written = unsafe {
        MultiByteToWideChar(
            CP_OEMCP,
            0,
            bytes.as_ptr(),
            input_length,
            wide.as_mut_ptr(),
            required,
        )
    };
    if written != required {
        return Err(unrecognized_output(operation));
    }
    String::from_utf16(&wide).map_err(|_| unrecognized_output(operation))
}

#[derive(Default)]
struct InterfaceBuilder {
    touched: bool,
    name: Option<String>,
    description: Option<String>,
    state: Option<String>,
    ssid: Option<String>,
    bssid: Option<String>,
    profile_name: Option<String>,
    authentication: Option<String>,
    cipher: Option<String>,
    signal_percent: Option<u8>,
    channel: Option<u32>,
}

impl InterfaceBuilder {
    fn apply(&mut self, label: &str, value: &str) -> Result<()> {
        if label_is(label, &["name", "名称"]) {
            self.name = owned_non_empty(value);
        } else if label_is(label, &["description", "描述"]) {
            self.description = owned_non_empty(value);
        } else if label_is(label, &["state", "状态"]) {
            self.state = owned_non_empty(value);
        } else if normalized_label_eq(label, "ssid") {
            self.ssid = owned_non_empty(value);
        } else if normalized_label_eq(label, "bssid") {
            self.bssid = owned_non_empty(value);
        } else if label_is(label, &["profile", "profilename", "配置文件"]) {
            self.profile_name = owned_non_empty(value);
        } else if label_is(label, &["authentication", "身份验证", "身分验证"]) {
            self.authentication = owned_non_empty(value);
        } else if label_is(label, &["cipher", "加密", "密码"]) {
            self.cipher = owned_non_empty(value);
        } else if label_is(label, &["signal", "信号"]) {
            self.signal_percent = Some(parse_signal(value)?);
        } else if label_is(label, &["channel", "通道", "频道"]) {
            self.channel = Some(parse_channel(value)?);
        } else {
            return Ok(());
        }
        self.touched = true;
        Ok(())
    }

    fn finish(self, observed_at_unix_ms: u64) -> Result<WifiInterfaceSnapshot> {
        let interface_name = self.name.ok_or_else(|| {
            unrecognized_output("netsh wlan show interfaces (missing interface name)")
        })?;
        let connection_state = self
            .state
            .as_deref()
            .map(classify_connection_state)
            .unwrap_or(WifiConnectionState::Unknown);
        let live_network = (connection_state == WifiConnectionState::Connected).then(|| {
            let assessment =
                assess_wifi_security(self.authentication.as_deref(), self.cipher.as_deref());
            WifiLiveNetwork {
                ssid: self.ssid,
                bssid: self.bssid,
                profile_name: self.profile_name,
                authentication: self.authentication,
                cipher: self.cipher,
                signal_percent: self.signal_percent,
                channel: self.channel,
                assessment,
            }
        });
        Ok(WifiInterfaceSnapshot {
            observed_at_unix_ms,
            interface_name,
            description: self.description,
            state: self.state,
            connection_state,
            live_network,
        })
    }
}

fn parse_interfaces(output: &str, observed_at_unix_ms: u64) -> Result<Vec<WifiInterfaceSnapshot>> {
    let operation = "netsh wlan show interfaces";
    let mut interfaces = Vec::new();
    let mut builder = InterfaceBuilder::default();
    for line in output.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            if builder.touched {
                interfaces.push(builder.finish(observed_at_unix_ms)?);
                builder = InterfaceBuilder::default();
            }
            continue;
        }
        let Some((label, value)) = split_field(trimmed) else {
            continue;
        };
        if label_is(label, &["name", "名称"]) && builder.name.is_some() {
            interfaces.push(builder.finish(observed_at_unix_ms)?);
            builder = InterfaceBuilder::default();
        }
        builder.apply(label, value)?;
    }
    if builder.touched {
        interfaces.push(builder.finish(observed_at_unix_ms)?);
    }
    if interfaces.is_empty() && !is_no_interface_output(output) {
        return Err(unrecognized_output(operation));
    }
    Ok(interfaces)
}

fn is_no_interface_output(output: &str) -> bool {
    let lowercase = output.to_ascii_lowercase();
    lowercase.contains("there is no interface")
        || lowercase.contains("no wireless interface")
        || output.contains("没有接口")
        || output.contains("没有无线接口")
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ProfileIndexEntry {
    name: String,
    interface_name: Option<String>,
    profile_type: String,
}

fn parse_profile_index(output: &str) -> Result<Vec<ProfileIndexEntry>> {
    let operation = "netsh wlan show profiles";
    let mut profiles = Vec::new();
    let mut current_interface = None;
    let mut saw_profile_document = false;
    for line in output.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let header = trim_trailing_colon(trimmed);
        if let Some(interface) = strip_prefix_ascii_case(header, "Profiles on interface ") {
            let interface = interface.trim();
            current_interface = owned_non_empty(interface);
            saw_profile_document = true;
            continue;
        }
        if let Some(interface) = header
            .strip_prefix("接口")
            .and_then(|rest| rest.trim_start().strip_suffix("上的配置文件"))
        {
            current_interface = owned_non_empty(interface.trim());
            saw_profile_document = true;
            continue;
        }
        if is_profile_section_heading(trimmed) {
            saw_profile_document = true;
            continue;
        }
        let Some((label, value)) = split_field(trimmed) else {
            continue;
        };
        if !is_profile_type_label(label) {
            continue;
        }
        saw_profile_document = true;
        if value.trim().is_empty()
            || value.trim().eq_ignore_ascii_case("<none>")
            || matches!(value.trim(), "<无>" | "无")
        {
            continue;
        }
        profiles.push(ProfileIndexEntry {
            name: value.trim().to_owned(),
            interface_name: current_interface.clone(),
            profile_type: label.trim().to_owned(),
        });
    }
    if profiles.is_empty() && !saw_profile_document && !is_no_interface_output(output) {
        return Err(unrecognized_output(operation));
    }
    Ok(profiles)
}

fn parse_profile_security(output: &str) -> Result<(Option<String>, Option<String>)> {
    let mut authentication = None;
    let mut cipher = None;
    for line in output.lines() {
        let Some((label, value)) = split_field(line.trim()) else {
            continue;
        };
        if label_is(label, &["authentication", "身份验证", "身分验证"]) {
            authentication = owned_non_empty(value);
        } else if label_is(label, &["cipher", "加密", "密码"]) {
            cipher = owned_non_empty(value);
        }
    }
    if authentication.is_none() && cipher.is_none() {
        return Err(unrecognized_output(
            "netsh wlan show profile (missing security settings)",
        ));
    }
    // Every other line, including `Security key` and an unexpected
    // `Key Content`, is intentionally ignored and cannot enter the report.
    Ok((authentication, cipher))
}

fn classify_connection_state(value: &str) -> WifiConnectionState {
    if normalized_eq_any(value, &["connected", "已连接"]) {
        WifiConnectionState::Connected
    } else if normalized_eq_any(value, &["disconnected", "已断开连接", "已中断连线"]) {
        WifiConnectionState::Disconnected
    } else {
        WifiConnectionState::Unknown
    }
}

fn parse_signal(value: &str) -> Result<u8> {
    let value = value.trim().strip_suffix('%').unwrap_or(value).trim();
    let percent = value.parse::<u8>().map_err(|_| {
        unrecognized_output("netsh wlan show interfaces (invalid signal percentage)")
    })?;
    if percent > 100 {
        return Err(unrecognized_output(
            "netsh wlan show interfaces (signal exceeds 100 percent)",
        ));
    }
    Ok(percent)
}

fn parse_channel(value: &str) -> Result<u32> {
    value
        .trim()
        .parse::<u32>()
        .map_err(|_| unrecognized_output("netsh wlan show interfaces (invalid channel number)"))
}

fn split_field(line: &str) -> Option<(&str, &str)> {
    let ascii = line.split_once(':');
    let full_width = line.split_once('：');
    let (label, value) = match (ascii, full_width) {
        (Some(ascii), Some(full_width)) => {
            if ascii.0.len() <= full_width.0.len() {
                ascii
            } else {
                full_width
            }
        }
        (Some(field), None) | (None, Some(field)) => field,
        (None, None) => return None,
    };
    Some((label.trim(), value.trim()))
}

fn trim_trailing_colon(value: &str) -> &str {
    value
        .strip_suffix(':')
        .or_else(|| value.strip_suffix('：'))
        .unwrap_or(value)
        .trim_end()
}

fn is_profile_section_heading(value: &str) -> bool {
    starts_with_ascii_case(value, "Group policy profiles")
        || value.eq_ignore_ascii_case("User profiles")
        || value.starts_with("组策略配置文件")
        || value == "用户配置文件"
}

fn is_profile_type_label(label: &str) -> bool {
    label_is(
        label,
        &[
            "alluserprofile",
            "currentuserprofile",
            "grouppolicyprofile",
            "所有用户配置文件",
            "当前用户配置文件",
            "组策略配置文件",
        ],
    )
}

fn strip_prefix_ascii_case<'a>(value: &'a str, prefix: &str) -> Option<&'a str> {
    let candidate = value.get(..prefix.len())?;
    candidate
        .eq_ignore_ascii_case(prefix)
        .then(|| &value[prefix.len()..])
}

fn starts_with_ascii_case(value: &str, prefix: &str) -> bool {
    strip_prefix_ascii_case(value, prefix).is_some()
}

fn label_is(label: &str, candidates: &[&str]) -> bool {
    normalized_eq_any(label, candidates)
}

fn normalized_label_eq(value: &str, candidate: &str) -> bool {
    normalized_eq(value, candidate)
}

fn normalized_eq_any(value: &str, candidates: &[&str]) -> bool {
    candidates
        .iter()
        .any(|candidate| normalized_eq(value, candidate))
}

fn normalized_eq(value: &str, candidate: &str) -> bool {
    value
        .chars()
        .filter(|character| !character.is_whitespace() && *character != '-' && *character != '_')
        .map(|character| character.to_ascii_lowercase())
        .eq(candidate.chars())
}

fn owned_non_empty(value: &str) -> Option<String> {
    let value = value.trim();
    (!value.is_empty()).then(|| value.to_owned())
}

fn unrecognized_output(operation: &str) -> LctrlError {
    LctrlError::ChannelUnavailable {
        channel: format!("{operation} returned unrecognized output"),
    }
}
