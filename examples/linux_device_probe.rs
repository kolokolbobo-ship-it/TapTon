//! Diagnostic: print the device names TapTone (cpal/ALSA) sees on this machine.
//! Run with: cargo run --release --example linux_device_probe
//!
//! On Linux the ALSA layer exposes many aliases for the *same* physical device
//! ("default", "pipewire", "sysdefault", "front:CARD=...", ...). TapTone selects
//! devices by these names, so the lists below are exactly what the settings UI
//! offers and what the engine resolves.

use cpal::traits::{DeviceTrait, HostTrait};

fn main() {
    let host = cpal::default_host();
    println!("host: {:?}", host.id());

    let default_out = host.default_output_device().and_then(|d| d.name().ok());
    let default_in = host.default_input_device().and_then(|d| d.name().ok());
    println!("default output: {default_out:?}");
    println!("default input:  {default_in:?}");

    println!("\n--- playback devices (output_devices) ---");
    if let Ok(it) = host.output_devices() {
        for d in it {
            println!("{}", d.name().unwrap_or_else(|_| "<error>".into()));
        }
    }
    println!("\n--- recording devices (input_devices) ---");
    if let Ok(it) = host.input_devices() {
        for d in it {
            println!("{}", d.name().unwrap_or_else(|_| "<error>".into()));
        }
    }

    // What the microphone-route dropdown offers after the Linux cleanup
    // (see taptone::audio::routing::filter_route_output_names).
    println!("\n--- microphone route choices (filtered + sorted) ---");
    match taptone::audio::engine::list_devices(taptone::audio::routing::DeviceKind::Output) {
        Ok(names) => {
            let filtered = taptone::audio::routing::filter_route_output_names(&names);
            for n in taptone::audio::routing::sort_for_mic_route(&filtered) {
                println!("{n}");
            }
        }
        Err(e) => println!("listing failed: {e}"),
    }
}
