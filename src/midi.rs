use midir::{Ignore, MidiInput};
use std::sync::mpsc::{self, Receiver, Sender};

pub struct MidiReceiver {
    pub rx: Receiver<u8>,
    // Keep connection alive.
    _conn: midir::MidiInputConnection<()>,
}

impl MidiReceiver {
    /// Connect to the first available MIDI port whose name contains `port_hint`,
    /// or the first port if `port_hint` is None.
    pub fn connect(port_hint: Option<&str>) -> Result<Self, String> {
        let mut input = MidiInput::new("midi-staff-trainer").map_err(|e| e.to_string())?;
        input.ignore(Ignore::None);

        let ports = input.ports();
        if ports.is_empty() {
            return Err("No MIDI input ports found".into());
        }

        let port = if let Some(hint) = port_hint {
            ports
                .iter()
                .find(|p| {
                    input
                        .port_name(p)
                        .map(|n| n.contains(hint))
                        .unwrap_or(false)
                })
                .ok_or_else(|| format!("No port matching '{hint}'"))?
        } else {
            &ports[0]
        };

        let port_name = input.port_name(port).unwrap_or_default();
        let (tx, rx): (Sender<u8>, Receiver<u8>) = mpsc::channel();

        let conn = input
            .connect(
                port,
                "midi-staff-trainer-in",
                move |_stamp, msg, _| {
                    // Note-on with velocity > 0
                    if msg.len() >= 3 && (msg[0] & 0xF0) == 0x90 && msg[2] > 0 {
                        let _ = tx.send(msg[1]);
                    }
                },
                (),
            )
            .map_err(|e| e.to_string())?;

        eprintln!("Connected to MIDI port: {port_name}");
        Ok(Self { rx, _conn: conn })
    }

    /// List available MIDI input port names.
    pub fn list_ports() -> Vec<String> {
        let mut input = match MidiInput::new("midi-staff-trainer-list") {
            Ok(i) => i,
            Err(_) => return vec![],
        };
        input.ignore(Ignore::None);
        input
            .ports()
            .iter()
            .filter_map(|p| input.port_name(p).ok())
            .collect()
    }
}
