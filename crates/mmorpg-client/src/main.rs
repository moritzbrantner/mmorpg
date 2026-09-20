#![forbid(unsafe_code)]

mod desktop;

use mmorpg_client::{
    ClientError, graphics::render_offscreen, network::ClientSession, presentation::Presentation,
};
use mmorpg_core::{ZoneId, ZoneSnapshot, outpost_definition};
use std::{
    path::PathBuf,
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::sync::{oneshot, watch};

#[derive(Clone)]
enum NetworkUpdate {
    Waiting,
    Snapshot(Arc<ZoneSnapshot>),
    Failed(String),
}

struct Options {
    url: String,
    certificate: Option<PathBuf>,
    zone_id: ZoneId,
    smoke: bool,
    frames: Option<u32>,
}

impl Options {
    fn parse(args: impl IntoIterator<Item = String>) -> Result<Option<Self>, ClientError> {
        let mut options = Self {
            url: "https://localhost:4433/game/matches/zone-1".into(),
            certificate: None,
            zone_id: ZoneId::new(1),
            smoke: false,
            frames: None,
        };
        let mut args = args.into_iter();
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--help" | "-h" => return Ok(None),
                "--url" => options.url = args.next().ok_or("--url needs a value")?,
                "--certificate" => {
                    options.certificate =
                        Some(args.next().ok_or("--certificate needs a PEM path")?.into())
                }
                "--zone" => {
                    options.zone_id = ZoneId::new(args.next().ok_or("--zone needs an ID")?.parse()?)
                }
                "--smoke" => options.smoke = true,
                "--frames" => {
                    let count = args.next().ok_or("--frames needs a count")?.parse()?;
                    if count == 0 {
                        return Err("--frames must be positive".into());
                    }
                    options.frames = Some(count);
                }
                _ => return Err(format!("unknown option {arg}").into()),
            }
        }
        if !options.url.starts_with("https://") {
            return Err("WebTransport requires an https URL".into());
        }
        Ok(Some(options))
    }
}

fn main() -> Result<(), ClientError> {
    let Some(options) = Options::parse(std::env::args().skip(1))? else {
        println!(
            "mmorpg-client [--url https://host:4433/game/matches/zone-1] [--zone 1] [--certificate cert.pem] [--smoke] [--frames N]\nWASD/arrows move. Escape closes. --smoke connects and verifies an offscreen GPU frame.\nWithout --certificate, system certificate trust is used."
        );
        return Ok(());
    };
    let runtime = Arc::new(tokio::runtime::Runtime::new()?);
    let definition = outpost_definition();
    let session = runtime.block_on(ClientSession::connect(
        &options.url,
        options.certificate.as_deref(),
        options.zone_id,
        definition.revision(),
    ))?;
    let player_id = session.player_id();
    if options.smoke {
        return runtime.block_on(async {
            let snapshot = session.receive_snapshot().await?;
            let tick = snapshot.tick;
            let mut presentation = Presentation::new(player_id, definition, Instant::now());
            presentation.push(snapshot, Instant::now())?;
            let now = Instant::now();
            let colors = render_offscreen(&presentation.scene(now), presentation.camera_target(now)).await?;
            println!("{{\"event\":\"client_smoke_passed\",\"player_id\":{player_id},\"tick\":{tick},\"rendered_colors\":{colors}}}");
            Ok(())
        });
    }
    let (input_sender, input_receiver) = watch::channel([0_i8; 2]);
    let (update_sender, update_receiver) = watch::channel(NetworkUpdate::Waiting);
    let (shutdown_sender, shutdown_receiver) = oneshot::channel();
    let mut task = runtime.spawn(async move {
        if let Err(error) =
            run_session(session, input_receiver, &update_sender, shutdown_receiver).await
        {
            // The owning window observes this final state; no detached errors.
            update_sender.send_replace(NetworkUpdate::Failed(error.to_string()));
        }
    });
    let result = desktop::run(
        Arc::clone(&runtime),
        player_id,
        definition,
        input_sender,
        update_receiver,
        options.frames,
    );
    // A closed worker already owns its connection cleanup.
    let _ = shutdown_sender.send(());
    runtime.block_on(async {
        match tokio::time::timeout(Duration::from_secs(3), &mut task).await {
            Ok(result) => result?,
            Err(_) => {
                task.abort();
                if let Err(error) = task.await
                    && !error.is_cancelled()
                {
                    return Err::<(), ClientError>(error.into());
                }
            }
        }
        Ok::<(), ClientError>(())
    })?;
    result
}

async fn run_session(
    mut session: ClientSession,
    input: watch::Receiver<[i8; 2]>,
    updates: &watch::Sender<NetworkUpdate>,
    mut shutdown: oneshot::Receiver<()>,
) -> Result<(), ClientError> {
    let mut heartbeat = tokio::time::interval(Duration::from_millis(50));
    heartbeat.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut last_snapshot = Instant::now();
    loop {
        tokio::select! {
            _ = &mut shutdown => return Ok(()),
            _ = heartbeat.tick() => {
                if last_snapshot.elapsed() > Duration::from_secs(5) { return Err("server snapshots stopped".into()); }
                let [x, z] = *input.borrow();
                session.send_movement(x, z)?;
            }
            snapshot = session.receive_snapshot() => {
                last_snapshot = Instant::now();
                updates.send_replace(NetworkUpdate::Snapshot(Arc::new(snapshot?)));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn options_fail_closed_on_unknown_or_invalid_configuration() {
        for args in [
            vec!["--unknown"],
            vec!["--url", "http://localhost"],
            vec!["--certificate"],
            vec!["--zone", "invalid"],
            vec!["--frames", "0"],
        ] {
            assert!(Options::parse(args.into_iter().map(str::to_owned)).is_err());
        }
    }
}
