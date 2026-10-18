//! Encrypted channel between two machines.
//!
//! Paired peers talk Noise IK: the caller already holds the callee's pinned key, the
//! callee learns the caller's key in the first message and drops anyone it has not
//! paired with. Pairing itself runs SPAKE2 on a 6-digit code, then Noise XXpsk3 keyed
//! with the SPAKE2 secret, so a wrong code fails the handshake and an attacker gets one
//! guess per attempt instead of an offline brute force.

use anyhow::{Context, Result, anyhow, bail};
use snow::{Builder, HandshakeState, Keypair, StatelessTransportState};
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::net::tcp::{OwnedReadHalf, OwnedWriteHalf};

const PAIRED: &str = "Noise_IK_25519_ChaChaPoly_BLAKE2s";
const PAIRING: &str = "Noise_XXpsk3_25519_ChaChaPoly_BLAKE2s";
const PROLOGUE: &[u8] = b"hidpilot-flow/1";
const MAX_FRAME: usize = 65_535;
const TAG: usize = 16;
/// Plaintext per frame: frame limit minus the AEAD tag and our 1-byte "more" flag.
const CHUNK: usize = MAX_FRAME - TAG - 1;
/// Largest message we accept (clipboard images can be big; anything past this is refused).
const MAX_MESSAGE: usize = 64 << 20;

pub fn generate_keypair() -> Result<Keypair> {
    Ok(Builder::new(PAIRED.parse()?).generate_keypair()?)
}

/// Length and payload go out in one write, so with TCP_NODELAY a message is one packet.
async fn write_frame(w: &mut (impl AsyncWriteExt + Unpin), data: &[u8]) -> Result<()> {
    let mut frame = Vec::with_capacity(2 + data.len());
    frame.extend_from_slice(&(data.len() as u16).to_be_bytes());
    frame.extend_from_slice(data);
    w.write_all(&frame).await?;
    Ok(())
}

async fn read_frame(r: &mut (impl AsyncReadExt + Unpin)) -> Result<Vec<u8>> {
    let mut len = [0u8; 2];
    r.read_exact(&mut len).await?;
    let mut buf = vec![0u8; u16::from_be_bytes(len) as usize];
    r.read_exact(&mut buf).await?;
    Ok(buf)
}

async fn handshake(stream: &mut TcpStream, mut hs: HandshakeState, initiator: bool) -> Result<HandshakeState> {
    let mut buf = vec![0u8; MAX_FRAME];
    let mut sending = initiator;
    while !hs.is_handshake_finished() {
        if sending {
            let n = hs.write_message(&[], &mut buf)?;
            write_frame(stream, &buf[..n]).await?;
        } else {
            let msg = read_frame(stream).await?;
            hs.read_message(&msg, &mut buf).map_err(|_| anyhow!("handshake rejected (wrong code or unknown machine)"))?;
        }
        sending = !sending;
    }
    Ok(hs)
}

pub struct Link {
    pub reader: LinkReader,
    pub writer: LinkWriter,
    /// The other side's static key, verified by the handshake.
    pub remote_key: Vec<u8>,
}

pub struct LinkReader {
    io: OwnedReadHalf,
    noise: Arc<StatelessTransportState>,
    nonce: u64,
}

pub struct LinkWriter {
    io: OwnedWriteHalf,
    noise: Arc<StatelessTransportState>,
    nonce: u64,
}

fn finish(stream: TcpStream, hs: HandshakeState) -> Result<Link> {
    let remote_key = hs.get_remote_static().context("peer sent no static key")?.to_vec();
    let noise = Arc::new(hs.into_stateless_transport_mode()?);
    stream.set_nodelay(true)?;
    let (r, w) = stream.into_split();
    Ok(Link {
        reader: LinkReader { io: r, noise: noise.clone(), nonce: 0 },
        writer: LinkWriter { io: w, noise, nonce: 0 },
        remote_key,
    })
}

/// Opens a channel to an already paired machine whose key we hold.
pub async fn connect(mut stream: TcpStream, private: &[u8], remote: &[u8]) -> Result<Link> {
    let hs = Builder::new(PAIRED.parse()?)
        .local_private_key(private)?
        .remote_public_key(remote)?
        .prologue(PROLOGUE)?
        .build_initiator()?;
    let hs = handshake(&mut stream, hs, true).await?;
    finish(stream, hs)
}

/// Accepts a channel; `trusted` decides whether the caller's key is one we paired with.
pub async fn accept(mut stream: TcpStream, private: &[u8], trusted: impl Fn(&[u8]) -> bool) -> Result<Link> {
    let hs = Builder::new(PAIRED.parse()?).local_private_key(private)?.prologue(PROLOGUE)?.build_responder()?;
    // IK: the first message carries the caller's key; check it before answering.
    let mut hs = hs;
    let mut buf = vec![0u8; MAX_FRAME];
    let msg = read_frame(&mut stream).await?;
    hs.read_message(&msg, &mut buf).map_err(|_| anyhow!("bad handshake"))?;
    let caller = hs.get_remote_static().context("no caller key")?;
    if !trusted(caller) {
        bail!("refused a machine that is not paired ({})", crate::config::hex(&caller[..8]));
    }
    let n = hs.write_message(&[], &mut buf)?;
    write_frame(&mut stream, &buf[..n]).await?;
    finish(stream, hs)
}

/// Pairing handshake. Both sides type or show the same code; returns the peer's key.
pub async fn pair(mut stream: TcpStream, private: &[u8], code: &str, initiator: bool) -> Result<Link> {
    use spake2::{Ed25519Group, Identity, Password, Spake2};
    let (state, outbound) = Spake2::<Ed25519Group>::start_symmetric(&Password::new(code.as_bytes()), &Identity::new(PROLOGUE));
    write_frame(&mut stream, &outbound).await?;
    let inbound = read_frame(&mut stream).await?;
    let secret = state.finish(&inbound).map_err(|_| anyhow!("pairing failed"))?;
    let psk: [u8; 32] = blake2_256(&secret);
    let b = Builder::new(PAIRING.parse()?).local_private_key(private)?.psk(3, &psk)?.prologue(PROLOGUE)?;
    let hs = if initiator { b.build_initiator()? } else { b.build_responder()? };
    let hs = handshake(&mut stream, hs, initiator).await?;
    finish(stream, hs)
}

fn blake2_256(data: &[u8]) -> [u8; 32] {
    use blake2::{Blake2s256, Digest};
    let mut h = Blake2s256::new();
    h.update(b"hidpilot-flow pairing psk");
    h.update(data);
    h.finalize().into()
}

impl LinkWriter {
    pub async fn send(&mut self, msg: &[u8]) -> Result<()> {
        let mut out = vec![0u8; MAX_FRAME];
        let mut chunks = msg.chunks(CHUNK).peekable();
        if msg.is_empty() {
            bail!("empty message");
        }
        while let Some(c) = chunks.next() {
            let mut plain = Vec::with_capacity(c.len() + 1);
            plain.push(chunks.peek().is_some() as u8);
            plain.extend_from_slice(c);
            let n = self.noise.write_message(self.nonce, &plain, &mut out)?;
            self.nonce += 1;
            write_frame(&mut self.io, &out[..n]).await?;
        }
        Ok(())
    }
}

impl LinkReader {
    pub async fn recv(&mut self) -> Result<Vec<u8>> {
        let mut msg = Vec::new();
        let mut plain = vec![0u8; MAX_FRAME];
        loop {
            let frame = read_frame(&mut self.io).await?;
            let n = self.noise.read_message(self.nonce, &frame, &mut plain).map_err(|_| anyhow!("corrupt or forged frame"))?;
            self.nonce += 1;
            if n == 0 {
                bail!("empty frame");
            }
            msg.extend_from_slice(&plain[1..n]);
            if msg.len() > MAX_MESSAGE {
                bail!("message too large");
            }
            if plain[0] == 0 {
                return Ok(msg);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::net::TcpListener;

    async fn socket_pair() -> (TcpStream, TcpStream) {
        let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = l.local_addr().unwrap();
        let (a, b) = tokio::join!(TcpStream::connect(addr), l.accept());
        (a.unwrap(), b.unwrap().0)
    }

    #[tokio::test]
    async fn pairing_then_pinned_link() {
        let (ka, kb) = (generate_keypair().unwrap(), generate_keypair().unwrap());
        let (sa, sb) = socket_pair().await;
        let (a, b) = tokio::join!(pair(sa, &ka.private, "123456", true), pair(sb, &kb.private, "123456", false));
        let (a, b) = (a.unwrap(), b.unwrap());
        assert_eq!(a.remote_key, kb.public);
        assert_eq!(b.remote_key, ka.public);

        // Paired link, with a message far larger than one frame.
        let (sa, sb) = socket_pair().await;
        let pub_a = ka.public.clone();
        let (a, b) = tokio::join!(
            connect(sa, &ka.private, &kb.public),
            accept(sb, &kb.private, move |k: &[u8]| k == pub_a.as_slice())
        );
        let (mut a, mut b) = (a.unwrap(), b.unwrap());
        let big: Vec<u8> = (0..300_000u32).map(|i| i as u8).collect();
        a.writer.send(&big).await.unwrap();
        assert_eq!(b.reader.recv().await.unwrap(), big);
        b.writer.send(b"pong").await.unwrap();
        assert_eq!(a.reader.recv().await.unwrap(), b"pong");
    }

    #[tokio::test]
    async fn wrong_code_fails() {
        let (ka, kb) = (generate_keypair().unwrap(), generate_keypair().unwrap());
        let (sa, sb) = socket_pair().await;
        let (a, b) = tokio::join!(pair(sa, &ka.private, "123456", true), pair(sb, &kb.private, "654321", false));
        assert!(b.is_err(), "responder must reject a wrong code");
        // The initiator may finish its half, but its first real message never gets through.
        if let Ok(mut a) = a {
            let _ = a.writer.send(b"hi").await;
            assert!(a.reader.recv().await.is_err());
        }
    }

    #[tokio::test]
    async fn unpaired_machine_is_refused() {
        let (ka, kb, stranger) = (generate_keypair().unwrap(), generate_keypair().unwrap(), generate_keypair().unwrap());
        let (sa, sb) = socket_pair().await;
        let pub_a = ka.public.clone();
        let (s, b) = tokio::join!(
            connect(sa, &stranger.private, &kb.public),
            accept(sb, &kb.private, move |k: &[u8]| k == pub_a.as_slice())
        );
        assert!(b.is_err());
        assert!(s.is_err());
    }
}
