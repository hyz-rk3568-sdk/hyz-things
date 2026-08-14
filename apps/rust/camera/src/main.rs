use hyz_camera::{
    adapters::{
        inbound::unix_control::serve_root_control,
        outbound::{gstreamer::GStreamerMediaAdapter, webrtc::Str0mWebRtcAdapter},
    },
    application::CameraApplication,
};
use signal_hook::{
    consts::signal::{SIGINT, SIGTERM},
    flag,
};
use std::{
    error::Error,
    sync::{atomic::AtomicBool, Arc},
};

const USAGE: &str = "Usage: hyz-camera daemon";

fn main() -> Result<(), Box<dyn Error>> {
    let mut arguments = std::env::args();
    let _program = arguments.next();
    if arguments.next().as_deref() != Some("daemon") || arguments.next().is_some() {
        return Err(USAGE.into());
    }
    if unsafe { libc::geteuid() } != 0 {
        return Err("hyz-camera must run as root".into());
    }

    let mut random = [0u8; 12];
    getrandom::fill(&mut random).map_err(|_| "secure random generation failed")?;
    let generation: String = random.iter().map(|byte| format!("{byte:02x}")).collect();

    let media = Arc::new(GStreamerMediaAdapter::new()?);
    let webrtc = Arc::new(Str0mWebRtcAdapter::new());
    let application = Arc::new(CameraApplication::new(media, webrtc, generation.clone())?);
    let shutdown = Arc::new(AtomicBool::new(false));
    flag::register(SIGTERM, shutdown.clone())?;
    flag::register(SIGINT, shutdown.clone())?;
    serve_root_control(application, shutdown, &generation)?;
    Ok(())
}
