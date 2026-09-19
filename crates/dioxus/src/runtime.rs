//! Drive terminal input and component work without an application-specific loop.
use crate::View;
use futures_lite::{future, StreamExt};
use wove::{
    terminal::{self, EventStream, Terminal},
    Event,
};

/// Run until `quit` accepts an event or input closes. Which keys quit is the
/// application's decision; an accepted event is not dispatched. Component tasks
/// are polled while waiting for input. The caller chooses the executor;
/// terminal modes use RAII.
pub async fn run(
    view: &mut View,
    mut quit: impl FnMut(&Event) -> bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut terminal = Terminal::new()?;
    let mut input = EventStream::new();
    loop {
        let (width, height) = terminal.size()?;
        terminal.draw(view.frame(width, height)?)?;
        let event = future::race(async { Some(input.next().await) }, async {
            view.wait_for_work().await;
            None
        })
        .await;
        let Some(event) = event else {
            continue;
        };
        let Some(event) = event else {
            break;
        };
        let Some(event) = terminal::convert(event?) else {
            continue;
        };
        if quit(&event) {
            break;
        }
        view.send(event)?;
    }
    Ok(())
}
