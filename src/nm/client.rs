//! High level operations on top of the NetworkManager D-Bus API.

use std::collections::HashMap;
use std::time::Duration;

use anyhow::{Context, Result, bail};
use futures_util::future::join_all;
use zbus::Connection;
use zbus::proxy::CacheProperties;
use zbus::zvariant::{ObjectPath, OwnedObjectPath, OwnedValue, Value};

use super::proxies::*;
use super::types::*;

// ------------------------------------------------------------------ properties

/// The result of `org.freedesktop.DBus.Properties.GetAll`, with lenient typed
/// accessors: a property NetworkManager did not return simply reads as a
/// default rather than aborting the whole refresh.
pub struct Props(HashMap<String, OwnedValue>);

impl Props {
    pub fn u32(&self, key: &str) -> u32 {
        self.0
            .get(key)
            .and_then(|v| v.downcast_ref::<u32>().ok())
            .unwrap_or(0)
    }

    pub fn u8(&self, key: &str) -> u8 {
        self.0
            .get(key)
            .and_then(|v| v.downcast_ref::<u8>().ok())
            .unwrap_or(0)
    }

    pub fn i32(&self, key: &str) -> i32 {
        self.0
            .get(key)
            .and_then(|v| v.downcast_ref::<i32>().ok())
            .unwrap_or(0)
    }

    pub fn i64(&self, key: &str) -> i64 {
        self.0
            .get(key)
            .and_then(|v| v.downcast_ref::<i64>().ok())
            .unwrap_or(0)
    }

    pub fn bool(&self, key: &str) -> bool {
        self.0
            .get(key)
            .and_then(|v| v.downcast_ref::<bool>().ok())
            .unwrap_or(false)
    }

    pub fn string(&self, key: &str) -> String {
        self.0
            .get(key)
            .and_then(|v| v.downcast_ref::<String>().ok())
            .unwrap_or_default()
    }

    pub fn bytes(&self, key: &str) -> Vec<u8> {
        self.0
            .get(key)
            .and_then(|v| Vec::<u8>::try_from(Value::from(v.clone())).ok())
            .unwrap_or_default()
    }

    /// Object paths: NetworkManager uses `/` as its "unset" sentinel.
    pub fn path(&self, key: &str) -> Option<OwnedObjectPath> {
        let p = self
            .0
            .get(key)
            .and_then(|v| OwnedObjectPath::try_from(v.clone()).ok())?;
        (p.as_str() != "/").then_some(p)
    }

    pub fn paths(&self, key: &str) -> Vec<OwnedObjectPath> {
        self.0
            .get(key)
            .and_then(|v| Vec::<OwnedObjectPath>::try_from(Value::from(v.clone())).ok())
            .unwrap_or_default()
    }

    /// The second field of a `(uu)` struct, as used by `Device.StateReason`.
    pub fn struct_u32_1(&self, key: &str) -> u32 {
        self.0
            .get(key)
            .and_then(|v| v.downcast_ref::<zbus::zvariant::Structure>().ok())
            .and_then(|s| s.fields().get(1).and_then(|f| f.downcast_ref::<u32>().ok()))
            .unwrap_or(0)
    }
}

// --------------------------------------------------------------- secret inputs

/// What the user typed in the join dialog.
#[derive(Debug, Clone)]
pub enum Secret {
    None,
    Passphrase(String),
    Enterprise { identity: String, password: String },
}

// --------------------------------------------------------------------- client

pub struct NmClient {
    conn: Connection,
    manager: ManagerProxy<'static>,
    settings: SettingsProxy<'static>,
}

impl NmClient {
    pub async fn new() -> Result<Self> {
        let conn = Connection::system()
            .await
            .context("connecting to the system bus")?;
        let manager = ManagerProxy::builder(&conn)
            .cache_properties(CacheProperties::No)
            .build()
            .await
            .context("reaching org.freedesktop.NetworkManager")?;
        let settings = SettingsProxy::builder(&conn)
            .cache_properties(CacheProperties::No)
            .build()
            .await?;
        Ok(Self {
            conn,
            manager,
            settings,
        })
    }

    pub fn connection(&self) -> &Connection {
        &self.conn
    }

    async fn get_all(&self, path: &ObjectPath<'_>, iface: &str) -> Result<Props> {
        let reply = self
            .conn
            .call_method(
                Some(NM_SERVICE),
                path,
                Some("org.freedesktop.DBus.Properties"),
                "GetAll",
                &(iface,),
            )
            .await?;
        let map: HashMap<String, OwnedValue> = reply.body().deserialize()?;
        Ok(Props(map))
    }

    // ------------------------------------------------------------- read state

    pub async fn snapshot(&self) -> Result<Snapshot> {
        let mgr = self
            .get_all(&ObjectPath::try_from(NM_PATH)?, IFACE_MANAGER)
            .await
            .context("reading NetworkManager state")?;

        let saved = self.saved_wifi_connections().await.unwrap_or_default();

        let mut devices = Vec::new();
        for path in self.manager.get_devices().await? {
            match self.read_device(&path, &saved).await {
                Ok(Some(dev)) => devices.push(dev),
                // A device can disappear between the listing and the read; that
                // is normal, not an error worth surfacing.
                Ok(None) | Err(_) => continue,
            }
        }
        devices.sort_by(|a, b| a.interface.cmp(&b.interface));

        Ok(Snapshot {
            version: mgr.string("Version"),
            state: Some(NmState::from(mgr.u32("State"))),
            connectivity: Some(Connectivity::from(mgr.u32("Connectivity"))),
            wireless_enabled: mgr.bool("WirelessEnabled"),
            wireless_hw_enabled: mgr.bool("WirelessHardwareEnabled"),
            networking_enabled: mgr.bool("NetworkingEnabled"),
            devices,
        })
    }

    async fn read_device(
        &self,
        path: &OwnedObjectPath,
        saved: &[SavedConnection],
    ) -> Result<Option<WifiDevice>> {
        let d = self.get_all(&path.as_ref(), IFACE_DEVICE).await?;
        if d.u32("DeviceType") != NM_DEVICE_TYPE_WIFI {
            return Ok(None);
        }
        let w = self.get_all(&path.as_ref(), IFACE_WIRELESS).await?;

        let active = match d.path("ActiveConnection") {
            Some(p) => self.read_active_connection(&p).await.ok(),
            None => None,
        };
        let ip4 = match d.path("Ip4Config") {
            Some(p) => self.read_ip4(&p).await.unwrap_or_default(),
            None => IpInfo::default(),
        };
        let ip6 = match d.path("Ip6Config") {
            Some(p) => self.read_ip6(&p).await.unwrap_or_default(),
            None => IpInfo::default(),
        };

        let active_ap = w.path("ActiveAccessPoint");

        // One round trip per access point, in flight together: a busy band can
        // carry a hundred of them, and walking the list serially is what makes a
        // refresh visibly lag behind the radio.
        let ap_paths = w.paths("AccessPoints");
        let aps: Vec<AccessPoint> = join_all(ap_paths.iter().map(|p| self.read_ap(p)))
            .await
            .into_iter()
            .flatten()
            .collect();

        let networks = aggregate(aps, active_ap.as_ref(), saved, &d.string("Interface"));

        Ok(Some(WifiDevice {
            path: path.clone(),
            interface: d.string("Interface"),
            driver: d.string("Driver"),
            driver_version: d.string("DriverVersion"),
            hw_address: d.string("HwAddress"),
            mtu: d.u32("Mtu"),
            state: DeviceState::from(d.u32("State")),
            state_reason: d.struct_u32_1("StateReason"),
            managed: d.bool("Managed"),
            autoconnect: d.bool("Autoconnect"),
            mode: ApMode::from(w.u32("Mode")),
            bitrate_kbps: w.u32("Bitrate"),
            last_scan_ms: w.i64("LastScan"),
            active,
            ip4,
            ip6,
            networks,
        }))
    }

    async fn read_ap(&self, path: &OwnedObjectPath) -> Result<AccessPoint> {
        let p = self.get_all(&path.as_ref(), IFACE_AP).await?;
        Ok(AccessPoint {
            path: path.clone(),
            bssid: p.string("HwAddress"),
            ssid: p.bytes("Ssid"),
            strength: p.u8("Strength"),
            frequency: p.u32("Frequency"),
            max_bitrate: p.u32("MaxBitrate"),
            mode: ApMode::from(p.u32("Mode")),
            flags: p.u32("Flags"),
            wpa_flags: p.u32("WpaFlags"),
            rsn_flags: p.u32("RsnFlags"),
            last_seen: p.i32("LastSeen"),
        })
    }

    async fn read_active_connection(&self, path: &OwnedObjectPath) -> Result<ActiveConnectionInfo> {
        let p = self.get_all(&path.as_ref(), IFACE_ACTIVE).await?;
        Ok(ActiveConnectionInfo {
            id: p.string("Id"),
            state: ActiveState::from(p.u32("State")),
            default_route: p.bool("Default"),
        })
    }

    async fn read_ip4(&self, path: &OwnedObjectPath) -> Result<IpInfo> {
        let proxy = Ip4ConfigProxy::builder(&self.conn)
            .path(path.clone())?
            .cache_properties(CacheProperties::No)
            .build()
            .await?;
        let addresses = proxy
            .address_data()
            .await
            .unwrap_or_default()
            .iter()
            .map(|m| {
                let a = m
                    .get("address")
                    .and_then(|v| v.downcast_ref::<String>().ok())
                    .unwrap_or_default();
                let p = m
                    .get("prefix")
                    .and_then(|v| v.downcast_ref::<u32>().ok())
                    .unwrap_or(0);
                format!("{a}/{p}")
            })
            .collect();
        let nameservers = proxy
            .nameserver_data()
            .await
            .unwrap_or_default()
            .iter()
            .filter_map(|m| m.get("address").and_then(|v| v.downcast_ref::<String>().ok()))
            .collect();
        let gateway = proxy.gateway().await.ok().filter(|g| !g.is_empty());
        Ok(IpInfo {
            addresses,
            gateway,
            nameservers,
            domains: proxy.domains().await.unwrap_or_default(),
        })
    }

    async fn read_ip6(&self, path: &OwnedObjectPath) -> Result<IpInfo> {
        let proxy = Ip6ConfigProxy::builder(&self.conn)
            .path(path.clone())?
            .cache_properties(CacheProperties::No)
            .build()
            .await?;
        let addresses = proxy
            .address_data()
            .await
            .unwrap_or_default()
            .iter()
            .map(|m| {
                let a = m
                    .get("address")
                    .and_then(|v| v.downcast_ref::<String>().ok())
                    .unwrap_or_default();
                let p = m
                    .get("prefix")
                    .and_then(|v| v.downcast_ref::<u32>().ok())
                    .unwrap_or(0);
                format!("{a}/{p}")
            })
            .collect();
        let nameservers = proxy
            .nameservers()
            .await
            .unwrap_or_default()
            .iter()
            .filter_map(|raw| format_ipv6(raw))
            .collect();
        Ok(IpInfo {
            addresses,
            gateway: proxy.gateway().await.ok().filter(|g| !g.is_empty()),
            nameservers,
            domains: Vec::new(),
        })
    }

    pub async fn saved_wifi_connections(&self) -> Result<Vec<SavedConnection>> {
        let mut out = Vec::new();
        for path in self.settings.list_connections().await? {
            let proxy = ConnectionProxy::builder(&self.conn)
                .path(path.clone())?
                .cache_properties(CacheProperties::No)
                .build()
                .await?;
            let Ok(s) = proxy.get_settings().await else {
                continue;
            };
            let conn = s.get("connection");
            let kind = conn
                .and_then(|c| c.get("type"))
                .and_then(|v| v.downcast_ref::<String>().ok())
                .unwrap_or_default();
            if kind != "802-11-wireless" {
                continue;
            }
            let wifi = s.get("802-11-wireless");
            out.push(SavedConnection {
                path,
                id: conn
                    .and_then(|c| c.get("id"))
                    .and_then(|v| v.downcast_ref::<String>().ok())
                    .unwrap_or_default(),
                ssid: wifi
                    .and_then(|w| w.get("ssid"))
                    .and_then(|v| Vec::<u8>::try_from(Value::from(v.clone())).ok())
                    .unwrap_or_default(),
                autoconnect: conn
                    .and_then(|c| c.get("autoconnect"))
                    .and_then(|v| v.downcast_ref::<bool>().ok())
                    .unwrap_or(true),
                interface_name: conn
                    .and_then(|c| c.get("interface-name"))
                    .and_then(|v| v.downcast_ref::<String>().ok())
                    .filter(|s| !s.is_empty()),
                hidden: wifi
                    .and_then(|w| w.get("hidden"))
                    .and_then(|v| v.downcast_ref::<bool>().ok())
                    .unwrap_or(false),
            });
        }
        Ok(out)
    }

    // ----------------------------------------------------------------- actions

    pub async fn set_wireless_enabled(&self, on: bool) -> Result<()> {
        self.manager
            .set_wireless_enabled(on)
            .await
            .context("toggling the Wi-Fi radio")?;
        Ok(())
    }

    pub async fn request_scan(&self, device: &OwnedObjectPath) -> Result<()> {
        let proxy = self.wireless(device).await?;
        // A failed read is "unknown", not a value: -1 is what NetworkManager
        // itself reports for a device that has never scanned, so folding the two
        // together would let a one-off read error read as a finished scan.
        let before = self
            .get_all(&device.as_ref(), IFACE_WIRELESS)
            .await
            .map(|p| p.i64("LastScan"))
            .ok();
        proxy
            .request_scan(HashMap::new())
            .await
            .context("requesting a scan")?;

        // RequestScan returns as soon as the request is queued; wait for
        // LastScan to move so the caller can refresh against fresh results.
        for _ in 0..60 {
            tokio::time::sleep(Duration::from_millis(250)).await;
            let now = self
                .get_all(&device.as_ref(), IFACE_WIRELESS)
                .await
                .map(|p| p.i64("LastScan"))
                .ok();
            if let (Some(before), Some(now)) = (before, now)
                && now != before
            {
                return Ok(());
            }
        }
        bail!("scan did not complete within 15s")
    }

    pub async fn disconnect(&self, device: &OwnedObjectPath) -> Result<()> {
        self.device(device)
            .await?
            .disconnect()
            .await
            .context("disconnecting the device")?;
        Ok(())
    }

    pub async fn set_device_autoconnect(&self, device: &OwnedObjectPath, on: bool) -> Result<()> {
        self.device(device)
            .await?
            .set_autoconnect(on)
            .await
            .context("setting device autoconnect")?;
        Ok(())
    }

    pub async fn set_device_managed(&self, device: &OwnedObjectPath, on: bool) -> Result<()> {
        self.device(device)
            .await?
            .set_managed(on)
            .await
            .context("setting device managed")?;
        Ok(())
    }

    pub async fn forget(&self, connection: &OwnedObjectPath) -> Result<()> {
        ConnectionProxy::builder(&self.conn)
            .path(connection.clone())?
            .cache_properties(CacheProperties::No)
            .build()
            .await?
            .delete()
            .await
            .context("deleting the saved connection")?;
        Ok(())
    }

    /// Activate a profile NetworkManager already has stored.
    pub async fn activate(
        &self,
        connection: &OwnedObjectPath,
        device: &OwnedObjectPath,
        ap: Option<&OwnedObjectPath>,
    ) -> Result<OwnedObjectPath> {
        let none = ObjectPath::try_from("/")?;
        let specific = ap.map(|p| p.as_ref()).unwrap_or(none);
        self.manager
            .activate_connection(&connection.as_ref(), &device.as_ref(), &specific)
            .await
            .context("activating the connection")
            .map_err(Into::into)
    }

    /// Create a profile from the details the user gave us and bring it up.
    pub async fn add_and_activate(
        &self,
        settings: ConnSettings,
        device: &OwnedObjectPath,
        ap: Option<&OwnedObjectPath>,
    ) -> Result<OwnedObjectPath> {
        let none = ObjectPath::try_from("/")?;
        let specific = ap.map(|p| p.as_ref()).unwrap_or(none);
        let (_conn, active) = self
            .manager
            .add_and_activate_connection(settings, &device.as_ref(), &specific)
            .await
            .context("creating and activating the connection")?;
        Ok(active)
    }

    /// Replace the secrets on an existing profile, so a saved network with a
    /// changed password can be rejoined without forgetting it first.
    pub async fn update_secrets(
        &self,
        connection: &OwnedObjectPath,
        security: Security,
        secret: &Secret,
    ) -> Result<()> {
        let proxy = ConnectionProxy::builder(&self.conn)
            .path(connection.clone())?
            .cache_properties(CacheProperties::No)
            .build()
            .await?;
        let existing = proxy.get_settings().await?;
        let mut settings: ConnSettings = existing
            .into_iter()
            .map(|(k, v)| {
                (
                    k,
                    v.into_iter()
                        .map(|(k2, v2)| (k2, Value::from(v2)))
                        .collect(),
                )
            })
            .collect();
        settings.remove("802-11-wireless-security");
        settings.remove("802-1x");
        for (group, values) in security_settings(security, secret)? {
            settings.insert(group, values);
        }
        proxy
            .update(settings)
            .await
            .context("updating stored secrets")?;
        Ok(())
    }

    /// Follow an activation to a verdict, so the UI can report *why* a join
    /// failed instead of leaving the user staring at a spinner.
    pub async fn wait_for_activation(
        &self,
        active: &OwnedObjectPath,
        device: &OwnedObjectPath,
    ) -> Result<()> {
        for _ in 0..180 {
            tokio::time::sleep(Duration::from_millis(250)).await;

            let dev = self.get_all(&device.as_ref(), IFACE_DEVICE).await?;
            let dev_state = DeviceState::from(dev.u32("State"));
            let reason = dev.struct_u32_1("StateReason");

            match self.get_all(&active.as_ref(), IFACE_ACTIVE).await {
                Ok(p) => match ActiveState::from(p.u32("State")) {
                    ActiveState::Activated => return Ok(()),
                    ActiveState::Deactivated => {
                        bail!("connection failed: {}", device_state_reason(reason))
                    }
                    _ => {}
                },
                // The active-connection object is torn down on failure.
                Err(_) => bail!("connection failed: {}", device_state_reason(reason)),
            }

            if dev_state == DeviceState::Failed {
                bail!("connection failed: {}", device_state_reason(reason));
            }
        }
        bail!("connection timed out")
    }

    async fn device(&self, path: &OwnedObjectPath) -> Result<DeviceProxy<'static>> {
        Ok(DeviceProxy::builder(&self.conn)
            .path(path.clone())?
            .cache_properties(CacheProperties::No)
            .build()
            .await?)
    }

    async fn wireless(&self, path: &OwnedObjectPath) -> Result<WirelessProxy<'static>> {
        Ok(WirelessProxy::builder(&self.conn)
            .path(path.clone())?
            .cache_properties(CacheProperties::No)
            .build()
            .await?)
    }
}

// ------------------------------------------------------------------ aggregation

/// Merge the raw AP list into one row per SSID, strongest AP first, tagged with
/// whichever stored profiles could be used to join it.
fn aggregate(
    aps: Vec<AccessPoint>,
    active_ap: Option<&OwnedObjectPath>,
    saved: &[SavedConnection],
    interface: &str,
) -> Vec<Network> {
    let mut by_ssid: HashMap<Vec<u8>, Vec<AccessPoint>> = HashMap::new();
    for ap in aps {
        by_ssid.entry(ap.ssid.clone()).or_default().push(ap);
    }

    let mut networks: Vec<Network> = by_ssid
        .into_iter()
        .map(|(ssid, mut aps)| {
            // The associated AP leads, so the details pane keeps showing the
            // radio we are actually on rather than flipping to whichever band
            // happens to read one percent stronger this second.
            aps.sort_by_key(|ap| {
                (
                    !active_ap.is_some_and(|p| &ap.path == p),
                    std::cmp::Reverse(ap.strength),
                )
            });
            let active = active_ap.is_some_and(|p| aps.iter().any(|ap| &ap.path == p));
            let saved = saved
                .iter()
                .filter(|s| {
                    s.ssid == ssid
                        && s.interface_name
                            .as_deref()
                            .is_none_or(|iface| iface == interface)
                })
                .cloned()
                .collect();
            Network {
                name: ssid_to_string(&ssid),
                ssid,
                aps,
                saved,
                active,
            }
        })
        .collect();

    networks.sort_by(|a, b| {
        b.active
            .cmp(&a.active)
            // Access points that broadcast no SSID all collapse into one row and
            // cannot be joined by selecting them, so they belong at the bottom
            // rather than pushing real networks off the screen.
            .then(a.is_hidden().cmp(&b.is_hidden()))
            .then(b.strength().cmp(&a.strength()))
            .then(a.name.cmp(&b.name))
    });
    networks
}

fn format_ipv6(raw: &[u8]) -> Option<String> {
    let bytes: [u8; 16] = raw.try_into().ok()?;
    Some(std::net::Ipv6Addr::from(bytes).to_string())
}

// ------------------------------------------------------- connection profiles

/// Build the profile NetworkManager needs to join `ssid` for the first time.
pub fn build_wifi_profile(
    ssid: &[u8],
    security: Security,
    secret: &Secret,
    hidden: bool,
    interface: &str,
) -> Result<ConnSettings> {
    let mut settings: ConnSettings = HashMap::new();

    let mut connection: HashMap<String, Value<'static>> = HashMap::new();
    connection.insert("id".into(), Value::from(ssid_to_string(ssid)));
    connection.insert("type".into(), Value::from("802-11-wireless"));
    connection.insert("autoconnect".into(), Value::from(true));
    connection.insert("interface-name".into(), Value::from(interface.to_string()));
    settings.insert("connection".into(), connection);

    let mut wireless: HashMap<String, Value<'static>> = HashMap::new();
    wireless.insert("ssid".into(), Value::from(ssid.to_vec()));
    wireless.insert("mode".into(), Value::from("infrastructure"));
    if hidden {
        wireless.insert("hidden".into(), Value::from(true));
    }
    settings.insert("802-11-wireless".into(), wireless);

    for (group, values) in security_settings(security, secret)? {
        settings.insert(group, values);
    }

    settings.insert(
        "ipv4".into(),
        HashMap::from([("method".to_string(), Value::from("auto"))]),
    );
    settings.insert(
        "ipv6".into(),
        HashMap::from([("method".to_string(), Value::from("auto"))]),
    );

    Ok(settings)
}

fn security_settings(
    security: Security,
    secret: &Secret,
) -> Result<Vec<(String, HashMap<String, Value<'static>>)>> {
    let mut out: Vec<(String, HashMap<String, Value<'static>>)> = Vec::new();
    let mut sec: HashMap<String, Value<'static>> = HashMap::new();

    match (security, secret) {
        (Security::Open, _) => return Ok(out),
        (Security::Owe, _) => {
            sec.insert("key-mgmt".into(), Value::from("owe"));
        }
        (Security::Wep, Secret::Passphrase(p)) => {
            sec.insert("key-mgmt".into(), Value::from("none"));
            sec.insert("auth-alg".into(), Value::from("open"));
            sec.insert("wep-key0".into(), Value::from(p.clone()));
            // A 10- or 26-character hex string is a raw key; anything else is
            // treated as a passphrase to be hashed.
            let is_hex_key =
                matches!(p.len(), 10 | 26) && p.chars().all(|c| c.is_ascii_hexdigit());
            sec.insert(
                "wep-key-type".into(),
                Value::from(if is_hex_key { 1u32 } else { 2u32 }),
            );
        }
        (Security::WpaPsk, Secret::Passphrase(p)) => {
            sec.insert("key-mgmt".into(), Value::from("wpa-psk"));
            sec.insert("psk".into(), Value::from(p.clone()));
        }
        (Security::Sae, Secret::Passphrase(p)) => {
            sec.insert("key-mgmt".into(), Value::from("sae"));
            sec.insert("psk".into(), Value::from(p.clone()));
        }
        (Security::Enterprise, Secret::Enterprise { identity, password }) => {
            sec.insert("key-mgmt".into(), Value::from("wpa-eap"));
            let eap: HashMap<String, Value<'static>> = HashMap::from([
                ("eap".to_string(), Value::from(vec!["peap".to_string()])),
                ("identity".to_string(), Value::from(identity.clone())),
                ("password".to_string(), Value::from(password.clone())),
                ("phase2-auth".to_string(), Value::from("mschapv2")),
            ]);
            out.push(("802-1x".into(), eap));
        }
        // Dropping a secret of the wrong shape would hand a secured SSID a
        // profile with no security group at all, which NetworkManager would
        // happily store and then fail to associate with. Say what is missing.
        (Security::Enterprise, _) => bail!("802.1X needs an identity and a password"),
        _ => bail!("{security} needs a passphrase"),
    }

    out.push(("802-11-wireless-security".into(), sec));
    Ok(out)
}

/// Which stored profile should we prefer when joining `network`?
pub fn preferred_saved(network: &Network) -> Option<&SavedConnection> {
    network
        .saved
        .iter()
        .find(|s| s.interface_name.is_some())
        .or_else(|| network.saved.first())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ap(path: &str, ssid: &[u8], strength: u8) -> AccessPoint {
        AccessPoint {
            path: OwnedObjectPath::try_from(path).unwrap(),
            bssid: path.into(),
            ssid: ssid.to_vec(),
            strength,
            frequency: 2412,
            max_bitrate: 130_000,
            mode: ApMode::Infra,
            flags: AP_FLAG_PRIVACY,
            wpa_flags: 0,
            rsn_flags: SEC_KEY_MGMT_PSK,
            last_seen: 0,
        }
    }

    fn saved(id: &str, ssid: &[u8], interface: Option<&str>) -> SavedConnection {
        SavedConnection {
            path: OwnedObjectPath::try_from("/conn/1").unwrap(),
            id: id.into(),
            ssid: ssid.to_vec(),
            autoconnect: true,
            interface_name: interface.map(Into::into),
            hidden: false,
        }
    }

    #[test]
    fn aps_sharing_an_ssid_become_one_row() {
        let nets = aggregate(
            vec![
                ap("/ap/1", b"home", 40),
                ap("/ap/2", b"home", 80),
                ap("/ap/3", b"cafe", 60),
            ],
            None,
            &[],
            "wlan0",
        );
        assert_eq!(nets.len(), 2);
        // Strongest network first, and within a row the strongest AP leads.
        assert_eq!(nets[0].name, "home");
        assert_eq!(nets[0].aps.len(), 2);
        assert_eq!(nets[0].best().strength, 80);
        assert_eq!(nets[1].name, "cafe");
    }

    #[test]
    fn the_connected_network_sorts_first_and_leads_with_its_own_ap() {
        let active = OwnedObjectPath::try_from("/ap/2").unwrap();
        let nets = aggregate(
            vec![
                ap("/ap/1", b"home", 90),
                ap("/ap/2", b"home", 30),
                ap("/ap/3", b"cafe", 99),
            ],
            Some(&active),
            &[],
            "wlan0",
        );
        assert_eq!(nets[0].name, "home");
        assert!(nets[0].active);
        assert_eq!(nets[0].best().path.as_str(), "/ap/2");
    }

    #[test]
    fn nameless_access_points_sort_below_real_networks() {
        let nets = aggregate(
            vec![
                ap("/ap/1", b"", 99),
                ap("/ap/2", b"", 98),
                ap("/ap/3", b"cafe", 20),
            ],
            None,
            &[],
            "wlan0",
        );
        assert_eq!(nets[0].name, "cafe");
        assert!(nets[1].is_hidden());
        assert_eq!(nets[1].aps.len(), 2);
    }

    #[test]
    fn profiles_pinned_to_another_interface_do_not_count_as_saved() {
        let nets = aggregate(
            vec![ap("/ap/1", b"home", 50)],
            None,
            &[
                saved("home-wlan1", b"home", Some("wlan1")),
                saved("home-any", b"home", None),
            ],
            "wlan0",
        );
        assert_eq!(nets[0].saved.len(), 1);
        assert_eq!(nets[0].saved[0].id, "home-any");
    }

    #[test]
    fn a_psk_profile_carries_the_ssid_and_key_management() {
        let profile = build_wifi_profile(
            b"home",
            Security::WpaPsk,
            &Secret::Passphrase("hunter2".into()),
            false,
            "wlan0",
        )
        .unwrap();
        assert_eq!(
            profile["connection"]["type"],
            Value::from("802-11-wireless")
        );
        assert_eq!(profile["802-11-wireless"]["ssid"], Value::from(b"home".to_vec()));
        assert!(!profile["802-11-wireless"].contains_key("hidden"));
        let sec = &profile["802-11-wireless-security"];
        assert_eq!(sec["key-mgmt"], Value::from("wpa-psk"));
        assert_eq!(sec["psk"], Value::from("hunter2"));
    }

    #[test]
    fn a_hidden_open_profile_has_no_security_group() {
        let profile =
            build_wifi_profile(b"lab", Security::Open, &Secret::None, true, "wlan0").unwrap();
        assert_eq!(profile["802-11-wireless"]["hidden"], Value::from(true));
        assert!(!profile.contains_key("802-11-wireless-security"));
    }

    #[test]
    fn wep_keys_are_told_apart_from_wep_passphrases() {
        let hex = build_wifi_profile(
            b"old",
            Security::Wep,
            &Secret::Passphrase("0123456789".into()),
            false,
            "wlan0",
        )
        .unwrap();
        assert_eq!(
            hex["802-11-wireless-security"]["wep-key-type"],
            Value::from(1u32)
        );
        let phrase = build_wifi_profile(
            b"old",
            Security::Wep,
            &Secret::Passphrase("open sesame".into()),
            false,
            "wlan0",
        )
        .unwrap();
        assert_eq!(
            phrase["802-11-wireless-security"]["wep-key-type"],
            Value::from(2u32)
        );
    }

    #[test]
    fn a_secured_network_never_yields_a_profile_without_a_security_group() {
        let err = build_wifi_profile(b"home", Security::WpaPsk, &Secret::None, false, "wlan0")
            .unwrap_err()
            .to_string();
        assert_eq!(err, "WPA2 needs a passphrase");
        assert!(
            build_wifi_profile(
                b"campus",
                Security::Enterprise,
                &Secret::Passphrase("pw".into()),
                false,
                "wlan0",
            )
            .is_err()
        );
    }

    #[test]
    fn an_enterprise_profile_gets_an_802_1x_group() {
        let profile = build_wifi_profile(
            b"campus",
            Security::Enterprise,
            &Secret::Enterprise {
                identity: "ada".into(),
                password: "pw".into(),
            },
            false,
            "wlan0",
        )
        .unwrap();
        assert_eq!(
            profile["802-11-wireless-security"]["key-mgmt"],
            Value::from("wpa-eap")
        );
        assert_eq!(profile["802-1x"]["identity"], Value::from("ada"));
        assert_eq!(profile["802-1x"]["phase2-auth"], Value::from("mschapv2"));
    }
}
