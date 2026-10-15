//! The running service: keeps links to paired machines and hands the mouse (and
//! keyboard) over when the pointer is pushed against the edge facing one of them.

use crate::clipboard::Clipboard;
use crate::config::{self, Config, PORT, Side};
use crate::devices::Devices;
use crate::discovery;
use crate::link::{self, Link, LinkReader};
use crate::pointer::Pointer;
use crate::proto::{DeviceChannel, Msg, VERSION};
use anyhow::{Context, Result};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant, SystemTime};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::mpsc;

const POLL: Duration = Duration::from_millis(8);
/// The pointer must stay on the edge this long, so brushing past it does nothing.
const DWELL: Duration = Duration::from_millis(40);
/// After the mouse arrives, ignore edges briefly so it does not bounce straight back.
const SETTLE: Duration = Duration::from_millis(600);
const KEEPALIVE: Duration = Duration::from_secs(10);

struct Conn {
    name: String,
    tx: mpsc::UnboundedSender<Msg>,
    devices: Vec<DeviceChannel>,
}

struct Shared {
    cfg: Config,
    cfg_mtime: Option<SystemTime>,
    conns: HashMap<String, Conn>,
    /// True while the mouse is connected to this machine.
    holding: bool,
    settle_until: Instant,
    devices: Option<Devices>,
}

type State = Arc<Mutex<Shared>>;

pub async fn run() -> Result<()> {
    let cfg = Config::load_or_create()?;
    println!("flow: {} ({}) listening on port {PORT}", cfg.name, cfg.my_id());
    let (_disc, mut found) = discovery::start(&cfg)?;
    let state: State = Arc::new(Mutex::new(Shared {
        cfg_mtime: mtime(),
        cfg,
        conns: HashMap::new(),
        holding: false,
        settle_until: Instant::now(),
        devices: None,
    }));
    let clipboard = Clipboard::start();

    rescan_devices(state.clone(), Duration::ZERO);

    let listener = TcpListener::bind(("0.0.0.0", PORT)).await.with_context(|| format!("port {PORT} is busy"))?;
    {
        let (state, clipboard) = (state.clone(), clipboard.clone());
        tokio::spawn(async move {
            while let Ok((stream, addr)) = listener.accept().await {
                let (state, clipboard) = (state.clone(), clipboard.clone());
                tokio::spawn(async move {
                    let private = state.lock().unwrap().cfg.private().unwrap_or_default();
                    let trusted = |k: &[u8]| state.lock().unwrap().cfg.peers.contains_key(&config::hex(k));
                    match link::accept(stream, &private, trusted).await {
                        Ok(l) => serve(state, clipboard, l, Some(addr)).await,
                        Err(e) => eprintln!("flow: {addr}: {e:#}"),
                    }
                });
            }
        });
    }

    // Dial paired peers we hear about. Only the side with the smaller key dials, so two
    // machines never open two links to each other.
    {
        let (state, clipboard) = (state.clone(), clipboard.clone());
        tokio::spawn(async move {
            while let Some(f) = found.recv().await {
                let should = {
                    let mut s = state.lock().unwrap();
                    let me = s.cfg.public_key.clone();
                    match s.cfg.peers.get_mut(&f.key) {
                        Some(p) => {
                            p.name = f.name.clone();
                            p.addr = f.addrs.first().map(|a| a.to_string());
                            me < f.key && !s.conns.contains_key(&f.key)
                        }
                        None => false,
                    }
                };
                if should {
                    tokio::spawn(dial(state.clone(), clipboard.clone(), f.key.clone(), f.addrs.clone()));
                }
            }
        });
    }
    // mDNS is often filtered (corporate Wi-Fi, VPNs): retry the last known address too.
    {
        let (state, clipboard) = (state.clone(), clipboard.clone());
        tokio::spawn(async move {
            loop {
                let todo: Vec<(String, SocketAddr)> = {
                    let s = state.lock().unwrap();
                    let me = &s.cfg.public_key;
                    s.cfg
                        .peers
                        .iter()
                        .filter(|(k, _)| me < *k && !s.conns.contains_key(*k))
                        .filter_map(|(k, p)| Some((k.clone(), p.addr.as_ref()?.parse().ok()?)))
                        .collect()
                };
                for (key, addr) in todo {
                    tokio::spawn(dial(state.clone(), clipboard.clone(), key, vec![addr]));
                }
                tokio::time::sleep(Duration::from_secs(15)).await;
            }
        });
    }

    let handle = tokio::runtime::Handle::current();
    let edge_state = state.clone();
    let edge_clip = clipboard.clone();
    thread::spawn(move || {
        if let Err(e) = edge_loop(edge_state, edge_clip, handle) {
            eprintln!("flow: edge watcher stopped: {e:#}");
        }
    });

    tokio::signal::ctrl_c().await?;
    Ok(())
}

fn mtime() -> Option<SystemTime> {
    std::fs::metadata(config::path().ok()?).ok()?.modified().ok()
}

async fn dial(state: State, clipboard: Clipboard, key: String, addrs: Vec<SocketAddr>) {
    let (private, remote) = {
        let s = state.lock().unwrap();
        (s.cfg.private().unwrap_or_default(), config::unhex(&key).unwrap_or_default())
    };
    for addr in addrs {
        let Ok(Ok(stream)) = tokio::time::timeout(Duration::from_secs(3), TcpStream::connect(addr)).await else { continue };
        match link::connect(stream, &private, &remote).await {
            Ok(l) => return serve(state, clipboard, l, Some(addr)).await,
            Err(e) => eprintln!("flow: {addr}: {e:#}"),
        }
    }
}

async fn serve(state: State, clipboard: Clipboard, link: Link, addr: Option<SocketAddr>) {
    let key = config::hex(&link.remote_key);
    let (tx, mut rx) = mpsc::unbounded_channel::<Msg>();
    let hello = {
        let mut s = state.lock().unwrap();
        if s.conns.contains_key(&key) {
            return; // already linked
        }
        let name = s.cfg.peers.get(&key).map(|p| p.name.clone()).unwrap_or_default();
        if let (Some(p), Some(a)) = (s.cfg.peers.get_mut(&key), addr) {
            p.addr = Some(SocketAddr::new(a.ip(), PORT).to_string());
        }
        let _ = s.cfg.save();
        s.conns.insert(key.clone(), Conn { name: name.clone(), tx: tx.clone(), devices: Vec::new() });
        println!("flow: linked with {name}");
        Msg::Hello {
            name: s.cfg.name.clone(),
            version: VERSION,
            devices: s.devices.as_ref().map(|d| d.channels()).unwrap_or_default(),
        }
    };
    let _ = tx.send(hello);

    let Link { reader, mut writer, .. } = link;
    let writer_task = tokio::spawn(async move {
        let mut tick = tokio::time::interval(KEEPALIVE);
        loop {
            let msg = tokio::select! {
                m = rx.recv() => match m { Some(m) => m, None => break },
                _ = tick.tick() => Msg::Ping,
            };
            if writer.send(&msg.encode()).await.is_err() {
                break;
            }
        }
    });
    if let Err(e) = read_loop(&state, &clipboard, &key, reader, &tx).await {
        eprintln!("flow: link closed: {e:#}");
    }
    writer_task.abort();
    let mut s = state.lock().unwrap();
    if let Some(c) = s.conns.remove(&key) {
        println!("flow: lost {}", c.name);
    }
}

async fn read_loop(state: &State, clipboard: &Clipboard, key: &str, mut reader: LinkReader, tx: &mpsc::UnboundedSender<Msg>) -> Result<()> {
    loop {
        let msg = tokio::time::timeout(KEEPALIVE * 3, reader.recv()).await.context("peer went silent")??;
        match Msg::decode(&msg)? {
            Msg::Hello { name, devices, .. } => {
                let mut s = state.lock().unwrap();
                if let Some(c) = s.conns.get_mut(key) {
                    c.name = name.clone();
                    c.devices = devices;
                }
                if let Some(p) = s.cfg.peers.get_mut(key) {
                    p.name = name;
                }
                let _ = s.cfg.save();
            }
            Msg::Enter { edge, pos } => {
                {
                    let mut s = state.lock().unwrap();
                    s.holding = true;
                    s.settle_until = Instant::now() + SETTLE;
                }
                if let Ok(p) = Pointer::open() {
                    let _ = p.enter(edge, pos);
                }
                // The devices reconnect to us within a second or two; pick them up then.
                rescan_devices(state.clone(), Duration::from_millis(1500));
            }
            Msg::Clipboard(clip) => {
                if state.lock().unwrap().cfg.clipboard {
                    clipboard.put(clip);
                }
            }
            Msg::Ping => {
                let _ = tx.send(Msg::Pong);
            }
            Msg::Pong => {}
        }
    }
}

fn rescan_devices(state: State, after: Duration) {
    thread::spawn(move || {
        thread::sleep(after);
        match Devices::scan() {
            Ok(d) => {
                let mut s = state.lock().unwrap();
                let mut changed = false;
                for c in d.channels() {
                    if s.cfg.channels.insert(c.unit.clone(), c.channel) != Some(c.channel) {
                        changed = true;
                    }
                }
                if changed {
                    let _ = s.cfg.save();
                }
                s.holding = d.list.iter().any(|t| !t.keyboard);
                let hello_devices = d.channels();
                s.devices = Some(d);
                // Tell peers which channels we sit on, so they can send devices here.
                let name = s.cfg.name.clone();
                for c in s.conns.values() {
                    let _ = c.tx.send(Msg::Hello { name: name.clone(), version: VERSION, devices: hello_devices.clone() });
                }
            }
            Err(e) => eprintln!("flow: device scan failed: {e:#}"),
        }
    });
}

fn edge_loop(state: State, clipboard: Clipboard, rt: tokio::runtime::Handle) -> Result<()> {
    let pointer = Pointer::open()?;
    let mut pressed: Option<(Side, Instant)> = None;
    let mut last_reload = Instant::now();
    loop {
        thread::sleep(POLL);
        if last_reload.elapsed() > Duration::from_secs(2) {
            last_reload = Instant::now();
            reload_config(&state);
        }
        let (holding, settled) = {
            let s = state.lock().unwrap();
            (s.holding, Instant::now() >= s.settle_until)
        };
        if !holding || !settled {
            pressed = None;
            continue;
        }
        let Some((side, pos)) = pointer.at_edge()? else {
            pressed = None;
            continue;
        };
        let since = match pressed {
            Some((s, t)) if s == side => t,
            _ => {
                pressed = Some((side, Instant::now()));
                continue;
            }
        };
        if since.elapsed() < DWELL {
            continue;
        }
        pressed = None;
        if let Err(e) = hand_over(&state, &clipboard, &rt, side, pos) {
            eprintln!("flow: {e:#}");
        }
    }
}

fn reload_config(state: &State) {
    let now = mtime();
    let mut s = state.lock().unwrap();
    if now == s.cfg_mtime {
        return;
    }
    s.cfg_mtime = now;
    if let Ok(c) = Config::load_or_create() {
        s.cfg = c;
    }
}

fn hand_over(state: &State, clipboard: &Clipboard, rt: &tokio::runtime::Handle, side: Side, pos: f64) -> Result<()> {
    let mut guard = state.lock().unwrap();
    let s = &mut *guard;
    let Some((key, _)) = s.cfg.peers.iter().find(|(k, p)| p.side == Some(side) && s.conns.contains_key(*k)) else {
        return Ok(()); // nothing (connected) on that side
    };
    let key = key.clone();
    let conn = &s.conns[&key];
    let (tx, peer_name, announced) = (conn.tx.clone(), conn.name.clone(), conn.devices.clone());
    let keyboard_follows = s.cfg.keyboard_follows;
    let send_clipboard = s.cfg.clipboard;

    // Work out every switch before touching anything, so a missing channel aborts cleanly.
    let devices = s.devices.as_mut().context("no devices scanned yet")?;
    let mut plan = Vec::new();
    for t in &devices.list {
        if t.keyboard && !keyboard_follows {
            continue;
        }
        match devices.peer_channel(&t.unit, &peer_name, &announced) {
            Some(ch) if ch != t.channel => plan.push((t.unit.clone(), ch)),
            Some(_) => {}
            None if !t.keyboard => anyhow::bail!("{peer_name} is not paired with this mouse (no Easy-Switch channel found)"),
            None => eprintln!("flow: keyboard has no channel for {peer_name}; it stays here"),
        }
    }
    if plan.is_empty() {
        return Ok(());
    }

    if send_clipboard {
        if let Some(clip) = rt.block_on(clipboard.take_if_new()) {
            let _ = tx.send(Msg::Clipboard(clip));
        }
    }
    let _ = tx.send(Msg::Enter { edge: side.opposite(), pos });
    // Give the peer a head start to place its pointer before the mouse shows up there.
    thread::sleep(Duration::from_millis(30));
    for (unit, ch) in plan {
        devices.switch(&unit, ch)?;
    }
    s.holding = false;
    println!("flow: → {peer_name}");
    Ok(())
}
