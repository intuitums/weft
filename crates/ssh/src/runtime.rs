//! Run a remote application without acquiring the server terminal.
use crate::{server::Factory, Error, Peer};
use russh::{server, ChannelId};
use std::{
    panic::{catch_unwind, AssertUnwindSafe},
    time::Duration,
};
use tokio::sync::mpsc;
use wove::{input::Decoder, Depth, Event, Key, Options, Renderer};

pub(crate) enum Message {
    Data(Vec<u8>),
    Resize(u16, u16),
}

/// Keep non-Send application state off the network executor. Bounded queues and
/// output deadlines prevent a slow peer from retaining unlimited work. Application
/// panics unwind inside this boundary so remote mode restoration still runs.
pub(crate) fn run(
    factory: Factory,
    peer: Peer,
    mut input: mpsc::Receiver<Message>,
    handle: server::Handle,
    channel: ChannelId,
) {
    let runtime = tokio::runtime::Handle::current();
    let send = |bytes: Vec<u8>| -> Result<(), Error> {
        runtime.block_on(async {
            tokio::time::timeout(Duration::from_secs(5), handle.data(channel, bytes))
                .await
                .map_err(|_| "SSH output timed out")?
                .map_err(|_| "SSH channel closed")?;
            Ok(())
        })
    };
    // The same modes a local terminal session enables, carried over the channel.
    let modes = Options::default();
    let result = catch_unwind(AssertUnwindSafe(|| -> Result<(), Error> {
        let mut app = factory(&peer)?;
        let (mut width, mut height) = (peer.width, peer.height);
        // The peer's terminal type is all that is known of its colors.
        let term = peer.term.clone();
        let depth = Depth::from_env(|name| (name == "TERM").then(|| term.clone()));
        let mut renderer = Renderer::with_depth(depth);
        let mut decoder = Decoder::default();
        let mut bytes = Vec::new();
        modes.enter(&mut bytes)?;
        send(bytes)?;
        loop {
            let mut bytes = Vec::new();
            renderer.draw(&mut bytes, app.frame(width, height)?)?;
            if !bytes.is_empty() {
                send(bytes)?;
            }
            let message = runtime.block_on(async {
                if decoder.escape_pending() {
                    tokio::time::timeout(Duration::from_millis(40), input.recv()).await
                } else {
                    Ok(input.recv().await)
                }
            });
            let events = match message {
                Ok(Some(Message::Data(bytes))) => decoder.push(&bytes),
                Ok(Some(Message::Resize(w, h))) => {
                    width = w;
                    height = h;
                    vec![Event::Resize(w, h)]
                }
                Ok(None) => break,
                Err(_) => decoder.flush_escape(),
            };
            for event in events {
                if matches!(
                    event,
                    Event::Key(Key::Char('c'), wove::Modifiers { ctrl: true, .. })
                ) || !app.event(event)?
                {
                    return Ok(());
                }
            }
        }
        Ok(())
    }))
    .unwrap_or_else(|_| Err("SSH application panicked".into()));
    let mut bytes = Vec::new();
    let _ = modes.leave(&mut bytes);
    let _ = send(bytes);
    let _ = runtime.block_on(async {
        tokio::time::timeout(Duration::from_secs(5), async {
            let _ = handle
                .exit_status_request(channel, u32::from(result.is_err()))
                .await;
            let _ = handle.eof(channel).await;
            let _ = handle.close(channel).await;
        })
        .await
    });
}
