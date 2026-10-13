use nih_plug::prelude::*;
use std::f32::consts::TAU;
use std::sync::Arc;

const NUM_VOICES: usize = 16;

#[derive(Enum, PartialEq, Clone, Copy)]
enum Waveform {
    #[name = "Sine"]
    Sine,
    #[name = "Saw"]
    Saw,
    #[name = "Square"]
    Square,
}

#[derive(Params)]
struct SynthParams {
    #[id = "wave"]
    waveform: EnumParam<Waveform>,

    #[id = "gain"]
    gain: FloatParam,

    #[id = "attack"]
    attack: FloatParam,

    #[id = "release"]
    release: FloatParam,
}

impl Default for SynthParams {
    fn default() -> Self {
        Self {
            waveform: EnumParam::new("Waveform", Waveform::Saw),
            gain: FloatParam::new(
                "Gain",
                util::db_to_gain(-12.0),
                FloatRange::Skewed {
                    min: util::db_to_gain(-60.0),
                    max: util::db_to_gain(0.0),
                    factor: FloatRange::gain_skew_factor(-60.0, 0.0),
                },
            )
            .with_smoother(SmoothingStyle::Logarithmic(20.0))
            .with_unit(" dB")
            .with_value_to_string(formatters::v2s_f32_gain_to_db(1))
            .with_string_to_value(formatters::s2v_f32_gain_to_db()),

            attack: FloatParam::new(
                "Attack",
                0.005,
                FloatRange::Skewed { min: 0.001, max: 4.0, factor: FloatRange::skew_factor(-2.0) },
            )
            .with_unit(" s"),

            release: FloatParam::new(
                "Release",
                0.2,
                FloatRange::Skewed { min: 0.005, max: 6.0, factor: FloatRange::skew_factor(-2.0) },
            )
            .with_unit(" s"),
        }
    }
}

#[derive(Clone, Copy)]
struct Voice {
    note: u8,
    velocity: f32,
    phase: f32,
    phase_inc: f32,
    level: f32,
    releasing: bool,
}

struct BasicSynth {
    params: Arc<SynthParams>,
    voices: [Option<Voice>; NUM_VOICES],
    sample_rate: f32,
}

impl Default for BasicSynth {
    fn default() -> Self {
        Self {
            params: Arc::new(SynthParams::default()),
            voices: [None; NUM_VOICES],
            sample_rate: 44100.0,
        }
    }
}

// PolyBLEP: smooths the sharp edges of saw/square so they don't alias harshly.
fn poly_blep(t: f32, dt: f32) -> f32 {
    if t < dt {
        let x = t / dt;
        2.0 * x - x * x - 1.0
    } else if t > 1.0 - dt {
        let x = (t - 1.0) / dt;
        x * x + 2.0 * x + 1.0
    } else {
        0.0
    }
}

fn oscillator(wave: Waveform, phase: f32, dt: f32) -> f32 {
    match wave {
        Waveform::Sine => (phase * TAU).sin(),
        Waveform::Saw => (2.0 * phase - 1.0) - poly_blep(phase, dt),
        Waveform::Square => {
            let naive = if phase < 0.5 { 1.0 } else { -1.0 };
            naive + poly_blep(phase, dt) - poly_blep((phase + 0.5) % 1.0, dt)
        }
    }
}

impl Plugin for BasicSynth {
    const NAME: &'static str = "Basic Synth";
    const VENDOR: &'static str = "Prototype";
    const URL: &'static str = "https://example.com";
    const EMAIL: &'static str = "none@example.com";
    const VERSION: &'static str = env!("CARGO_PKG_VERSION");

    const AUDIO_IO_LAYOUTS: &'static [AudioIOLayout] = &[AudioIOLayout {
        main_input_channels: None,
        main_output_channels: NonZeroU32::new(2),
        ..AudioIOLayout::const_default()
    }];

    const MIDI_INPUT: MidiConfig = MidiConfig::Basic;
    const SAMPLE_ACCURATE_AUTOMATION: bool = true;

    type SysExMessage = ();
    type BackgroundTask = ();

    fn params(&self) -> Arc<dyn Params> {
        self.params.clone()
    }

    fn initialize(
        &mut self,
        _audio_io_layout: &AudioIOLayout,
        buffer_config: &BufferConfig,
        _context: &mut impl InitContext<Self>,
    ) -> bool {
        self.sample_rate = buffer_config.sample_rate;
        true
    }

    fn reset(&mut self) {
        self.voices = [None; NUM_VOICES];
    }

    fn process(
        &mut self,
        buffer: &mut Buffer,
        _aux: &mut AuxiliaryBuffers,
        context: &mut impl ProcessContext<Self>,
    ) -> ProcessStatus {
        let mut next_event = context.next_event();
        let sr = self.sample_rate;

        for (sample_id, channel_samples) in buffer.iter_samples().enumerate() {
            // Handle MIDI events that happen at this sample
            while let Some(event) = next_event {
                if event.timing() > sample_id as u32 {
                    break;
                }
                match event {
                    NoteEvent::NoteOn { note, velocity, .. } => {
                        let freq = util::midi_note_to_freq(note);
                        let voice = Voice {
                            note,
                            velocity,
                            phase: 0.0,
                            phase_inc: freq / sr,
                            level: 0.0,
                            releasing: false,
                        };
                        // Free slot, otherwise steal the first voice
                        let slot = self.voices.iter().position(|v| v.is_none()).unwrap_or(0);
                        self.voices[slot] = Some(voice);
                    }
                    NoteEvent::NoteOff { note, .. } => {
                        for v in self.voices.iter_mut().flatten() {
                            if v.note == note {
                                v.releasing = true;
                            }
                        }
                    }
                    NoteEvent::Choke { note, .. } => {
                        for v in self.voices.iter_mut() {
                            if matches!(v, Some(x) if x.note == note) {
                                *v = None;
                            }
                        }
                    }
                    _ => (),
                }
                next_event = context.next_event();
            }

            let wave = self.params.waveform.value();
            let gain = self.params.gain.smoothed.next();
            let attack_step = 1.0 / (self.params.attack.value() * sr).max(1.0);
            let release_step = 1.0 / (self.params.release.value() * sr).max(1.0);

            let mut out = 0.0f32;
            for slot in self.voices.iter_mut() {
                if let Some(v) = slot {
                    if v.releasing {
                        v.level -= release_step;
                        if v.level <= 0.0 {
                            *slot = None;
                            continue;
                        }
                    } else if v.level < 1.0 {
                        v.level = (v.level + attack_step).min(1.0);
                    }
                    out += oscillator(wave, v.phase, v.phase_inc) * v.level * v.velocity;
                    v.phase += v.phase_inc;
                    if v.phase >= 1.0 {
                        v.phase -= 1.0;
                    }
                }
            }

            let out = out * gain;
            for sample in channel_samples {
                *sample = out;
            }
        }

        ProcessStatus::KeepAlive
    }
}

impl ClapPlugin for BasicSynth {
    const CLAP_ID: &'static str = "com.prototype.basic-synth";
    const CLAP_DESCRIPTION: Option<&'static str> = Some("Simple sine/saw/square synth");
    const CLAP_MANUAL_URL: Option<&'static str> = None;
    const CLAP_SUPPORT_URL: Option<&'static str> = None;
    const CLAP_FEATURES: &'static [ClapFeature] =
        &[ClapFeature::Instrument, ClapFeature::Synthesizer, ClapFeature::Stereo];
}

impl Vst3Plugin for BasicSynth {
    const VST3_CLASS_ID: [u8; 16] = *b"BasicSynthProto1";
    const VST3_SUBCATEGORIES: &'static [Vst3SubCategory] =
        &[Vst3SubCategory::Instrument, Vst3SubCategory::Synth];
}

nih_export_clap!(BasicSynth);
nih_export_vst3!(BasicSynth);
