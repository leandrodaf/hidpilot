//! hidpilot-flow: move the mouse and keyboard between computers by pushing the pointer
//! past the screen edge, like Logitech Flow, with end-to-end encryption and no cloud.

mod agent;
mod clipboard;
mod config;
mod devices;
mod discovery;
mod link;
mod pointer;
mod proto;

use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};
use config::{Config, PORT, Peer, Side};
use proto::{Msg, VERSION};
use std::net::SocketAddr;
use std::time::Duration;
use tokio::net::{TcpListener, TcpStream};

/// Pairing listens on its own port, and only while `pair` runs.
const PAIR_PORT: u16 = PORT + 1;
const PAIR_WINDOW: Duration = Duration::from_secs(120);

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
    /// Pair with another computer. Without --to, shows a code and waits for it.
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
    /// Remove a paired computer
    Forget { peer: String },
    /// Show or change options
    Set {
        #[arg(long)]
        clipboard: Option<bool>,
        #[arg(long)]
        keyboard_follows: Option<bool>,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    match Cli::parse().cmd.unwrap_or(Cmd::Run) {
        Cmd::Run => agent::run().await,
        Cmd::Pair { to: None, .. } => pair_wait().await,
        Cmd::Pair { to: Some(to), code } => {
            let code = match code {
                Some(c) => c,
                None => prompt("Code shown on the other computer: ")?,
            };
            pair_with(&to, code.trim()).await
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
        Cmd::Set { clipboard, keyboard_follows } => {
            let mut cfg = Config::load_or_create()?;
            if let Some(v) = clipboard {
                cfg.clipboard = v;
            }
            if let Some(v) = keyboard_follows {
                cfg.keyboard_follows = v;
            }
            cfg.save()?;
            println!("clipboard: {}\nkeyboard follows: {}", cfg.clipboard, cfg.keyboard_follows);
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

fn random_code() -> String {
    format!("{:06}", rand::random::<u32>() % 1_000_000)
}

/// After the pairing handshake, both sides introduce themselves and store the other.
async fn finish_pairing(mut l: link::Link, addr: Option<SocketAddr>) -> Result<()> {
    let mut cfg = Config::load_or_create()?;
    let hello = Msg::Hello { name: cfg.name.clone(), version: VERSION, devices: Vec::new() };
    l.writer.send(&hello.encode()).await?;
    let Msg::Hello { name, .. } = Msg::decode(&l.reader.recv().await?)? else { bail!("unexpected message") };
    let key = config::hex(&l.remote_key);
    cfg.peers.insert(
        key.clone(),
        Peer { name: name.clone(), key: key.clone(), side: None, addr: addr.map(|a| SocketAddr::new(a.ip(), PORT).to_string()) },
    );
    cfg.save()?;
    println!("paired with {name} ({})", Config::id_of(&key));
    println!("now say where its screen is, e.g. `hidpilot-flow place \"{name}\" right`");
    Ok(())
}

async fn pair_wait() -> Result<()> {
    let cfg = Config::load_or_create()?;
    let code = random_code();
    let listener = TcpListener::bind(("0.0.0.0", PAIR_PORT)).await?;
    println!("Pairing code for {}: {}  {}", cfg.name, &code[..3], &code[3..]);
    println!("On the other computer run: hidpilot-flow pair --to \"{}\"", cfg.name);
    println!("Waiting up to {} s...", PAIR_WINDOW.as_secs());
    let private = cfg.private()?;
    let deadline = tokio::time::Instant::now() + PAIR_WINDOW;
    // A wrong code does not end the window, but three failures do: each one is a guess.
    let mut failures = 0;
    loop {
        let (stream, addr) = tokio::time::timeout_at(deadline, listener.accept()).await.context("nobody paired in time")??;
        match link::pair(stream, &private, &code, false).await {
            Ok(l) => return finish_pairing(l, Some(addr)).await,
            Err(e) => {
                failures += 1;
                eprintln!("pairing attempt from {addr} failed: {e:#}");
                if failures >= 3 {
                    bail!("too many failed attempts; run pair again for a new code");
                }
            }
        }
    }
}

async fn pair_with(to: &str, code: &str) -> Result<()> {
    let code: String = code.chars().filter(|c| c.is_ascii_digit()).collect();
    if code.len() != 6 {
        bail!("the code has 6 digits");
    }
    let addrs = resolve(to).await?;
    let cfg = Config::load_or_create()?;
    let private = cfg.private()?;
    for addr in addrs {
        let pair_addr = SocketAddr::new(addr.ip(), PAIR_PORT);
        let Ok(Ok(stream)) = tokio::time::timeout(Duration::from_secs(4), TcpStream::connect(pair_addr)).await else { continue };
        let l = link::pair(stream, &private, &code, true).await?;
        return finish_pairing(l, Some(pair_addr)).await;
    }
    bail!("could not reach {to}")
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
    while let Ok(Some(f)) = tokio::time::timeout_at(deadline, found.recv()).await {
        if f.name.to_lowercase() == want || f.key.starts_with(&want) {
            return Ok(f.addrs);
        }
    }
    bail!("\"{to}\" was not found on the network; try its IP address")
}
