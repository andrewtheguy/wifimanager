//! Application state and input handling.

use std::sync::Arc;
use std::time::{Duration, Instant};

use crossterm::event::{Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use tokio::sync::mpsc::Sender;
use zbus::zvariant::OwnedObjectPath;

use crate::nm::client::Secret;
use crate::nm::{NmClient, Security, Snapshot, WifiDevice, build_wifi_profile, preferred_saved};
use crate::nm::types::Network;

// ---------------------------------------------------------------- app messages

#[derive(Debug)]
pub enum Msg {
    Snapshot(Box<Snapshot>),
    Status(Status),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Level {
    Info,
    Ok,
    Error,
}

#[derive(Debug, Clone)]
pub struct Status {
    pub level: Level,
    pub text: String,
    /// Set while a long-running action is in flight, so the UI can spin.
    pub busy: bool,
}

impl Status {
    pub fn info(text: impl Into<String>) -> Self {
        Self { level: Level::Info, text: text.into(), busy: false }
    }
    pub fn busy(text: impl Into<String>) -> Self {
        Self { level: Level::Info, text: text.into(), busy: true }
    }
    pub fn ok(text: impl Into<String>) -> Self {
        Self { level: Level::Ok, text: text.into(), busy: false }
    }
    pub fn error(text: impl Into<String>) -> Self {
        Self { level: Level::Error, text: text.into(), busy: false }
    }
}

// -------------------------------------------------------------------- modals

#[derive(Debug, Clone)]
pub struct Field {
    pub label: &'static str,
    pub value: String,
    pub secret: bool,
}

#[derive(Debug, Clone)]
pub enum PromptKind {
    /// Create a new profile for a visible network and join it.
    Join {
        device: OwnedObjectPath,
        interface: String,
        ssid: Vec<u8>,
        security: Security,
        ap: Option<OwnedObjectPath>,
    },
    /// Create a profile for a network that is not broadcasting its SSID.
    JoinHidden {
        device: OwnedObjectPath,
        interface: String,
    },
    /// Rewrite the secrets on a stored profile, then bring it up.
    Rekey {
        device: OwnedObjectPath,
        connection: OwnedObjectPath,
        security: Security,
        ap: Option<OwnedObjectPath>,
    },
}

#[derive(Debug, Clone)]
pub struct Prompt {
    pub title: String,
    pub hint: String,
    pub fields: Vec<Field>,
    pub focus: usize,
    pub reveal: bool,
    pub kind: PromptKind,
}

#[derive(Debug, Clone)]
pub enum ConfirmKind {
    Forget { name: String, connections: Vec<OwnedObjectPath> },
    /// Disabling outlives the session, so it is the one device action that
    /// asks first.
    Disable {
        device: OwnedObjectPath,
        interface: String,
        /// The profile the device is carrying, if any.
        carrying: Option<String>,
    },
}

#[derive(Debug, Clone)]
pub enum Modal {
    None,
    Help,
    Prompt(Prompt),
    Confirm(ConfirmKind),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Focus {
    Devices,
    Networks,
}

// ----------------------------------------------------------------------- app

pub struct App {
    client: Arc<NmClient>,
    tx: Sender<Msg>,
    poke: Sender<()>,

    pub snapshot: Snapshot,
    pub first_load: bool,
    pub focus: Focus,
    pub modal: Modal,
    pub status: Status,
    pub status_at: Instant,
    pub quit: bool,

    /// Selections are held as identities rather than indices so a refresh that
    /// reorders the lists does not move the cursor out from under the user.
    selected_device: Option<OwnedObjectPath>,
    selected_ssid: Option<Vec<u8>>,
}

impl App {
    pub fn new(client: Arc<NmClient>, tx: Sender<Msg>, poke: Sender<()>) -> Self {
        Self {
            client,
            tx,
            poke,
            snapshot: Snapshot::default(),
            first_load: true,
            focus: Focus::Networks,
            modal: Modal::None,
            status: Status::info("connecting to NetworkManager…"),
            status_at: Instant::now(),
            quit: false,
            selected_device: None,
            selected_ssid: None,
        }
    }

    // ---------------------------------------------------------- selection

    pub fn devices(&self) -> &[WifiDevice] {
        &self.snapshot.devices
    }

    pub fn device_index(&self) -> usize {
        self.selected_device
            .as_ref()
            .and_then(|p| self.devices().iter().position(|d| &d.path == p))
            .unwrap_or(0)
    }

    pub fn device(&self) -> Option<&WifiDevice> {
        self.devices().get(self.device_index())
    }

    pub fn networks(&self) -> &[Network] {
        self.device().map(|d| d.networks.as_slice()).unwrap_or(&[])
    }

    pub fn network_index(&self) -> usize {
        self.selected_ssid
            .as_ref()
            .and_then(|s| self.networks().iter().position(|n| &n.ssid == s))
            .unwrap_or(0)
    }

    pub fn network(&self) -> Option<&Network> {
        self.networks().get(self.network_index())
    }

    fn select_device(&mut self, idx: usize) {
        if let Some(d) = self.devices().get(idx) {
            self.selected_device = Some(d.path.clone());
            self.selected_ssid = None;
        }
    }

    fn select_network(&mut self, idx: usize) {
        if let Some(n) = self.networks().get(idx) {
            self.selected_ssid = Some(n.ssid.clone());
        }
    }

    fn move_selection(&mut self, delta: isize) {
        match self.focus {
            Focus::Devices => {
                let len = self.devices().len();
                if len == 0 {
                    return;
                }
                let idx = clamp_move(self.device_index(), delta, len);
                self.select_device(idx);
            }
            Focus::Networks => {
                let len = self.networks().len();
                if len == 0 {
                    return;
                }
                let idx = clamp_move(self.network_index(), delta, len);
                self.select_network(idx);
            }
        }
    }

    // ------------------------------------------------------------ messages

    pub fn on_msg(&mut self, msg: Msg) {
        match msg {
            Msg::Snapshot(s) => {
                let was_loading = self.first_load;
                self.snapshot = *s;
                self.first_load = false;
                if self.selected_device.is_none() {
                    self.select_device(0);
                }
                // Pin the network cursor to a specific SSID rather than leaving
                // it at "whatever is row 0", so a rescan that reorders the list
                // cannot move it out from under the user mid-keystroke.
                let adrift = self
                    .selected_ssid
                    .as_ref()
                    .is_none_or(|s| !self.networks().iter().any(|n| &n.ssid == s));
                if adrift {
                    self.select_network(0);
                }
                // Retire the start-up placeholder once there is something to
                // look at; anything else on screen is the user's own business.
                if was_loading {
                    self.status = Status::info(String::new());
                }
            }
            Msg::Status(s) => {
                self.status = s;
                self.status_at = Instant::now();
            }
        }
    }

    pub fn status_visible(&self) -> bool {
        self.status.busy
            || self.status.level == Level::Error
            || self.status_at.elapsed() < Duration::from_secs(6)
    }

    fn set_status(&mut self, s: Status) {
        self.status = s;
        self.status_at = Instant::now();
    }

    /// Run a fallible action off the UI thread, reporting the outcome in the
    /// status bar and refreshing the view when it lands.
    fn spawn<F>(&self, pending: &str, done: String, fut: F)
    where
        F: std::future::Future<Output = anyhow::Result<()>> + Send + 'static,
    {
        let tx = self.tx.clone();
        let poke = self.poke.clone();
        let _ = tx.try_send(Msg::Status(Status::busy(pending)));
        tokio::spawn(async move {
            let status = match fut.await {
                Ok(()) => Status::ok(done),
                Err(e) => Status::error(format_error(&e)),
            };
            let _ = tx.send(Msg::Status(status)).await;
            let _ = poke.send(()).await;
        });
    }

    // -------------------------------------------------------------- input

    pub fn on_event(&mut self, ev: Event) {
        let Event::Key(key) = ev else { return };
        if key.kind == KeyEventKind::Release {
            return;
        }
        if key.modifiers.contains(KeyModifiers::CONTROL) && matches!(key.code, KeyCode::Char('c')) {
            self.quit = true;
            return;
        }
        match std::mem::replace(&mut self.modal, Modal::None) {
            Modal::None => self.on_key_normal(key),
            // Any key dismisses the help overlay; taking it out of `self.modal`
            // above is the whole action.
            Modal::Help => {}
            Modal::Prompt(p) => self.on_key_prompt(key, p),
            Modal::Confirm(c) => self.on_key_confirm(key, c),
        }
    }

    fn on_key_normal(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Char('q') => self.quit = true,
            KeyCode::Char('?') => self.modal = Modal::Help,
            KeyCode::Esc => self.set_status(Status::info("")),
            KeyCode::Tab | KeyCode::BackTab => {
                self.focus = match self.focus {
                    Focus::Devices => Focus::Networks,
                    Focus::Networks => Focus::Devices,
                }
            }
            KeyCode::Left | KeyCode::Char('h') => self.focus = Focus::Devices,
            KeyCode::Right | KeyCode::Char('l') => self.focus = Focus::Networks,
            KeyCode::Down | KeyCode::Char('j') => self.move_selection(1),
            KeyCode::Up | KeyCode::Char('k') => self.move_selection(-1),
            KeyCode::PageDown => self.move_selection(10),
            KeyCode::PageUp => self.move_selection(-10),
            KeyCode::Home | KeyCode::Char('g') => self.move_selection(isize::MIN / 2),
            KeyCode::End | KeyCode::Char('G') => self.move_selection(isize::MAX / 2),
            KeyCode::Enter => self.join_selected(false),
            KeyCode::Char('p') => self.join_selected(true),
            KeyCode::Char('n') => self.join_hidden(),
            KeyCode::Char('w') => self.toggle_radio(),
            KeyCode::Char('s') | KeyCode::Char('r') => self.scan(),
            KeyCode::Char('d') => self.disconnect(),
            KeyCode::Char('f') => self.forget(),
            KeyCode::Char('a') => self.toggle_device_autoconnect(),
            KeyCode::Char('e') => self.toggle_device_enabled(),
            _ => {}
        }
    }

    fn on_key_prompt(&mut self, key: KeyEvent, mut p: Prompt) {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        match key.code {
            KeyCode::Esc => return,
            KeyCode::Enter => {
                self.submit_prompt(p);
                return;
            }
            KeyCode::Tab | KeyCode::Down => p.focus = (p.focus + 1) % p.fields.len(),
            KeyCode::BackTab | KeyCode::Up => {
                p.focus = (p.focus + p.fields.len() - 1) % p.fields.len()
            }
            KeyCode::Backspace => {
                p.fields[p.focus].value.pop();
            }
            KeyCode::Char('u') if ctrl => p.fields[p.focus].value.clear(),
            KeyCode::Char('r') if ctrl => p.reveal = !p.reveal,
            KeyCode::Char(c) if !ctrl => p.fields[p.focus].value.push(c),
            _ => {}
        }
        self.modal = Modal::Prompt(p);
    }

    fn on_key_confirm(&mut self, key: KeyEvent, c: ConfirmKind) {
        match key.code {
            KeyCode::Char('y') | KeyCode::Char('Y') => match c {
                ConfirmKind::Forget { name, connections } => {
                    let client = self.client.clone();
                    self.spawn(
                        format!("forgetting {name}…").as_str(),
                        format!("forgot {name}"),
                        async move {
                            for path in connections {
                                client.forget(&path).await?;
                            }
                            Ok(())
                        },
                    );
                }
                ConfirmKind::Disable { device, interface, .. } => {
                    let client = self.client.clone();
                    self.spawn(
                        format!("disabling {interface}…").as_str(),
                        format!("{interface} disabled"),
                        async move { client.disable_device(&device, &interface).await },
                    );
                }
            },
            _ => {}
        }
    }

    // ------------------------------------------------------------ actions

    fn toggle_radio(&mut self) {
        if !self.snapshot.wireless_hw_enabled {
            self.set_status(Status::error(
                "Wi-Fi is blocked by a hardware switch or rfkill",
            ));
            return;
        }
        let on = !self.snapshot.wireless_enabled;
        let client = self.client.clone();
        self.spawn(
            if on { "enabling Wi-Fi…" } else { "disabling Wi-Fi…" },
            format!("Wi-Fi {}", if on { "enabled" } else { "disabled" }),
            async move { client.set_wireless_enabled(on).await },
        );
    }

    fn scan(&mut self) {
        let Some(dev) = self.device() else {
            self.set_status(Status::error("no Wi-Fi device selected"));
            return;
        };
        if !self.snapshot.wireless_enabled {
            self.set_status(Status::error("Wi-Fi is off — press w to enable it"));
            return;
        }
        if !dev.enabled() {
            self.set_status(Status::error(format!(
                "{} is disabled — press e to enable it",
                dev.interface
            )));
            return;
        }
        let (path, iface) = (dev.path.clone(), dev.interface.clone());
        let client = self.client.clone();
        self.spawn(
            format!("scanning on {iface}…").as_str(),
            format!("scan complete on {iface}"),
            async move { client.request_scan(&path).await },
        );
    }

    fn disconnect(&mut self) {
        let Some(dev) = self.device() else { return };
        if dev.active.is_none() && !dev.state.is_connected() {
            self.set_status(Status::info(format!("{} is not connected", dev.interface)));
            return;
        }
        let (path, iface) = (dev.path.clone(), dev.interface.clone());
        let client = self.client.clone();
        self.spawn(
            format!("disconnecting {iface}…").as_str(),
            format!("{iface} disconnected"),
            async move { client.disconnect(&path).await },
        );
    }

    fn toggle_device_autoconnect(&mut self) {
        let Some(dev) = self.device() else { return };
        let (path, iface, on) = (dev.path.clone(), dev.interface.clone(), !dev.autoconnect);
        let client = self.client.clone();
        self.spawn(
            "updating device…",
            format!("{iface} autoconnect {}", on_off(on)),
            async move { client.set_device_autoconnect(&path, on).await },
        );
    }

    /// Enable takes effect at once; disable is confirmed first because it
    /// holds across reboots.
    fn toggle_device_enabled(&mut self) {
        let Some(dev) = self.device() else { return };
        let (device, interface) = (dev.path.clone(), dev.interface.clone());
        if dev.enabled() {
            self.modal = Modal::Confirm(ConfirmKind::Disable {
                device,
                interface,
                carrying: dev.active.as_ref().map(|a| a.id.clone()),
            });
            return;
        }
        let client = self.client.clone();
        self.spawn(
            format!("enabling {interface}…").as_str(),
            format!("{interface} enabled"),
            async move { client.enable_device(&device, &interface).await },
        );
    }

    fn forget(&mut self) {
        let Some(net) = self.network() else { return };
        if net.saved.is_empty() {
            self.set_status(Status::info(format!("no saved profile for {}", net.name)));
            return;
        }
        self.modal = Modal::Confirm(ConfirmKind::Forget {
            name: display_name(net),
            connections: net.saved.iter().map(|s| s.path.clone()).collect(),
        });
    }

    /// `rekey` forces the password prompt even when a profile is already stored,
    /// which is how you recover from a changed Wi-Fi password.
    fn join_selected(&mut self, rekey: bool) {
        if self.focus == Focus::Devices {
            self.focus = Focus::Networks;
            return;
        }
        let Some(dev) = self.device() else { return };
        let (device, interface) = (dev.path.clone(), dev.interface.clone());
        let Some(net) = self.network() else {
            self.set_status(Status::info("nothing to join — press s to scan"));
            return;
        };
        if net.active && !rekey {
            self.set_status(Status::info(format!(
                "already connected to {}",
                display_name(net)
            )));
            return;
        }
        if net.is_hidden() {
            self.set_status(Status::info(
                "that entry hides its SSID — press n to join it by name",
            ));
            return;
        }

        let security = net.security();
        let ap = Some(net.best().path.clone());
        let name = display_name(net);
        let ssid = net.ssid.clone();
        let saved = preferred_saved(net).cloned();

        // Ask for a secret when there is no stored profile to lean on, or when
        // the user explicitly asked to re-enter one.
        if security.needs_secret() && (saved.is_none() || rekey) {
            let (title, kind) = match &saved {
                Some(s) => (
                    format!("Reconnect to {name}"),
                    PromptKind::Rekey {
                        device,
                        connection: s.path.clone(),
                        security,
                        ap,
                    },
                ),
                None => (
                    format!("Join {name}"),
                    PromptKind::Join {
                        device,
                        interface,
                        ssid: ssid.clone(),
                        security,
                        ap,
                    },
                ),
            };
            self.modal = Modal::Prompt(secret_prompt(title, security, kind));
            return;
        }

        let client = self.client.clone();
        let pending = format!("connecting to {name}…");
        let done = format!("connected to {name}");
        match saved {
            Some(saved) => {
                let conn = saved.path.clone();
                self.spawn(pending.as_str(), done, async move {
                    let active = client.activate(&conn, &device, ap.as_ref()).await?;
                    client.wait_for_activation(&active, &device).await
                });
            }
            None => {
                let profile =
                    match build_wifi_profile(&ssid, security, &Secret::None, false, &interface) {
                        Ok(profile) => profile,
                        Err(e) => {
                            self.set_status(Status::error(format_error(&e)));
                            return;
                        }
                    };
                self.spawn(pending.as_str(), done, async move {
                    let active = client.add_and_activate(profile, &device, ap.as_ref()).await?;
                    client.wait_for_activation(&active, &device).await
                });
            }
        }
    }

    fn join_hidden(&mut self) {
        let Some(dev) = self.device() else {
            self.set_status(Status::error("no Wi-Fi device selected"));
            return;
        };
        self.modal = Modal::Prompt(Prompt {
            title: "Join a hidden network".into(),
            hint: "leave the password empty for an open network".into(),
            fields: vec![
                Field { label: "SSID", value: String::new(), secret: false },
                Field { label: "Password", value: String::new(), secret: true },
            ],
            focus: 0,
            reveal: false,
            kind: PromptKind::JoinHidden {
                device: dev.path.clone(),
                interface: dev.interface.clone(),
            },
        });
    }

    fn submit_prompt(&mut self, p: Prompt) {
        let client = self.client.clone();
        match p.kind.clone() {
            PromptKind::Join { device, interface, ssid, security, ap } => {
                let Some(secret) = secret_from(&p, security) else {
                    self.modal = Modal::Prompt(reject(p, "a password is required"));
                    return;
                };
                let name = crate::nm::ssid_to_string(&ssid);
                let profile = match build_wifi_profile(&ssid, security, &secret, false, &interface) {
                    Ok(profile) => profile,
                    Err(e) => {
                        self.modal = Modal::Prompt(reject(p, &format_error(&e)));
                        return;
                    }
                };
                self.spawn(
                    format!("connecting to {name}…").as_str(),
                    format!("connected to {name}"),
                    async move {
                        let active = client.add_and_activate(profile, &device, ap.as_ref()).await?;
                        client.wait_for_activation(&active, &device).await
                    },
                );
            }
            PromptKind::Rekey { device, connection, security, ap } => {
                let Some(secret) = secret_from(&p, security) else {
                    self.modal = Modal::Prompt(reject(p, "a password is required"));
                    return;
                };
                self.spawn("reconnecting…", "connected".into(), async move {
                    client.update_secrets(&connection, security, &secret).await?;
                    let active = client.activate(&connection, &device, ap.as_ref()).await?;
                    client.wait_for_activation(&active, &device).await
                });
            }
            PromptKind::JoinHidden { device, interface } => {
                let ssid = p.fields[0].value.trim().to_string();
                if ssid.is_empty() {
                    self.modal = Modal::Prompt(reject(p, "an SSID is required"));
                    return;
                }
                let password = p.fields[1].value.clone();
                let (security, secret) = if password.is_empty() {
                    (Security::Open, Secret::None)
                } else {
                    (Security::WpaPsk, Secret::Passphrase(password))
                };
                let profile =
                    match build_wifi_profile(ssid.as_bytes(), security, &secret, true, &interface) {
                        Ok(profile) => profile,
                        Err(e) => {
                            self.modal = Modal::Prompt(reject(p, &format_error(&e)));
                            return;
                        }
                    };
                let name = ssid.clone();
                self.spawn(
                    format!("connecting to {name}…").as_str(),
                    format!("connected to {name}"),
                    async move {
                        let active = client.add_and_activate(profile, &device, None).await?;
                        client.wait_for_activation(&active, &device).await
                    },
                );
            }
        }
    }
}

// ----------------------------------------------------------------- helpers

fn clamp_move(current: usize, delta: isize, len: usize) -> usize {
    let next = current as isize + delta;
    next.clamp(0, len as isize - 1) as usize
}

fn on_off(v: bool) -> &'static str {
    if v { "on" } else { "off" }
}

pub fn display_name(net: &Network) -> String {
    if net.name.is_empty() {
        "<hidden>".into()
    } else {
        net.name.clone()
    }
}

fn secret_prompt(title: String, security: Security, kind: PromptKind) -> Prompt {
    let fields = match security {
        Security::Enterprise => vec![
            Field { label: "Identity", value: String::new(), secret: false },
            Field { label: "Password", value: String::new(), secret: true },
        ],
        _ => vec![Field { label: "Password", value: String::new(), secret: true }],
    };
    let hint = match security {
        Security::Enterprise => "PEAP / MSCHAPv2".into(),
        Security::Wep => "WEP key or passphrase".into(),
        other => format!("{other} passphrase"),
    };
    Prompt { title, hint, fields, focus: 0, reveal: false, kind }
}

fn reject(mut p: Prompt, why: &str) -> Prompt {
    p.hint = why.to_string();
    p
}

fn secret_from(p: &Prompt, security: Security) -> Option<Secret> {
    match security {
        Security::Enterprise => {
            let identity = p.fields[0].value.trim().to_string();
            let password = p.fields[1].value.clone();
            (!identity.is_empty() && !password.is_empty())
                .then_some(Secret::Enterprise { identity, password })
        }
        _ => {
            let value = p.fields[0].value.clone();
            (!value.is_empty()).then_some(Secret::Passphrase(value))
        }
    }
}

/// D-Bus errors arrive as an interface-qualified name plus a sentence; keep the
/// sentence and the part of the name that identifies the action, and drop the
/// `org.freedesktop.*` boilerplate that only costs the status bar room.
pub fn format_error(e: &anyhow::Error) -> String {
    let context = e.to_string();
    let root = e
        .chain()
        .last()
        .map(|c| c.to_string())
        .unwrap_or_default();

    let mut msg = if root.is_empty() || root == context {
        context
    } else {
        format!("{context}: {root}")
    };
    for noise in [
        "org.freedesktop.NetworkManager.",
        "org.freedesktop.DBus.Error.",
        "PermissionDenied: ",
    ] {
        msg = msg.replace(noise, "");
    }
    // polkit is the usual reason a change is refused, and the fix is not
    // something the message itself ever mentions.
    let lower = msg.to_lowercase();
    if lower.contains("not authorized") {
        msg.push_str("  — needs a local login session or root");
    } else if lower.contains("permission denied") || lower.contains("not permitted") {
        msg.push_str("  — needs root");
    }
    msg
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_keeps_the_action_and_drops_the_bus_boilerplate() {
        let root = anyhow::anyhow!(
            "org.freedesktop.NetworkManager.wifi.scan request failed: not authorized"
        );
        let e = root.context("requesting a scan");
        assert_eq!(
            format_error(&e),
            concat!(
                "requesting a scan: wifi.scan request failed: not authorized",
                "  — needs a local login session or root"
            )
        );
    }

    #[test]
    fn a_refused_file_write_points_at_root() {
        let e = anyhow::Error::from(std::io::Error::from_raw_os_error(13))
            .context("writing /etc/NetworkManager/conf.d/90-wifimanager-wlan0.conf");
        assert_eq!(
            format_error(&e),
            "writing /etc/NetworkManager/conf.d/90-wifimanager-wlan0.conf: \
             Permission denied (os error 13)  — needs root"
        );
    }

    #[test]
    fn error_without_a_cause_is_not_doubled_up() {
        let e = anyhow::anyhow!("scan timed out");
        assert_eq!(format_error(&e), "scan timed out");
    }

    #[test]
    fn movement_is_clamped_to_the_list() {
        assert_eq!(clamp_move(0, -1, 5), 0);
        assert_eq!(clamp_move(4, 1, 5), 4);
        assert_eq!(clamp_move(2, 10, 5), 4);
        assert_eq!(clamp_move(2, isize::MIN / 2, 5), 0);
    }
}
