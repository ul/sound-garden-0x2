use anyhow::{Result, anyhow};
use audio_ops::{MIDI_EVENT_RING_CAPACITY, MidiEvent};
use audio_vm::Sample;
use midir::{Ignore, MidiInput, MidiInputConnection, MidiInputPort};
use rtrb::{Producer, RingBuffer};
use std::time::Instant;

/// The MIDI messages Sound Garden understands. Notes go to the per-frame note
/// bus read by `mpoly`; controllers and bend update the shared `MidiControls`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum MidiMessage {
    Note(MidiEvent),
    /// Controller number and value normalised to 0..1.
    Controller {
        controller: u8,
        value: Sample,
    },
    /// Pitch bend normalised to -1..1.
    Bend(Sample),
}

/// A MIDI message stamped with its arrival time, so the audio callback can
/// place it on the matching frame.
#[derive(Clone, Copy, Debug)]
pub struct TimedMidiEvent {
    pub at: Instant,
    pub message: MidiMessage,
}

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
) -> Result<Option<(MidiInputHandle, rtrb::Consumer<TimedMidiEvent>, String)>> {
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
            let (producer, consumer) = RingBuffer::<TimedMidiEvent>::new(MIDI_EVENT_RING_CAPACITY);
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
    mut producer: Producer<TimedMidiEvent>,
) -> Result<MidiInputConnection<()>> {
    Ok(input.connect(
        port,
        "sound-garden-midi-in",
        // midir's timestamp has a backend-specific origin that can't be compared
        // with the audio clock, so stamp arrival with Instant instead.
        move |_timestamp, message, _| {
            if let Some(message) = decode_message(message) {
                producer
                    .push(TimedMidiEvent {
                        at: Instant::now(),
                        message,
                    })
                    .ok();
            }
        },
        (),
    )?)
}

fn decode_message(message: &[u8]) -> Option<MidiMessage> {
    let status = *message.first()?;
    let channel = status & 0x0f;
    match status & 0xf0 {
        0x80 if message.len() >= 3 => {
            Some(MidiMessage::Note(MidiEvent::note_off(channel, message[1])))
        }
        0x90 if message.len() >= 3 => {
            let velocity = f64::from(message[2]) / 127.0;
            Some(MidiMessage::Note(if velocity > 0.0 {
                MidiEvent::note_on(channel, message[1], velocity)
            } else {
                MidiEvent::note_off(channel, message[1])
            }))
        }
        0xB0 if message.len() >= 3 => Some(MidiMessage::Controller {
            controller: message[1] & 0x7f,
            value: f64::from(message[2] & 0x7f) / 127.0,
        }),
        0xE0 if message.len() >= 3 => {
            // 14-bit, LSB first, centred on 8192. Scale each side separately
            // so both extremes reach exactly -1 and 1.
            let raw = ((i32::from(message[2] & 0x7f)) << 7 | i32::from(message[1] & 0x7f)) - 8192;
            let value = if raw >= 0 {
                f64::from(raw) / 8191.0
            } else {
                f64::from(raw) / 8192.0
            };
            Some(MidiMessage::Bend(value))
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
            Some(MidiMessage::Note(MidiEvent::note_on(1, 60, 64.0 / 127.0)))
        );
        assert!(matches!(
            decode_message(&[0x91, 60, 0]),
            Some(MidiMessage::Note(event)) if event.kind == MidiEventKind::NoteOff
        ));
    }

    #[test]
    fn decodes_controllers_on_any_channel() {
        for status in [0xB0, 0xB5, 0xBF] {
            assert_eq!(
                decode_message(&[status, 74, 127]),
                Some(MidiMessage::Controller {
                    controller: 74,
                    value: 1.0
                })
            );
        }
        assert_eq!(
            decode_message(&[0xB0, 1, 0]),
            Some(MidiMessage::Controller {
                controller: 1,
                value: 0.0
            })
        );
    }

    #[test]
    fn decodes_pitch_bend_to_exact_extremes_and_centre() {
        assert_eq!(
            decode_message(&[0xE0, 0x00, 0x40]),
            Some(MidiMessage::Bend(0.0))
        );
        assert_eq!(
            decode_message(&[0xE3, 0x7f, 0x7f]),
            Some(MidiMessage::Bend(1.0))
        );
        assert_eq!(
            decode_message(&[0xE0, 0x00, 0x00]),
            Some(MidiMessage::Bend(-1.0))
        );
        // Truncated messages are ignored rather than misread.
        assert_eq!(decode_message(&[0xE0, 0x00]), None);
    }
}
