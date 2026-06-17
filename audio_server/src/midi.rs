use anyhow::{Result, anyhow};
use audio_ops::{MIDI_EVENT_RING_CAPACITY, MidiEvent};
use midir::{Ignore, MidiInput, MidiInputConnection, MidiInputPort};
use rtrb::{Producer, RingBuffer};

#[derive(Clone, Debug, Default)]
pub enum MidiInputSelection {
    #[default]
    None,
    Auto,
    Match(String),
}

pub struct MidiInputHandle {
    #[allow(dead_code)]
    connection: MidiInputConnection<()>,
}

pub fn list_inputs() -> Result<Vec<String>> {
    let input = MidiInput::new("sound-garden-list-midi")?;
    Ok(input
        .ports()
        .iter()
        .enumerate()
        .map(|(index, port)| {
            let name = input
                .port_name(port)
                .unwrap_or_else(|_| "<unknown>".to_string());
            format!("{index}: {name}")
        })
        .collect())
}

pub fn open_input(
    selection: &MidiInputSelection,
) -> Result<Option<(MidiInputHandle, rtrb::Consumer<MidiEvent>, String)>> {
    match selection {
        MidiInputSelection::None => Ok(None),
        MidiInputSelection::Auto | MidiInputSelection::Match(_) => {
            let mut input = MidiInput::new("sound-garden-midi")?;
            input.ignore(Ignore::None);
            let ports = input.ports();
            let Some(port) = select_port(&input, &ports, selection)? else {
                return Ok(None);
            };
            let name = input
                .port_name(&port)
                .unwrap_or_else(|_| "<unknown>".to_string());
            let (producer, consumer) = RingBuffer::<MidiEvent>::new(MIDI_EVENT_RING_CAPACITY);
            let connection = connect(input, &port, producer)?;
            Ok(Some((MidiInputHandle { connection }, consumer, name)))
        }
    }
}

fn select_port(
    input: &MidiInput,
    ports: &[MidiInputPort],
    selection: &MidiInputSelection,
) -> Result<Option<MidiInputPort>> {
    match selection {
        MidiInputSelection::None => Ok(None),
        MidiInputSelection::Auto => Ok(ports.first().cloned()),
        MidiInputSelection::Match(query) => {
            if let Ok(index) = query.parse::<usize>() {
                return Ok(ports.get(index).cloned());
            }
            let query = query.to_lowercase();
            for port in ports {
                let name = input.port_name(port).unwrap_or_default();
                if name.to_lowercase().contains(&query) {
                    return Ok(Some(port.clone()));
                }
            }
            Err(anyhow!("No MIDI input matching {query:?}"))
        }
    }
}

fn connect(
    input: MidiInput,
    port: &MidiInputPort,
    mut producer: Producer<MidiEvent>,
) -> Result<MidiInputConnection<()>> {
    Ok(input.connect(
        port,
        "sound-garden-midi-in",
        move |_timestamp, message, _| {
            if let Some(event) = decode_message(message) {
                producer.push(event).ok();
            }
        },
        (),
    )?)
}

fn decode_message(message: &[u8]) -> Option<MidiEvent> {
    let status = *message.first()?;
    let channel = status & 0x0f;
    match status & 0xf0 {
        0x80 if message.len() >= 3 => Some(MidiEvent::note_off(channel, message[1])),
        0x90 if message.len() >= 3 => {
            let velocity = f64::from(message[2]) / 127.0;
            if velocity > 0.0 {
                Some(MidiEvent::note_on(channel, message[1], velocity))
            } else {
                Some(MidiEvent::note_off(channel, message[1]))
            }
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use audio_ops::MidiEventKind;

    #[test]
    fn decodes_note_on_and_velocity_zero_as_off() {
        assert_eq!(
            decode_message(&[0x91, 60, 64]),
            Some(MidiEvent::note_on(1, 60, 64.0 / 127.0))
        );
        assert_eq!(
            decode_message(&[0x91, 60, 0]).map(|event| event.kind),
            Some(MidiEventKind::NoteOff)
        );
    }
}
