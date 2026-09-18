use midir::{Ignore, MidiInput};
use std::sync::mpsc::{self, Receiver, Sender};

pub struct MidiReceiver {
    pub rx: Receiver<u8>,
    _conn: midir::MidiInputConnection<()>, // keeps the connection alive
}

impl MidiReceiver {
    /// Connect to the first port whose name contains `port_hint`,
    /// or the very first available port when `port_hint` is `None`.
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
                .find(|p| input.port_name(p).is_ok_and(|n| n.contains(hint)))
                .ok_or_else(|| format!("No MIDI port matching '{hint}'"))?
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
                    // Note-on (status 0x9n) with velocity > 0.
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

    /// List the names of all currently available MIDI input ports.
    #[must_use]
    pub fn list_ports() -> Vec<String> {
        let input = match MidiInput::new("midi-staff-trainer-list") {
            Ok(i) => i,
            Err(_) => return vec![],
        };
        input
            .ports()
            .iter()
            .filter_map(|p| input.port_name(p).ok())
            .collect()
    }
}
