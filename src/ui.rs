//! Rendering. Everything here reads from `App`; nothing here mutates it.

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{
    Block, BorderType, Borders, Clear, List, ListItem, ListState, Paragraph, Wrap,
};

use crate::app::{App, ConfirmKind, Field, Focus, Level, Modal, Prompt, display_name};
use crate::nm::types::{
    DeviceState, Network, Security, WifiDevice, describe_security, device_state_reason,
};

const SPINNER: [char; 10] = ['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];

const FG_DIM: Color = Color::DarkGray;
const FG_KEY: Color = Color::Gray;
const ACCENT: Color = Color::Cyan;
const BG_DEEP: Color = Color::Rgb(16, 18, 22);

pub fn draw(frame: &mut Frame, app: &App) {
    let [header, body, footer] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Fill(1),
        Constraint::Length(1),
    ])
    .areas(frame.area());

    let [lists, details] =
        Layout::vertical([Constraint::Fill(1), Constraint::Length(14)]).areas(body);

    let [devices_area, networks_area] =
        Layout::horizontal([Constraint::Length(38), Constraint::Fill(1)]).areas(lists);

    let [device_detail_area, network_detail_area] =
        Layout::horizontal([Constraint::Percentage(50), Constraint::Fill(1)]).areas(details);

    draw_header(frame, header, app);
    draw_devices(frame, devices_area, app);
    draw_device_detail(frame, device_detail_area, app);
    draw_networks(frame, networks_area, app);
    draw_network_detail(frame, network_detail_area, app);
    draw_footer(frame, footer, app);

    match &app.modal {
        Modal::None => {}
        Modal::Help => draw_help(frame),
        Modal::Prompt(p) => draw_prompt(frame, p),
        Modal::Confirm(c) => draw_confirm(frame, c),
    }
}

// ---------------------------------------------------------------------- chrome

/// A bordered pane. The focused one is meant to be obvious across the room:
/// a heavy accent border and a reversed title chip, against a thin dim border
/// and plain title for everything else.
fn panel(title: impl Into<String>, focused: bool) -> Block<'static> {
    let (border_type, border_style, title_style) = if focused {
        (
            BorderType::Thick,
            Style::default().fg(ACCENT),
            Style::default().fg(BG_DEEP).bg(ACCENT).add_modifier(Modifier::BOLD),
        )
    } else {
        (
            BorderType::Rounded,
            Style::default().fg(FG_DIM),
            Style::default().fg(FG_KEY),
        )
    };
    Block::default()
        .borders(Borders::ALL)
        .border_type(border_type)
        .border_style(border_style)
        .title(Span::styled(format!(" {} ", title.into()), title_style))
}

fn draw_header(frame: &mut Frame, area: Rect, app: &App) {
    let s = &app.snapshot;
    let radio = if !s.wireless_hw_enabled {
        Span::styled(" blocked ", Style::default().fg(Color::Black).bg(Color::Red))
    } else if s.wireless_enabled {
        Span::styled(" Wi-Fi on ", Style::default().fg(Color::Black).bg(Color::Green))
    } else {
        Span::styled(" Wi-Fi off ", Style::default().fg(Color::Black).bg(Color::Yellow))
    };

    let mut spans = vec![
        Span::styled(" wifimanager ", Style::default().fg(Color::Black).bg(ACCENT)),
        Span::raw(" "),
        radio,
        Span::raw("  "),
    ];
    if let Some(state) = s.state {
        spans.push(kv("state", &state.to_string()));
        spans.push(Span::raw("  "));
    }
    if let Some(c) = s.connectivity {
        spans.push(kv("internet", &c.to_string()));
        spans.push(Span::raw("  "));
    }
    if !s.networking_enabled {
        spans.push(Span::styled(
            "networking disabled  ",
            Style::default().fg(Color::Red),
        ));
    }
    if !s.version.is_empty() {
        spans.push(kv("NM", &s.version));
    }
    spans.push(Span::styled(
        "  ? help",
        Style::default().fg(FG_DIM),
    ));

    frame.render_widget(Paragraph::new(Line::from(spans)), area);
}

fn draw_footer(frame: &mut Frame, area: Rect, app: &App) {
    if app.status_visible() && !app.status.text.is_empty() {
        let color = match app.status.level {
            Level::Info => ACCENT,
            Level::Ok => Color::Green,
            Level::Error => Color::Red,
        };
        let mut spans = Vec::new();
        if app.status.busy {
            let i = (app.status_at.elapsed().as_millis() / 90) as usize % SPINNER.len();
            spans.push(Span::styled(
                format!(" {} ", SPINNER[i]),
                Style::default().fg(color),
            ));
        } else {
            spans.push(Span::raw(" "));
        }
        spans.push(Span::styled(app.status.text.clone(), Style::default().fg(color)));
        frame.render_widget(Paragraph::new(Line::from(spans)), area);
        return;
    }

    // Name the pane Tab moves to, so the arrow keys are never a guess.
    let tab_hint = match app.focus {
        Focus::Devices => "tab → networks",
        Focus::Networks => "tab → devices",
    };
    let keys: &[(&str, &str)] = &[
        ("↵", "join"),
        ("s", "scan"),
        ("d", "disconnect"),
        ("w", "radio"),
        ("f", "forget"),
        ("p", "re-key"),
        ("n", "hidden"),
        ("a", "autoconnect"),
        ("e", "enable/disable"),
        ("q", "quit"),
    ];
    let mut spans = vec![Span::raw(" ")];
    for (k, label) in keys {
        spans.push(Span::styled(
            (*k).to_string(),
            Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
        ));
        spans.push(Span::styled(
            format!(" {label}  "),
            Style::default().fg(FG_DIM),
        ));
    }
    spans.push(Span::styled(
        tab_hint.to_string(),
        Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
    ));
    frame.render_widget(Paragraph::new(Line::from(spans)), area);
}

// --------------------------------------------------------------------- devices

fn draw_devices(frame: &mut Frame, area: Rect, app: &App) {
    let focused = app.focus == Focus::Devices;
    let block = panel(format!("Devices ({})", app.devices().len()), focused);

    if app.devices().is_empty() {
        let msg = if app.first_load {
            "loading…"
        } else {
            "no Wi-Fi devices"
        };
        frame.render_widget(
            Paragraph::new(Span::styled(msg, Style::default().fg(FG_DIM))).block(block),
            area,
        );
        return;
    }

    let items: Vec<ListItem> = app.devices().iter().map(device_row).collect();

    let mut state = ListState::default().with_selected(Some(app.device_index()));
    frame.render_stateful_widget(
        List::new(items)
            .block(block)
            .highlight_style(selection_style(focused))
            .highlight_symbol(selection_marker(focused)),
        area,
        &mut state,
    );
}

fn device_row(d: &WifiDevice) -> ListItem<'static> {
    let color = device_color(d.state);
    let head = Line::from(vec![
        Span::styled(
            if d.state.is_connected() { "●" } else { "○" },
            Style::default().fg(color),
        ),
        Span::raw(" "),
        Span::styled(
            truncate(&d.label(), 30),
            Style::default().add_modifier(Modifier::BOLD),
        ),
    ]);

    // Second line: the interface and state, then whatever is most useful to
    // know at a glance about this device without selecting it.
    let detail = if let Some(net) = d.active_network() {
        format!("{} · {}%", display_name(net), net.strength())
    } else if let Some(a) = &d.active {
        a.id.clone()
    } else if !d.enabled() {
        "disabled".into()
    } else if d.state_reason != 0 {
        device_state_reason(d.state_reason)
    } else {
        String::new()
    };

    ListItem::new(vec![
        head,
        Line::from(vec![
            Span::styled(format!("  {} ", d.interface), Style::default().fg(FG_DIM)),
            Span::styled(d.state.to_string(), Style::default().fg(color)),
            Span::styled(
                if detail.is_empty() { String::new() } else { format!(" · {}", truncate(&detail, 22)) },
                Style::default().fg(FG_DIM),
            ),
        ]),
    ])
}

fn draw_device_detail(frame: &mut Frame, area: Rect, app: &App) {
    let block = panel("Device", false);
    let Some(d) = app.device() else {
        frame.render_widget(Paragraph::new("").block(block), area);
        return;
    };

    let mut lines: Vec<Line> = Vec::new();
    lines.push(row("interface", &d.interface));
    if !d.model.is_empty() {
        lines.push(row("model", &d.model));
    }
    lines.push(Line::from(vec![
        Span::styled(format!("{:<11}", "state"), Style::default().fg(FG_KEY)),
        Span::styled(d.state.to_string(), Style::default().fg(device_color(d.state))),
        Span::styled(
            if d.state_reason == 0 {
                String::new()
            } else {
                format!("  ({})", device_state_reason(d.state_reason))
            },
            Style::default().fg(FG_DIM),
        ),
    ]));
    if let Some(a) = &d.active {
        lines.push(row(
            "profile",
            &format!(
                "{} · {}{}",
                a.id,
                a.state,
                if a.default_route { " · default route" } else { "" }
            ),
        ));
    }
    if let Some(net) = d.active_network() {
        let ap = net.best();
        let rate = if d.bitrate_kbps > 0 {
            format!(" · {} Mb/s", d.bitrate_kbps / 1000)
        } else {
            String::new()
        };
        lines.push(row(
            "link",
            &format!(
                "{}% · ch {} · {}{}",
                ap.strength,
                ap.channel(),
                ap.band(),
                rate
            ),
        ));
    }
    lines.push(row("mac", &d.hw_address));
    lines.push(row("driver", &format!("{} {}", d.driver, d.driver_version)));

    let mut flags = vec![short_mode(d.mode).to_string()];
    flags.push(enabled_flag(d).into());
    flags.push(if d.autoconnect {
        "autoconnect".into()
    } else {
        "no autoconnect".into()
    });
    flags.push(format!("mtu {}", d.mtu));
    lines.push(row("flags", &flags.join(" · ")));

    // A released device keeps whatever addresses it had until the link is
    // brought back; they are history, not a link, so they are not shown.
    if !d.enabled() {
        frame.render_widget(
            Paragraph::new(Text::from(lines))
                .block(block)
                .wrap(Wrap { trim: false }),
            area,
        );
        return;
    }

    if !d.ip4.addresses.is_empty() {
        lines.push(row("ipv4", &d.ip4.addresses.join(", ")));
    }
    let mut routing = Vec::new();
    if let Some(g) = &d.ip4.gateway {
        routing.push(format!("gw {g}"));
    }
    if !d.ip4.nameservers.is_empty() {
        routing.push(format!("dns {}", d.ip4.nameservers.join(", ")));
    }
    if !d.ip4.domains.is_empty() {
        routing.push(d.ip4.domains.join(", "));
    }
    if !routing.is_empty() {
        lines.push(row("routing", &routing.join(" · ")));
    }
    // Prefer a routable v6 address over the link-local one everybody has.
    if let Some(addr) = d
        .ip6
        .addresses
        .iter()
        .find(|a| !a.starts_with("fe80"))
        .or_else(|| d.ip6.addresses.first())
    {
        lines.push(row("ipv6", addr));
    }
    if d.last_scan_ms > 0 {
        lines.push(row("scanned", &format!("{} ago", scan_age(d))));
    }

    frame.render_widget(
        Paragraph::new(Text::from(lines))
            .block(block)
            .wrap(Wrap { trim: false }),
        area,
    );
}

/// The runtime state and whether it survives a reboot, when the two disagree:
/// a device someone unmanaged with nmcli comes back at the next boot, and one
/// with a drop-in but no runtime change goes away at it.
fn enabled_flag(d: &WifiDevice) -> &'static str {
    match (d.enabled(), d.disabled_by_config) {
        (true, false) => "enabled",
        (true, true) => "enabled until reboot",
        (false, true) => "disabled",
        (false, false) => "disabled until reboot",
    }
}

// -------------------------------------------------------------------- networks

fn draw_networks(frame: &mut Frame, area: Rect, app: &App) {
    let focused = app.focus == Focus::Networks;
    let title = match app.device() {
        Some(d) => format!("Networks on {} ({})", truncate(&d.label(), 40), d.networks.len()),
        None => "Networks".to_string(),
    };
    let block = panel(title, focused);

    if app.networks().is_empty() {
        let msg = if app.first_load {
            "loading…"
        } else if !app.snapshot.wireless_enabled {
            "Wi-Fi is off — press w to turn it on"
        } else if app.device().is_some_and(|d| !d.enabled()) {
            "this device is disabled — press e to enable it"
        } else {
            "no networks in range — press s to scan"
        };
        frame.render_widget(
            Paragraph::new(Span::styled(msg, Style::default().fg(FG_DIM))).block(block),
            area,
        );
        return;
    }

    let items: Vec<ListItem> = app.networks().iter().map(network_row).collect();
    let mut state = ListState::default().with_selected(Some(app.network_index()));
    frame.render_stateful_widget(
        List::new(items)
            .block(block)
            .highlight_style(selection_style(focused))
            .highlight_symbol(selection_marker(focused)),
        area,
        &mut state,
    );
}

fn network_row(net: &Network) -> ListItem<'_> {
    let mut spans = vec![
        Span::styled(
            if net.active { "● " } else { "  " },
            Style::default().fg(Color::Green),
        ),
        Span::styled(
            format!("{:<28}", truncate(&display_name(net), 28)),
            if net.active {
                Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)
            } else {
                Style::default().add_modifier(Modifier::BOLD)
            },
        ),
    ];
    spans.extend(signal_bars(net.strength()));
    spans.push(Span::styled(
        format!(" {:>3}%  ", net.strength()),
        Style::default().fg(strength_color(net.strength())),
    ));
    spans.push(Span::styled(
        format!("{:<7}", net.security().to_string()),
        Style::default().fg(security_color(net.security())),
    ));
    spans.push(Span::styled(
        format!("{:<8}", net.best().band()),
        Style::default().fg(FG_DIM),
    ));
    spans.push(Span::styled(
        if net.saved.is_empty() { "     " } else { "saved" },
        Style::default().fg(Color::Blue),
    ));
    if net.aps.len() > 1 {
        spans.push(Span::styled(
            format!("  {}×", net.aps.len()),
            Style::default().fg(FG_DIM),
        ));
    }
    ListItem::new(Line::from(spans))
}

fn draw_network_detail(frame: &mut Frame, area: Rect, app: &App) {
    let block = panel("Network", false);
    let Some(net) = app.network() else {
        frame.render_widget(Paragraph::new("").block(block), area);
        return;
    };
    let ap = net.best();

    let mut lines = vec![
        Line::from(vec![
            Span::styled(
                display_name(net),
                Style::default().add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                if net.active { "  connected" } else { "" },
                Style::default().fg(Color::Green),
            ),
        ]),
        row("bssid", &format!("{}  ({})", ap.bssid, ap.mode)),
        row(
            "signal",
            &format!("{}%  ·  max {} Mb/s", ap.strength, ap.max_bitrate / 1000),
        ),
        row(
            "channel",
            &format!("{} · {} MHz · {}", ap.channel(), ap.frequency, ap.band()),
        ),
        row(
            "security",
            &describe_security(ap.flags, ap.wpa_flags, ap.rsn_flags),
        ),
        row("last seen", &seen_age(ap.last_seen)),
    ];

    if net.aps.len() > 1 {
        let others: Vec<String> = net
            .aps
            .iter()
            .skip(1)
            .take(2)
            .map(|a| format!("{} {}% ch{}", a.bssid, a.strength, a.channel()))
            .collect();
        let more = net.aps.len() - 1 - others.len();
        lines.push(row(
            "also on",
            &format!(
                "{}{}",
                others.join("  "),
                if more > 0 { format!("  +{more}") } else { String::new() }
            ),
        ));
    }

    if net.saved.is_empty() {
        lines.push(row("profile", "none saved"));
    } else {
        for s in net.saved.iter().take(2) {
            lines.push(row(
                "profile",
                &format!(
                    "{} · autoconnect {}{}",
                    s.id,
                    yes_no(s.autoconnect),
                    if s.hidden { " · hidden" } else { "" }
                ),
            ));
        }
    }

    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        action_hint(net),
        Style::default().fg(FG_DIM),
    )));

    frame.render_widget(
        Paragraph::new(Text::from(lines))
            .block(block)
            .wrap(Wrap { trim: false }),
        area,
    );
}

fn action_hint(net: &Network) -> String {
    if net.is_hidden() {
        "these access points broadcast no name — press n to join one by SSID".into()
    } else if net.active {
        "connected — d disconnects the device, p re-enters the password".into()
    } else if !net.saved.is_empty() {
        "↵ connects with the saved profile · p replaces the password · f forgets it".into()
    } else if net.security().needs_secret() {
        "↵ asks for the password, then joins".into()
    } else {
        "↵ joins this open network".into()
    }
}

// ----------------------------------------------------------------------- modals

fn draw_prompt(frame: &mut Frame, p: &Prompt) {
    let height = 5 + p.fields.len() as u16 * 2;
    let area = centered(frame.area(), 62, height);
    frame.render_widget(Clear, area);

    let mut lines = vec![Line::from(Span::styled(
        p.hint.clone(),
        Style::default().fg(FG_DIM),
    ))];
    for (i, f) in p.fields.iter().enumerate() {
        lines.push(Line::from(""));
        lines.push(field_line(f, i == p.focus, p.reveal));
    }
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "↵ connect   tab field   ^r reveal   ^u clear   esc cancel",
        Style::default().fg(FG_DIM),
    )));

    frame.render_widget(
        Paragraph::new(Text::from(lines)).block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(ACCENT))
                .title(Span::styled(
                    format!(" {} ", p.title),
                    Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
                ))
                .padding(ratatui::widgets::Padding::horizontal(1)),
        ),
        area,
    );
}

fn field_line(f: &Field, focused: bool, reveal: bool) -> Line<'static> {
    let shown = if f.secret && !reveal {
        "•".repeat(f.value.chars().count())
    } else {
        f.value.clone()
    };
    Line::from(vec![
        Span::styled(
            format!("{:<10}", f.label),
            Style::default().fg(if focused { ACCENT } else { FG_KEY }),
        ),
        Span::raw(shown),
        Span::styled(
            if focused { "▏" } else { "" },
            Style::default().fg(ACCENT).add_modifier(Modifier::RAPID_BLINK),
        ),
    ])
}

fn draw_confirm(frame: &mut Frame, c: &ConfirmKind) {
    let (title, body) = match c {
        ConfirmKind::Forget { name, connections } => (
            "Forget network",
            format!(
                "Delete {} saved profile{} for “{}”?\nThis device will stop connecting to it automatically.",
                connections.len(),
                if connections.len() == 1 { "" } else { "s" },
                name
            ),
        ),
        ConfirmKind::Disable { interface, carrying, .. } => (
            "Disable device",
            format!(
                "Disable {interface}?{}\nNetworkManager lets go of it and the link goes down. It stays disabled across reboots until e enables it again.",
                match carrying {
                    Some(id) => format!(" It is carrying “{id}”, which another radio may pick up."),
                    None => String::new(),
                }
            ),
        ),
    };
    // Border and padding take four columns. The height follows the wrapped
    // body so a longer warning is never cut off at the bottom; word wrapping
    // packs a row less tightly than a character count, hence the margin.
    let (width, packed) = (64u16, 52u16);
    let body_rows: u16 = body
        .lines()
        .map(|l| (l.chars().count() as u16).div_ceil(packed).max(1))
        .sum();
    let area = centered(frame.area(), width, body_rows + 5);
    frame.render_widget(Clear, area);
    let mut lines = vec![Line::from("")];
    lines.extend(body.lines().map(|l| Line::from(l.to_string())));
    lines.extend([
        Line::from(""),
        Line::from(Span::styled(
            "y confirm    esc cancel",
            Style::default().fg(FG_DIM),
        )),
    ]);
    frame.render_widget(
        Paragraph::new(Text::from(lines))
            .wrap(Wrap { trim: false })
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_type(BorderType::Rounded)
                    .border_style(Style::default().fg(Color::Yellow))
                    .title(Span::styled(
                        format!(" {title} "),
                        Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
                    ))
                    .padding(ratatui::widgets::Padding::horizontal(1)),
            ),
        area,
    );
}

const HELP: &[(&str, &str)] = &[
    ("↑ ↓ / j k", "move within the focused pane"),
    ("tab / ← →", "switch between devices and networks"),
    ("g / G", "jump to the first or last row"),
    ("", ""),
    ("enter", "join the selected network"),
    ("p", "re-enter the password, then reconnect"),
    ("n", "join a network by name (hidden SSID)"),
    ("f", "delete the saved profile for this network"),
    ("d", "disconnect the selected device"),
    ("", ""),
    ("s / r", "rescan on the selected device"),
    ("w", "turn the Wi-Fi radio on or off"),
    ("a", "toggle autoconnect on the selected device"),
    ("e", "enable or disable the selected device (persists)"),
    ("", ""),
    ("esc", "dismiss a message or close a dialog"),
    ("q / ctrl-c", "quit"),
];

fn draw_help(frame: &mut Frame) {
    let area = centered(frame.area(), 66, HELP.len() as u16 + 4);
    frame.render_widget(Clear, area);
    let mut lines = vec![Line::from("")];
    for (k, v) in HELP {
        if k.is_empty() {
            lines.push(Line::from(""));
            continue;
        }
        lines.push(Line::from(vec![
            Span::styled(
                format!("{k:<12}"),
                Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
            ),
            Span::raw(*v),
        ]));
    }
    frame.render_widget(
        Paragraph::new(Text::from(lines)).block(
            Block::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(ACCENT))
                .title(Span::styled(
                    " Keys — any key closes ",
                    Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
                ))
                .padding(ratatui::widgets::Padding::horizontal(1)),
        ),
        area,
    );
}

// ---------------------------------------------------------------------- pieces

fn centered(area: Rect, width: u16, height: u16) -> Rect {
    let w = width.min(area.width.saturating_sub(2));
    let h = height.min(area.height.saturating_sub(2));
    Rect {
        x: area.x + (area.width.saturating_sub(w)) / 2,
        y: area.y + (area.height.saturating_sub(h)) / 2,
        width: w,
        height: h,
    }
}

fn row(key: &str, value: &str) -> Line<'static> {
    Line::from(vec![
        Span::styled(format!("{key:<11}"), Style::default().fg(FG_KEY)),
        Span::raw(value.to_string()),
    ])
}

fn kv(key: &str, value: &str) -> Span<'static> {
    Span::styled(
        format!("{key} {value}"),
        Style::default().fg(FG_DIM),
    )
}

fn signal_bars(strength: u8) -> Vec<Span<'static>> {
    let filled = match strength {
        0..=24 => 1,
        25..=49 => 2,
        50..=74 => 3,
        _ => 4,
    };
    let glyphs = ['▂', '▄', '▆', '█'];
    let color = strength_color(strength);
    glyphs
        .iter()
        .enumerate()
        .map(|(i, g)| {
            Span::styled(
                g.to_string(),
                if i < filled {
                    Style::default().fg(color)
                } else {
                    Style::default().fg(FG_DIM)
                },
            )
        })
        .collect()
}

fn strength_color(strength: u8) -> Color {
    match strength {
        0..=24 => Color::Red,
        25..=49 => Color::Yellow,
        50..=74 => Color::LightGreen,
        _ => Color::Green,
    }
}

fn security_color(sec: Security) -> Color {
    match sec {
        Security::Open => Color::Red,
        Security::Wep => Color::Yellow,
        _ => Color::Blue,
    }
}

fn device_color(state: DeviceState) -> Color {
    match state {
        DeviceState::Activated => Color::Green,
        DeviceState::Failed => Color::Red,
        DeviceState::Unmanaged | DeviceState::Unavailable => Color::DarkGray,
        s if s.is_busy() => Color::Yellow,
        _ => Color::Gray,
    }
}

/// The cursor row. In the unfocused pane it stays visible — it is still where
/// the cursor will be on the next Tab — but faint enough not to compete.
fn selection_style(focused: bool) -> Style {
    if focused {
        Style::default().bg(Color::Rgb(38, 48, 60)).add_modifier(Modifier::BOLD)
    } else {
        Style::default().bg(Color::Rgb(24, 26, 30))
    }
}

/// Left gutter marker on the cursor row, drawn only in the focused pane. The
/// blank keeps both panes on the same column grid so nothing shifts on Tab.
fn selection_marker(focused: bool) -> &'static str {
    if focused { "\u{258c}" } else { " " }
}

fn yes_no(v: bool) -> &'static str {
    if v { "yes" } else { "no" }
}

fn short_mode(mode: crate::nm::types::ApMode) -> &'static str {
    match mode {
        crate::nm::types::ApMode::Infra => "infra",
        crate::nm::types::ApMode::AdHoc => "ad-hoc",
        crate::nm::types::ApMode::Ap => "hotspot",
        crate::nm::types::ApMode::Mesh => "mesh",
        crate::nm::types::ApMode::Unknown => "mode ?",
    }
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let mut out: String = s.chars().take(max.saturating_sub(1)).collect();
        out.push('…');
        out
    }
}

/// `AccessPoint.LastSeen` is CLOCK_BOOTTIME seconds, or -1 if never seen.
fn seen_age(last_seen: i32) -> String {
    if last_seen < 0 {
        return "never".into();
    }
    let Some(now_ms) = boottime_ms() else {
        return "just now".into();
    };
    let secs = (now_ms / 1000 - last_seen as i64).max(0);
    match secs {
        0..=3 => "moments ago".into(),
        4..=90 => format!("{secs}s ago"),
        _ => format!("{}m ago", secs / 60),
    }
}

/// `LastScan` is CLOCK_BOOTTIME milliseconds, so compare it against uptime.
fn scan_age(d: &WifiDevice) -> String {
    let Some(now_ms) = boottime_ms() else {
        return "just now".into();
    };
    let secs = ((now_ms - d.last_scan_ms).max(0) / 1000) as u64;
    match secs {
        0..=5 => "moments".into(),
        6..=90 => format!("{secs}s"),
        _ => format!("{}m", secs / 60),
    }
}

fn boottime_ms() -> Option<i64> {
    let raw = std::fs::read_to_string("/proc/uptime").ok()?;
    let secs: f64 = raw.split_whitespace().next()?.parse().ok()?;
    Some((secs * 1000.0) as i64)
}
