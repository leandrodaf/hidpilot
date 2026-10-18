//! hidpilot-flow: move the mouse and keyboard between computers by pushing the pointer
//! past the screen edge, like Logitech Flow, with end-to-end encryption and no cloud.

mod agent;
mod audio;
mod clipboard;
mod config;
mod devices;
mod discovery;
mod link;
mod pairing;
mod pointer;
mod proto;
mod ui;

use anyhow::{Result, bail};
use clap::{Parser, Subcommand};
use config::{Config, PORT, Side};
use std::net::SocketAddr;
use std::time::Duration;


#[derive(Parser)]
#[command(version, about = "Move mouse and keyboard between computers (Flow)")]
struct Cli {
    #[command(subcommand)]
    cmd: Option<Cmd>,
}

#[derive(Subcommand)]
enum Cmd {
    /// Run the Flow service (default)
    Run,
    /// Open the Flow page: computers nearby, pairing, screen positions, options
    Ui,
    /// Pair with another computer (easier from `ui`). The other one shows a code to type here.
    Pair {
        /// The other computer: its name as seen on the network, or ip[:port]
        #[arg(long)]
        to: Option<String>,
        /// The code shown on the other computer
        #[arg(long)]
        code: Option<String>,
    },
    /// Say where a paired computer's screen is: left, right, top or bottom
    Place { peer: String, side: Side },
    /// List paired computers
    Peers,
    /// Show the mice and keyboards connected here and their Easy-Switch channels (diagnosis)
    Devices,
    /// Remove a paired computer
    Forget { peer: String },
    /// Show or change options
    Set {
        #[arg(long)]
        clipboard: Option<bool>,
        #[arg(long)]
        keyboard_follows: Option<bool>,
        /// Move the headset's sound to the computer the mouse goes to
        #[arg(long)]
        audio_follows: Option<bool>,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    hidpilot::hidpp::use_software_id(hidpilot::hidpp::software_id::FLOW);
    match Cli::parse().cmd.unwrap_or(Cmd::Run) {
        Cmd::Run => agent::run().await,
        Cmd::Ui | Cmd::Pair { to: None, .. } => ui::open(),
        Cmd::Pair { to: Some(to), code } => pair_with(&to, code).await,
        Cmd::Devices => {
            let d = devices::Devices::scan()?;
            if d.list.is_empty() {
                println!("no Easy-Switch device is connected to this computer right now");
                println!("(the mouse must be here, over Bluetooth or a receiver, for Flow to hand it over)");
            }
            for l in d.describe() {
                println!("{l}");
            }
            Ok(())
        }
        Cmd::Place { peer, side } => {
            let mut cfg = Config::load_or_create()?;
            let key = find_peer(&cfg, &peer)?;
            cfg.peers.get_mut(&key).unwrap().side = Some(side);
            // One screen per side.
            for (k, p) in cfg.peers.iter_mut() {
                if *k != key && p.side == Some(side) {
                    p.side = None;
                }
            }
            cfg.save()?;
            println!("{peer} is now on the {side:?} side", side = side);
            Ok(())
        }
        Cmd::Peers => {
            let cfg = Config::load_or_create()?;
            println!("this computer: {} ({})", cfg.name, cfg.my_id());
            if cfg.peers.is_empty() {
                println!("no paired computers; run `hidpilot-flow pair` on one and `--to` on the other");
            }
            for (k, p) in &cfg.peers {
                let side = p.side.map(|s| format!("{s:?}").to_lowercase()).unwrap_or_else(|| "not placed".into());
                println!("  {} ({}) - {side}", p.name, Config::id_of(k));
            }
            Ok(())
        }
        Cmd::Forget { peer } => {
            let mut cfg = Config::load_or_create()?;
            let key = find_peer(&cfg, &peer)?;
            cfg.peers.remove(&key);
            cfg.save()?;
            println!("forgot {peer}");
            Ok(())
        }
        Cmd::Set { clipboard, keyboard_follows, audio_follows } => {
            let mut cfg = Config::load_or_create()?;
            if let Some(v) = clipboard {
                cfg.clipboard = v;
            }
            if let Some(v) = keyboard_follows {
                cfg.keyboard_follows = v;
            }
            if let Some(v) = audio_follows {
                cfg.audio_follows = v;
            }
            cfg.save()?;
            println!("clipboard: {}\nkeyboard follows: {}\naudio follows: {}", cfg.clipboard, cfg.keyboard_follows, cfg.audio_follows);
            Ok(())
        }
    }
}

fn prompt(q: &str) -> Result<String> {
    use std::io::Write;
    print!("{q}");
    std::io::stdout().flush()?;
    let mut s = String::new();
    std::io::stdin().read_line(&mut s)?;
    Ok(s)
}

fn find_peer(cfg: &Config, query: &str) -> Result<String> {
    let q = query.to_lowercase();
    let hits: Vec<&String> =
        cfg.peers.iter().filter(|(k, p)| p.name.to_lowercase().contains(&q) || k.starts_with(&q)).map(|(k, _)| k).collect();
    match hits.as_slice() {
        [k] => Ok((*k).clone()),
        [] => bail!("no paired computer matches \"{query}\""),
        _ => bail!("\"{query}\" matches more than one computer; use its id from `peers`"),
    }
}

async fn pair_with(to: &str, code: Option<String>) -> Result<()> {
    let addrs = resolve(to).await?;
    let mut cfg = Config::load_or_create()?;
    let (stream, addr) = pairing::ask(&addrs, &cfg).await?;
    let code = match code {
        Some(c) => c,
        None => prompt(&format!("Type the code shown on {to}: "))?,
    };
    let (peer, _) = pairing::complete(stream, addr, &cfg, code.trim(), true, None).await?;
    println!("paired with {} ({})", peer.name, Config::id_of(&peer.key));
    println!("now say where its screen is, e.g. `hidpilot-flow place \"{}\" right`", peer.name);
    cfg.peers.insert(peer.key.clone(), peer);
    cfg.save()
}

/// Accepts ip[:port], or a name announced on the network.
async fn resolve(to: &str) -> Result<Vec<SocketAddr>> {
    if let Ok(a) = to.parse::<SocketAddr>() {
        return Ok(vec![a]);
    }
    if let Ok(ip) = to.parse::<std::net::IpAddr>() {
        return Ok(vec![SocketAddr::new(ip, PORT)]);
    }
    let cfg = Config::load_or_create()?;
    let (_d, mut found) = discovery::start(&cfg)?;
    let want = to.to_lowercase();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(6);
    while let Ok(Some(ev)) = tokio::time::timeout_at(deadline, found.recv()).await {
        let discovery::Event::Found(f) = ev else { continue };
        if f.name.to_lowercase() == want || f.key.starts_with(&want) {
            return Ok(f.addrs);
        }
    }
    bail!("\"{to}\" was not found on the network; try its IP address")
}
