//! Plain data types mirroring the NetworkManager D-Bus API.

use std::fmt;

use zbus::zvariant::OwnedObjectPath;

// ---------------------------------------------------------------- device type

pub const NM_DEVICE_TYPE_WIFI: u32 = 2;

// --------------------------------------------------------------- manager state

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NmState {
    Unknown,
    Asleep,
    Disconnected,
    Disconnecting,
    Connecting,
    ConnectedLocal,
    ConnectedSite,
    ConnectedGlobal,
}

impl From<u32> for NmState {
    fn from(v: u32) -> Self {
        match v {
            10 => Self::Asleep,
            20 => Self::Disconnected,
            30 => Self::Disconnecting,
            40 => Self::Connecting,
            50 => Self::ConnectedLocal,
            60 => Self::ConnectedSite,
            70 => Self::ConnectedGlobal,
            _ => Self::Unknown,
        }
    }
}

impl fmt::Display for NmState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            Self::Unknown => "unknown",
            Self::Asleep => "asleep",
            Self::Disconnected => "disconnected",
            Self::Disconnecting => "disconnecting",
            Self::Connecting => "connecting",
            Self::ConnectedLocal => "link-local",
            Self::ConnectedSite => "site-only",
            Self::ConnectedGlobal => "connected",
        };
        f.write_str(s)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Connectivity {
    Unknown,
    None,
    Portal,
    Limited,
    Full,
}

impl From<u32> for Connectivity {
    fn from(v: u32) -> Self {
        match v {
            1 => Self::None,
            2 => Self::Portal,
            3 => Self::Limited,
            4 => Self::Full,
            _ => Self::Unknown,
        }
    }
}

impl fmt::Display for Connectivity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            Self::Unknown => "unknown",
            Self::None => "none",
            Self::Portal => "captive portal",
            Self::Limited => "limited",
            Self::Full => "full",
        };
        f.write_str(s)
    }
}

// ---------------------------------------------------------------- device state

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum DeviceState {
    Unknown,
    Unmanaged,
    Unavailable,
    Disconnected,
    Prepare,
    Config,
    NeedAuth,
    IpConfig,
    IpCheck,
    Secondaries,
    Activated,
    Deactivating,
    Failed,
}

impl From<u32> for DeviceState {
    fn from(v: u32) -> Self {
        match v {
            10 => Self::Unmanaged,
            20 => Self::Unavailable,
            30 => Self::Disconnected,
            40 => Self::Prepare,
            50 => Self::Config,
            60 => Self::NeedAuth,
            70 => Self::IpConfig,
            80 => Self::IpCheck,
            90 => Self::Secondaries,
            100 => Self::Activated,
            110 => Self::Deactivating,
            120 => Self::Failed,
            _ => Self::Unknown,
        }
    }
}

impl DeviceState {
    pub fn is_busy(self) -> bool {
        matches!(
            self,
            Self::Prepare
                | Self::Config
                | Self::NeedAuth
                | Self::IpConfig
                | Self::IpCheck
                | Self::Secondaries
                | Self::Deactivating
        )
    }

    pub fn is_connected(self) -> bool {
        self == Self::Activated
    }
}

impl fmt::Display for DeviceState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            Self::Unknown => "unknown",
            Self::Unmanaged => "unmanaged",
            Self::Unavailable => "unavailable",
            Self::Disconnected => "disconnected",
            Self::Prepare => "preparing",
            Self::Config => "configuring",
            Self::NeedAuth => "needs auth",
            Self::IpConfig => "getting IP",
            Self::IpCheck => "checking IP",
            Self::Secondaries => "secondaries",
            Self::Activated => "connected",
            Self::Deactivating => "disconnecting",
            Self::Failed => "failed",
        };
        f.write_str(s)
    }
}

/// `NMDeviceStateReason`, phrased for a status line. Values that cannot apply to
/// a Wi-Fi device (modem and GSM failures, mostly) fall through to the number,
/// which is still more use than a blank.
pub fn device_state_reason(v: u32) -> String {
    let s = match v {
        0 => "no reason",
        1 => "unknown error",
        2 => "device now managed",
        3 => "device now unmanaged",
        4 => "configuration failed",
        5 => "IP configuration unavailable",
        6 => "IP configuration expired",
        7 => "no secrets — wrong or missing password",
        8 => "supplicant disconnected",
        9 => "supplicant configuration failed",
        10 => "supplicant failed",
        11 => "supplicant timed out",
        15 => "DHCP failed to start",
        16 => "DHCP error",
        17 => "DHCP failed",
        18 => "shared connection failed to start",
        19 => "shared connection failed",
        20 => "link-local addressing failed to start",
        21 => "link-local addressing error",
        22 => "link-local addressing failed",
        35 => "firmware missing",
        36 => "device removed",
        37 => "system is sleeping",
        38 => "connection profile removed",
        39 => "disconnected by request",
        40 => "carrier changed",
        41 => "existing connection adopted",
        42 => "supplicant available",
        49 => "InfiniBand mode",
        50 => "a dependent connection failed",
        53 => "SSID not found",
        54 => "secondary connection failed",
        56 => "teamd control failed",
        60 => "superseded by a new activation",
        61 => "parent device changed",
        62 => "parent device management changed",
        63 => "OVSDB failed",
        64 => "duplicate IP address",
        65 => "unsupported IP method",
        66 => "SR-IOV configuration failed",
        67 => "peer not found",
        68 => "device handler failed",
        69 => "unmanaged by default",
        70 => "unmanaged: external interface is down",
        71 => "unmanaged: link not initialised",
        72 => "unmanaged: NetworkManager is quitting",
        73 => "unmanaged: sleeping",
        74 => "unmanaged by configuration",
        75 => "unmanaged by explicit request",
        76 => "unmanaged by user settings",
        77 => "unmanaged by udev rule",
        other => return format!("reason {other}"),
    };
    s.to_string()
}

// ------------------------------------------------------------ active connection

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActiveState {
    Unknown,
    Activating,
    Activated,
    Deactivating,
    Deactivated,
}

impl From<u32> for ActiveState {
    fn from(v: u32) -> Self {
        match v {
            1 => Self::Activating,
            2 => Self::Activated,
            3 => Self::Deactivating,
            4 => Self::Deactivated,
            _ => Self::Unknown,
        }
    }
}

impl fmt::Display for ActiveState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            Self::Unknown => "unknown",
            Self::Activating => "activating",
            Self::Activated => "activated",
            Self::Deactivating => "deactivating",
            Self::Deactivated => "deactivated",
        };
        f.write_str(s)
    }
}

// ------------------------------------------------------------------- 802.11 AP

pub const AP_FLAG_PRIVACY: u32 = 0x1;
pub const AP_FLAG_WPS: u32 = 0x2;

pub const SEC_PAIR_WEP40: u32 = 0x1;
pub const SEC_PAIR_WEP104: u32 = 0x2;
pub const SEC_PAIR_TKIP: u32 = 0x4;
pub const SEC_PAIR_CCMP: u32 = 0x8;
pub const SEC_GROUP_WEP40: u32 = 0x10;
pub const SEC_GROUP_WEP104: u32 = 0x20;
pub const SEC_GROUP_TKIP: u32 = 0x40;
pub const SEC_GROUP_CCMP: u32 = 0x80;
pub const SEC_KEY_MGMT_PSK: u32 = 0x100;
pub const SEC_KEY_MGMT_802_1X: u32 = 0x200;
pub const SEC_KEY_MGMT_SAE: u32 = 0x400;
pub const SEC_KEY_MGMT_OWE: u32 = 0x800;
pub const SEC_KEY_MGMT_OWE_TM: u32 = 0x1000;
pub const SEC_KEY_MGMT_EAP_SUITE_B_192: u32 = 0x2000;

/// What we need to know to build a connection profile for a network.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Security {
    Open,
    Owe,
    Wep,
    WpaPsk,
    Sae,
    Enterprise,
}

impl Security {
    pub fn from_flags(flags: u32, wpa: u32, rsn: u32) -> Self {
        let both = wpa | rsn;
        if both & (SEC_KEY_MGMT_802_1X | SEC_KEY_MGMT_EAP_SUITE_B_192) != 0 {
            Self::Enterprise
        } else if both & SEC_KEY_MGMT_PSK != 0 {
            Self::WpaPsk
        } else if rsn & SEC_KEY_MGMT_SAE != 0 {
            Self::Sae
        } else if both & (SEC_KEY_MGMT_OWE | SEC_KEY_MGMT_OWE_TM) != 0 {
            Self::Owe
        } else if flags & AP_FLAG_PRIVACY != 0 {
            Self::Wep
        } else {
            Self::Open
        }
    }

    /// Does joining this network require asking the user for something?
    pub fn needs_secret(self) -> bool {
        matches!(self, Self::Wep | Self::WpaPsk | Self::Sae | Self::Enterprise)
    }
}

impl fmt::Display for Security {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            Self::Open => "open",
            Self::Owe => "OWE",
            Self::Wep => "WEP",
            Self::WpaPsk => "WPA2",
            Self::Sae => "WPA3",
            Self::Enterprise => "802.1X",
        };
        f.write_str(s)
    }
}

/// Long-form security description for the details pane.
pub fn describe_security(flags: u32, wpa: u32, rsn: u32) -> String {
    let mut parts: Vec<String> = Vec::new();
    if wpa != 0 {
        parts.push(format!("WPA({})", describe_sec_flags(wpa)));
    }
    if rsn != 0 {
        parts.push(format!("RSN({})", describe_sec_flags(rsn)));
    }
    if parts.is_empty() {
        parts.push(if flags & AP_FLAG_PRIVACY != 0 {
            "WEP".into()
        } else {
            "none".into()
        });
    }
    if flags & AP_FLAG_WPS != 0 {
        parts.push("WPS".into());
    }
    parts.join(" ")
}

fn describe_sec_flags(f: u32) -> String {
    let mut key = Vec::new();
    if f & SEC_KEY_MGMT_PSK != 0 {
        key.push("psk");
    }
    if f & SEC_KEY_MGMT_SAE != 0 {
        key.push("sae");
    }
    if f & SEC_KEY_MGMT_802_1X != 0 {
        key.push("802.1x");
    }
    if f & SEC_KEY_MGMT_EAP_SUITE_B_192 != 0 {
        key.push("eap-suite-b");
    }
    if f & (SEC_KEY_MGMT_OWE | SEC_KEY_MGMT_OWE_TM) != 0 {
        key.push("owe");
    }
    let mut ciphers = Vec::new();
    if f & (SEC_PAIR_CCMP | SEC_GROUP_CCMP) != 0 {
        ciphers.push("ccmp");
    }
    if f & (SEC_PAIR_TKIP | SEC_GROUP_TKIP) != 0 {
        ciphers.push("tkip");
    }
    if f & (SEC_PAIR_WEP40 | SEC_GROUP_WEP40 | SEC_PAIR_WEP104 | SEC_GROUP_WEP104) != 0 {
        ciphers.push("wep");
    }
    if key.is_empty() && ciphers.is_empty() {
        return "-".into();
    }
    format!("{}/{}", key.join("+"), ciphers.join("+"))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ApMode {
    Unknown,
    AdHoc,
    Infra,
    Ap,
    Mesh,
}

impl From<u32> for ApMode {
    fn from(v: u32) -> Self {
        match v {
            1 => Self::AdHoc,
            2 => Self::Infra,
            3 => Self::Ap,
            4 => Self::Mesh,
            _ => Self::Unknown,
        }
    }
}

impl fmt::Display for ApMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            Self::Unknown => "unknown",
            Self::AdHoc => "ad-hoc",
            Self::Infra => "infrastructure",
            Self::Ap => "access point",
            Self::Mesh => "mesh",
        };
        f.write_str(s)
    }
}

/// 802.11 channel number for a centre frequency in MHz.
pub fn channel_for_freq(mhz: u32) -> u32 {
    match mhz {
        2484 => 14,
        2412..=2472 => (mhz - 2407) / 5,
        5160..=5885 => (mhz - 5000) / 5,
        5955..=7115 => (mhz - 5950) / 5,
        _ => 0,
    }
}

pub fn band_for_freq(mhz: u32) -> &'static str {
    match mhz {
        0..=2500 => "2.4 GHz",
        2501..=5900 => "5 GHz",
        5901..=7200 => "6 GHz",
        _ => "60 GHz",
    }
}

/// SSIDs are raw bytes; render them without letting control characters loose in
/// the terminal.
pub fn ssid_to_string(raw: &[u8]) -> String {
    if raw.is_empty() {
        return String::new();
    }
    match std::str::from_utf8(raw) {
        Ok(s) => s
            .chars()
            .map(|c| if c.is_control() { '\u{fffd}' } else { c })
            .collect(),
        Err(_) => raw.iter().map(|b| format!("\\x{b:02x}")).collect(),
    }
}

// -------------------------------------------------------------------- snapshot

#[derive(Debug, Clone)]
pub struct AccessPoint {
    pub path: OwnedObjectPath,
    pub bssid: String,
    pub ssid: Vec<u8>,
    pub strength: u8,
    pub frequency: u32,
    pub max_bitrate: u32,
    pub mode: ApMode,
    pub flags: u32,
    pub wpa_flags: u32,
    pub rsn_flags: u32,
    pub last_seen: i32,
}

impl AccessPoint {
    pub fn security(&self) -> Security {
        Security::from_flags(self.flags, self.wpa_flags, self.rsn_flags)
    }

    pub fn channel(&self) -> u32 {
        channel_for_freq(self.frequency)
    }

    pub fn band(&self) -> &'static str {
        band_for_freq(self.frequency)
    }
}

#[derive(Debug, Clone)]
pub struct SavedConnection {
    pub path: OwnedObjectPath,
    pub id: String,
    pub ssid: Vec<u8>,
    pub autoconnect: bool,
    pub interface_name: Option<String>,
    pub hidden: bool,
}

/// One row in the network list: every AP sharing an SSID, merged.
#[derive(Debug, Clone)]
pub struct Network {
    pub ssid: Vec<u8>,
    pub name: String,
    pub aps: Vec<AccessPoint>,
    pub saved: Vec<SavedConnection>,
    pub active: bool,
}

impl Network {
    pub fn best(&self) -> &AccessPoint {
        &self.aps[0]
    }

    pub fn strength(&self) -> u8 {
        self.best().strength
    }

    pub fn security(&self) -> Security {
        self.best().security()
    }

    pub fn is_hidden(&self) -> bool {
        self.ssid.is_empty()
    }
}

#[derive(Debug, Clone, Default)]
pub struct IpInfo {
    pub addresses: Vec<String>,
    pub gateway: Option<String>,
    pub nameservers: Vec<String>,
    pub domains: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct ActiveConnectionInfo {
    pub id: String,
    pub state: ActiveState,
    pub default_route: bool,
}

#[derive(Debug, Clone)]
pub struct WifiDevice {
    pub path: OwnedObjectPath,
    pub interface: String,
    pub driver: String,
    pub driver_version: String,
    pub hw_address: String,
    pub mtu: u32,
    pub state: DeviceState,
    pub state_reason: u32,
    pub managed: bool,
    pub autoconnect: bool,
    pub mode: ApMode,
    pub bitrate_kbps: u32,
    pub last_scan_ms: i64,
    pub active: Option<ActiveConnectionInfo>,
    pub ip4: IpInfo,
    pub ip6: IpInfo,
    pub networks: Vec<Network>,
}

impl WifiDevice {
    pub fn active_network(&self) -> Option<&Network> {
        self.networks.iter().find(|n| n.active)
    }
}

#[derive(Debug, Clone, Default)]
pub struct Snapshot {
    pub version: String,
    pub state: Option<NmState>,
    pub connectivity: Option<Connectivity>,
    pub wireless_enabled: bool,
    pub wireless_hw_enabled: bool,
    pub networking_enabled: bool,
    pub devices: Vec<WifiDevice>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn channels_map_across_the_bands() {
        assert_eq!(channel_for_freq(2412), 1);
        assert_eq!(channel_for_freq(2427), 4);
        assert_eq!(channel_for_freq(2484), 14);
        assert_eq!(channel_for_freq(5200), 40);
        assert_eq!(channel_for_freq(5955), 1);
        assert_eq!(band_for_freq(2427), "2.4 GHz");
        assert_eq!(band_for_freq(5200), "5 GHz");
        assert_eq!(band_for_freq(5955), "6 GHz");
    }

    #[test]
    fn security_is_read_off_the_ap_flags() {
        assert_eq!(Security::from_flags(0, 0, 0), Security::Open);
        assert_eq!(Security::from_flags(AP_FLAG_PRIVACY, 0, 0), Security::Wep);
        assert_eq!(
            Security::from_flags(AP_FLAG_PRIVACY, SEC_KEY_MGMT_PSK, 0),
            Security::WpaPsk
        );
        // WPA3-only networks are the ones that need `sae` rather than `wpa-psk`.
        assert_eq!(
            Security::from_flags(AP_FLAG_PRIVACY, 0, SEC_KEY_MGMT_SAE),
            Security::Sae
        );
        // A transition-mode AP advertises both; PSK is the interoperable choice.
        assert_eq!(
            Security::from_flags(AP_FLAG_PRIVACY, 0, SEC_KEY_MGMT_SAE | SEC_KEY_MGMT_PSK),
            Security::WpaPsk
        );
        assert_eq!(
            Security::from_flags(AP_FLAG_PRIVACY, 0, SEC_KEY_MGMT_802_1X),
            Security::Enterprise
        );
        assert_eq!(Security::from_flags(0, 0, SEC_KEY_MGMT_OWE), Security::Owe);
        assert!(!Security::Open.needs_secret());
        assert!(Security::Sae.needs_secret());
    }

    #[test]
    fn security_description_names_ciphers() {
        assert_eq!(
            describe_security(AP_FLAG_PRIVACY, 0, SEC_KEY_MGMT_PSK | SEC_PAIR_CCMP),
            "RSN(psk/ccmp)"
        );
        assert_eq!(describe_security(0, 0, 0), "none");
        assert_eq!(describe_security(AP_FLAG_PRIVACY, 0, 0), "WEP");
    }

    #[test]
    fn ssids_are_rendered_without_letting_control_bytes_through() {
        assert_eq!(ssid_to_string(b"home wifi"), "home wifi");
        assert_eq!(ssid_to_string(b""), "");
        assert_eq!(ssid_to_string(b"a\x07b"), "a\u{fffd}b");
        // Not every SSID is UTF-8; show the bytes rather than mangling them.
        assert_eq!(ssid_to_string(&[0xff, 0xfe]), "\\xff\\xfe");
    }
}
