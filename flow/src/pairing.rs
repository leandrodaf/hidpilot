//! Pairing the way Bluetooth does it: the new computer picks one it sees on the network
//! and asks to pair; the one asked shows a 6-digit code (a desktop notification and its
//! Flow page) and the person types it on the computer that asked. The code then keys
//! SPAKE2 + Noise XXpsk3 (link.rs), so each request is one guess and nothing is learned
//! from a wrong one.
//!
//! On the wire, before the handshake, the asking side sends one line of JSON naming itself.

use crate::config::{self, Config, PORT, Peer, Side};
use crate::link::{self, Link};
use crate::proto::{Msg, VERSION};
use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::net::SocketAddr;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

/// Pairing requests arrive on their own port, next to the Flow one.
pub const PAIR_PORT: u16 = PORT + 1;
/// How long a code stays valid.
pub const WINDOW: Duration = Duration::from_secs(120);

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Request {
    pub name: String,
}

pub fn random_code() -> String {
    format!("{:06}", rand::random::<u32>() % 1_000_000)
}

/// Asking side, step 1: reach the computer and ask. Returns the open connection.
pub async fn ask(addrs: &[SocketAddr], me: &Config) -> Result<(TcpStream, SocketAddr)> {
    for addr in addrs {
        let addr = SocketAddr::new(addr.ip(), PAIR_PORT);
        let Ok(Ok(mut stream)) = tokio::time::timeout(Duration::from_secs(4), TcpStream::connect(addr)).await else { continue };
        stream.set_nodelay(true)?;
        let mut line = serde_json::to_vec(&Request { name: me.name.clone() })?;
        line.push(b'\n');
        stream.write_all(&line).await?;
        return Ok((stream, addr));
    }
    bail!("could not reach that computer (is Flow running there, on the same network?)")
}

/// Asked side, step 1: read who is asking (one short line).
/// Byte by byte, so nothing of the handshake that follows is read ahead.
pub async fn read_request(stream: &mut TcpStream) -> Result<Request> {
    let read = async {
        let mut line = Vec::new();
        loop {
            let b = stream.read_u8().await?;
            if b == b'\n' {
                return Ok::<_, anyhow::Error>(line);
            }
            if line.len() >= 512 {
                bail!("request too long");
            }
            line.push(b);
        }
    };
    let line = tokio::time::timeout(Duration::from_secs(5), read).await.context("no request")??;
    let mut r: Request = serde_json::from_slice(&line).context("bad request")?;
    r.name = r.name.chars().filter(|c| !c.is_control()).take(64).collect();
    Ok(r)
}

/// Step 2 on both sides: the handshake on the code, then introductions. `your_side` is
/// where the other computer's screen sits as seen from here, if the person said so; the
/// result carries the new peer and where it said this computer's screen sits.
pub async fn complete(stream: TcpStream, addr: SocketAddr, me: &Config, code: &str, initiator: bool, your_side: Option<Side>) -> Result<(Peer, Option<Side>)> {
    let code: String = code.chars().filter(|c| c.is_ascii_digit()).collect();
    if code.len() != 6 {
        bail!("the code has 6 digits");
    }
    let private = me.private()?;
    let mut l: Link = tokio::time::timeout(WINDOW, link::pair(stream, &private, &code, initiator))
        .await
        .context("the code expired")?
        .map_err(|_| anyhow::anyhow!("wrong code"))?;
    let hello = Msg::Hello { name: me.name.clone(), version: VERSION, devices: Vec::new(), your_side };
    l.writer.send(&hello.encode()).await?;
    let Msg::Hello { name, your_side: mine, .. } = Msg::decode(&l.reader.recv().await?)? else { bail!("unexpected message") };
    let key = config::hex(&l.remote_key);
    let peer = Peer {
        name,
        key,
        side: your_side.or(mine.map(Side::opposite)),
        addr: Some(SocketAddr::new(addr.ip(), PORT).to_string()),
    };
    Ok((peer, mine))
}

/// Shows a desktop notification, best effort (Linux notify-send, macOS osascript,
/// Windows toast through PowerShell).
pub fn notify(title: &str, body: &str) {
    #[cfg(target_os = "linux")]
    let _ = std::process::Command::new("notify-send").args(["-a", "Orbit", "-u", "critical", "-i", "hidpilot", title, body]).spawn();
    #[cfg(target_os = "macos")]
    {
        let q = |s: &str| s.replace('\\', "\\\\").replace('"', "\\\"");
        let script = format!("display notification \"{}\" with title \"{}\" sound name \"Glass\"", q(body), q(title));
        let _ = std::process::Command::new("osascript").args(["-e", &script]).spawn();
    }
    #[cfg(windows)]
    {
        let q = |s: &str| s.replace('\'', "''");
        let script = format!(
            "[Windows.UI.Notifications.ToastNotificationManager, Windows.UI.Notifications, ContentType = WindowsRuntime] | Out-Null; \
             $x = [Windows.UI.Notifications.ToastNotificationManager]::GetTemplateContent([Windows.UI.Notifications.ToastTemplateType]::ToastText02); \
             $t = $x.GetElementsByTagName('text'); $t[0].AppendChild($x.CreateTextNode('{}')) | Out-Null; $t[1].AppendChild($x.CreateTextNode('{}')) | Out-Null; \
             [Windows.UI.Notifications.ToastNotificationManager]::CreateToastNotifier('{{1AC14E77-02E7-4E5D-B744-2EB1AE5198B7}}\\WindowsPowerShell\\v1.0\\powershell.exe').Show([Windows.UI.Notifications.ToastNotification]::new($x))",
            q(title),
            q(body)
        );
        let _ = std::process::Command::new("powershell").args(["-NoProfile", "-WindowStyle", "Hidden", "-Command", &script]).spawn();
    }
}

/// The two notification texts, in the system language when we have it.
pub fn texts(name: &str, code: &str) -> (String, String) {
    let code = format!("{} {}", &code[..3], &code[3..]);
    let lang = std::env::var("LANG").or_else(|_| std::env::var("LC_ALL")).unwrap_or_default().to_lowercase();
    let (t, b) = match lang.get(..2).unwrap_or("en") {
        "pt" => ("{name} quer se conectar", "Digite este código lá: {code}"),
        "es" => ("{name} quiere conectarse", "Escribe este código allí: {code}"),
        "fr" => ("{name} veut se connecter", "Saisissez ce code là-bas : {code}"),
        "de" => ("{name} möchte sich verbinden", "Gib dort diesen Code ein: {code}"),
        "it" => ("{name} vuole connettersi", "Digita questo codice lì: {code}"),
        "ja" => ("{name} が接続を求めています", "相手側でこのコードを入力: {code}"),
        "zh" => ("{name} 请求连接", "在对方电脑上输入此代码：{code}"),
        "ko" => ("{name}에서 연결을 요청합니다", "상대 컴퓨터에 이 코드를 입력하세요: {code}"),
        "ru" => ("{name} хочет подключиться", "Введите там этот код: {code}"),
        _ => ("{name} wants to connect", "Type this code there: {code}"),
    };
    (t.replace("{name}", name), b.replace("{code}", &code))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::net::TcpListener;

    fn config(name: &str) -> Config {
        let kp = link::generate_keypair().unwrap();
        let mut c: Config = toml::from_str(&format!("name = \"{name}\"\nprivate_key = \"\"\npublic_key = \"\"")).unwrap();
        c.private_key = config::hex(&kp.private);
        c.public_key = config::hex(&kp.public);
        c
    }

    /// The asking side sends its line, the asked side reads it and shows `shown`; the
    /// person types `typed`. Returns what each side ended up with.
    async fn run(shown: &str, typed: &str) -> (Result<(Peer, Option<Side>)>, Result<(Peer, Option<Side>)>) {
        let (mac, linux) = (config("mac"), config("linux"));
        let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = l.local_addr().unwrap();
        let asker = async {
            let mut s = TcpStream::connect(addr).await.unwrap();
            s.write_all(b"{\"name\":\"mac\"}\n").await.unwrap();
            complete(s, addr, &mac, typed, true, Some(Side::Left)).await
        };
        let asked = async {
            let (mut s, from) = l.accept().await.unwrap();
            let req = read_request(&mut s).await.unwrap();
            assert_eq!(req.name, "mac");
            complete(s, from, &linux, shown, false, None).await
        };
        tokio::join!(asker, asked)
    }

    #[tokio::test]
    async fn right_code_pairs_and_places_both_screens() {
        let (a, b) = run("123456", "123 456").await;
        let ((on_mac, _), (on_linux, told)) = (a.unwrap(), b.unwrap());
        assert_eq!((on_mac.name.as_str(), on_mac.side), ("linux", Some(Side::Left)));
        // The Mac said the Linux screen is on its left, so the Mac is on the Linux right.
        assert_eq!((on_linux.name.as_str(), on_linux.side, told), ("mac", Some(Side::Right), Some(Side::Left)));
    }

    #[tokio::test]
    async fn wrong_code_pairs_nothing() {
        let (a, b) = run("123456", "654321").await;
        assert!(a.is_err() && b.is_err());
    }
}
