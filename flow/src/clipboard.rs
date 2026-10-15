//! Clipboard hand-off. The clipboard travels once, when the mouse moves to the other
//! machine, never continuously, so nothing leaks while you work on one screen.

use crate::proto::Clip;
use base64::Engine;
use std::sync::mpsc;
use std::thread;
use tokio::sync::oneshot;

enum Req {
    Take(oneshot::Sender<Option<Clip>>),
    Put(Clip),
}

#[derive(Clone)]
pub struct Clipboard {
    tx: mpsc::Sender<Req>,
}

const MAX_IMAGE_BYTES: usize = 48 << 20;

impl Clipboard {
    /// The X11 clipboard is owned by a live client, so one thread keeps it for the
    /// whole session and serves requests.
    pub fn start() -> Clipboard {
        let (tx, rx) = mpsc::channel::<Req>();
        thread::spawn(move || {
            let Ok(mut cb) = arboard::Clipboard::new() else {
                eprintln!("flow: clipboard unavailable");
                return;
            };
            // What we last sent or received, so the same content is not bounced back.
            let mut last: Option<Clip> = None;
            for req in rx {
                match req {
                    Req::Take(reply) => {
                        let now = read(&mut cb);
                        let fresh = now.is_some() && now != last;
                        if fresh {
                            last = now.clone();
                        }
                        let _ = reply.send(if fresh { now } else { None });
                    }
                    Req::Put(clip) => {
                        if write(&mut cb, &clip) {
                            last = Some(clip);
                        }
                    }
                }
            }
        });
        Clipboard { tx }
    }

    /// Current clipboard if it changed since the last hand-off.
    pub async fn take_if_new(&self) -> Option<Clip> {
        let (tx, rx) = oneshot::channel();
        self.tx.send(Req::Take(tx)).ok()?;
        rx.await.ok().flatten()
    }

    pub fn put(&self, clip: Clip) {
        let _ = self.tx.send(Req::Put(clip));
    }
}

fn read(cb: &mut arboard::Clipboard) -> Option<Clip> {
    if let Ok(text) = cb.get_text() {
        if !text.is_empty() {
            return Some(Clip::Text { text });
        }
    }
    let img = cb.get_image().ok()?;
    if img.bytes.len() > MAX_IMAGE_BYTES {
        return None;
    }
    Some(Clip::Image {
        width: img.width,
        height: img.height,
        rgba: base64::engine::general_purpose::STANDARD.encode(&img.bytes),
    })
}

fn write(cb: &mut arboard::Clipboard, clip: &Clip) -> bool {
    match clip {
        Clip::Text { text } => cb.set_text(text.clone()).is_ok(),
        Clip::Image { width, height, rgba } => {
            let Ok(bytes) = base64::engine::general_purpose::STANDARD.decode(rgba) else { return false };
            if bytes.len() != width * height * 4 {
                return false;
            }
            cb.set_image(arboard::ImageData { width: *width, height: *height, bytes: bytes.into() }).is_ok()
        }
    }
}
