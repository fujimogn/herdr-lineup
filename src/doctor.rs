//! Read-only endpoint and daemon compatibility diagnostics.

use crate::{
    config::Config,
    daemon::{Options, probe},
    lock::{RuntimePaths, SessionLock},
    socket::Transport,
};
use std::io;

pub const MIN_HERDR_VERSION: &str = "0.9.3";
pub fn check_transport(transport: &mut Transport) -> io::Result<()> {
    transport.send_line(br#"{"id":"lineup-health","method":"ping","params":{}}"#)?;
    validate_pong(&transport.receive_line()?)
}
pub fn validate_pong(frame: &[u8]) -> io::Result<()> {
    let response: serde_json::Value = serde_json::from_slice(frame)
        .map_err(|_| io::Error::other("invalid Herdr health response"))?;
    let result = &response["result"];
    if response["id"] != "lineup-health"
        || result["type"] != "pong"
        || result["protocol"]
            .as_u64()
            .filter(|p| *p > 0 && *p <= u64::from(u32::MAX))
            .is_none()
    {
        return Err(io::Error::other(
            "Herdr JSON protocol health response is incompatible",
        ));
    }
    let version = result["version"]
        .as_str()
        .ok_or_else(|| io::Error::other("Herdr version is missing"))?;
    let version = version.split('+').next().unwrap_or(version);
    if version.contains('-') {
        return Err(io::Error::other(
            "Herdr prerelease compatibility is unverified",
        ));
    }
    let numbers: Option<Vec<u64>> = version.split('.').map(|part| part.parse().ok()).collect();
    let numbers = numbers
        .filter(|v| v.len() == 3)
        .ok_or_else(|| io::Error::other("invalid Herdr version"))?;
    if numbers.as_slice() < [0, 9, 3].as_slice() {
        return Err(io::Error::other("Herdr 0.9.3 or later is required"));
    }
    // JSON API methods are build independent. Binary protocol number equality
    // would incorrectly reject a compatible newer server (official stability contract).
    Ok(())
}
pub fn run(options: &Options) -> io::Result<()> {
    let config = Config::load(&options.config)?;
    let mut transport = Transport::connect(&options.socket)?;
    check_transport(&mut transport)?;
    let paths = RuntimePaths::new(&options.state, &options.socket)?;
    let health = probe(&paths, &options.socket)?;
    if let Some(health) = health {
        if health.refresh_failed || health.last_refresh_ms.is_none() {
            return Err(io::Error::other("daemon refresh is unhealthy"));
        }
        println!(
            "Herdr JSON API compatible; config valid; daemon/control/lock healthy; last refresh {} ms after startup",
            health.last_refresh_ms.unwrap_or(0)
        );
    } else {
        if SessionLock::is_held(&paths)? {
            return Err(io::Error::other(
                "daemon lock is live but control is unavailable",
            ));
        }
        if paths.pid.exists() {
            return Err(io::Error::other(
                "stale diagnostic PID record; no process was signalled",
            ));
        }
        return Err(io::Error::other(
            "daemon is stopped; invoke ensure explicitly",
        ));
    }
    println!(
        "Metadata publication and capacity diagnostics are pending the publication assignment."
    );
    println!(
        "Font mode: {}; auto uses conservative text fallback.",
        config.logo_mode
    );
    println!("Optional provider data may be unavailable; no reader or network call was made.");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn validates_minimum_version_and_required_health_response() {
        assert!(validate_pong(br#"{"id":"lineup-health","result":{"type":"pong","version":"0.9.3","protocol":22}}"#).is_ok());
        for reply in [
            br#"{"id":"lineup-health","result":{"type":"pong","version":"0.9.2","protocol":22}}"#
                .as_slice(),
            br#"{"id":"lineup-health","result":{"type":"pong","version":"0.9.3","protocol":0}}"#
                .as_slice(),
            br#"{"id":"wrong","result":{"type":"pong","version":"0.9.3","protocol":22}}"#
                .as_slice(),
        ] {
            assert!(validate_pong(reply).is_err());
        }
    }
}
