//! Audio engine: captures the guitar (sound card) and the voice mic,
//! mixes them and plays the result into a virtual cable that Discord
//! (or any app) can pick as a microphone.

use crate::config::ChannelSel;
use crate::dsp::{db_to_lin, soft_clip, NoiseGate, Peak, Resampler};
use anyhow::{anyhow, bail, Context, Result};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{Device, FromSample, Host, HostId, SampleFormat, SizedSample, Stream, StreamConfig};
use ringbuf::traits::{Consumer, Observer, Producer, Split};
use ringbuf::{HeapCons, HeapProd, HeapRb};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};

// ---------------------------------------------------------------------------
// Shared, lock-free parameters between UI and audio threads
// ---------------------------------------------------------------------------

#[derive(Default)]
pub struct AtomicF32(AtomicU32);

impl AtomicF32 {
    #[allow(dead_code)]
    pub fn new(v: f32) -> Self {
        Self(AtomicU32::new(v.to_bits()))
    }
    #[inline]
    pub fn get(&self) -> f32 {
        f32::from_bits(self.0.load(Ordering::Relaxed))
    }
    #[inline]
    pub fn set(&self, v: f32) {
        self.0.store(v.to_bits(), Ordering::Relaxed)
    }
}

#[derive(Default)]
pub struct Shared {
    pub guitar_gain_db: AtomicF32,
    pub mic_gain_db: AtomicF32,
    pub master_gain_db: AtomicF32,
    pub monitor_gain_db: AtomicF32,
    pub guitar_gate_db: AtomicF32,
    pub mic_gate_db: AtomicF32,
    pub guitar_muted: AtomicBool,
    pub mic_muted: AtomicBool,

    pub guitar_level: AtomicF32,
    pub mic_level: AtomicF32,
    pub out_level: AtomicF32,
    /// Number of output callbacks that ran short of input (underruns).
    pub underruns: AtomicU32,

    pub last_error: Mutex<Option<String>>,
}

impl Shared {
    fn report_error(&self, e: impl std::fmt::Display) {
        let msg = e.to_string();
        // xruns are transient (often one at stream start): count, do not alarm.
        if msg.contains("underrun") || msg.contains("overrun") {
            log::debug!("audio xrun: {msg}");
            self.underruns.fetch_add(1, Ordering::Relaxed);
            return;
        }
        log::error!("audio: {msg}");
        if let Ok(mut g) = self.last_error.lock() {
            *g = Some(e.to_string());
        }
    }
}

// ---------------------------------------------------------------------------
// Device discovery
// ---------------------------------------------------------------------------

#[derive(Clone, Debug)]
pub struct DeviceInfo {
    pub name: String,
    pub channels: u16,
    pub sample_rate: u32,
}

pub fn device_name(d: &Device) -> String {
    d.description()
        .map(|x| x.name().to_owned())
        .unwrap_or_else(|_| d.to_string())
}

/// Hosts compiled in and available on this machine (e.g. "WASAPI", "ASIO").
pub fn host_names() -> Vec<String> {
    cpal::available_hosts()
        .into_iter()
        .map(|id| id.name().to_owned())
        .collect()
}

fn host_by_name(name: Option<&str>) -> Result<Host> {
    match name {
        None => Ok(cpal::default_host()),
        Some(n) => {
            let id: HostId = cpal::available_hosts()
                .into_iter()
                .find(|id| id.name().eq_ignore_ascii_case(n))
                .ok_or_else(|| anyhow!("audio host '{n}' is not available"))?;
            cpal::host_from_id(id).map_err(|e| anyhow!("{e}"))
        }
    }
}

pub fn list_inputs(host: Option<&str>) -> Vec<DeviceInfo> {
    let Ok(host) = host_by_name(host) else { return vec![] };
    let Ok(devs) = host.input_devices() else { return vec![] };
    devs.filter_map(|d| {
        let cfg = d.default_input_config().ok()?;
        Some(DeviceInfo {
            name: device_name(&d),
            channels: cfg.channels(),
            sample_rate: cfg.sample_rate(),
        })
    })
    .collect()
}

pub fn list_outputs(host: Option<&str>) -> Vec<DeviceInfo> {
    let Ok(host) = host_by_name(host) else { return vec![] };
    let Ok(devs) = host.output_devices() else { return vec![] };
    devs.filter_map(|d| {
        let cfg = d.default_output_config().ok()?;
        Some(DeviceInfo {
            name: device_name(&d),
            channels: cfg.channels(),
            sample_rate: cfg.sample_rate(),
        })
    })
    .collect()
}

/// Known virtual-cable playback endpoints. Whatever plays into these
/// shows up as a recording device ("microphone") in Discord.
pub fn is_virtual_cable(name: &str) -> bool {
    cable_rank(name).is_some()
}

/// Preference order when auto-picking the output (lower is better).
/// VB-Cable first: Voicemeeter inputs work too but go through its mixer.
pub fn cable_rank(name: &str) -> Option<u8> {
    let n = name.to_ascii_lowercase();
    if n.contains("guitar to discord") {
        Some(0)
    } else if n.contains("cable input") {
        Some(1)
    } else if n.contains("voicemeeter aux input") {
        Some(2)
    } else if n.contains("voicemeeter input") || n.contains("voicemeeter in ") || n.contains("voicemeeter vaio3 input") {
        Some(3)
    } else if n.contains("vb-audio") && !n.contains("output") && !n.contains(" out ") {
        Some(4)
    } else {
        None
    }
}

/// Recording endpoint Discord should select for a given playback endpoint.
pub fn discord_side_name(output: &str) -> String {
    let n = output.to_ascii_lowercase();
    if n.contains("cable input") {
        "CABLE Output".into()
    } else if n.contains("voicemeeter aux input") {
        "Voicemeeter Out B2".into()
    } else if n.contains("voicemeeter input") {
        "Voicemeeter Out B1".into()
    } else {
        "l'entrée associée à ce câble".into()
    }
}

pub fn guess_output(list: &[DeviceInfo]) -> Option<String> {
    list.iter()
        .filter_map(|d| cable_rank(&d.name).map(|r| (r, d)))
        .min_by_key(|(r, _)| *r)
        .map(|(_, d)| d.name.clone())
}

pub fn guess_guitar(list: &[DeviceInfo]) -> Option<String> {
    const KEYS: [&str; 14] = [
        "focusrite", "scarlett", "audient", "motu", "steinberg", "presonus", "behringer",
        "universal audio", "line 6", "irig", "clarett", "id4", "id14", "rocksmith",
    ];
    list.iter()
        .find(|d| KEYS.iter().any(|k| d.name.to_lowercase().contains(k)))
        .map(|d| d.name.clone())
}

pub fn guess_mic(list: &[DeviceInfo], guitar: Option<&str>) -> Option<String> {
    let ok = |d: &&DeviceInfo| {
        let n = d.name.to_lowercase();
        Some(d.name.as_str()) != guitar && !n.contains("voicemeeter") && !n.contains("cable output") && !n.contains("vb-audio")
    };
    let pref = ["headset", "casque", "hyperx", "microphone", "micro", "mic"];
    pref.iter()
        .find_map(|k| list.iter().filter(ok).find(|d| d.name.to_lowercase().contains(k)))
        .or_else(|| list.iter().find(ok))
        .map(|d| d.name.clone())
}

fn find_input(host: &Host, name: &str) -> Result<Device> {
    host.input_devices()?
        .find(|d| device_name(d) == name)
        .ok_or_else(|| anyhow!("input device '{name}' not found"))
}

fn find_output(host: &Host, name: &str) -> Result<Device> {
    host.output_devices()?
        .find(|d| device_name(d) == name)
        .ok_or_else(|| anyhow!("output device '{name}' not found"))
}

// ---------------------------------------------------------------------------
// Engine
// ---------------------------------------------------------------------------

pub struct InputSel {
    pub host: Option<String>,
    pub device: String,
    pub channel: ChannelSel,
}

pub struct EngineSettings {
    pub guitar: Option<InputSel>,
    pub mic: Option<InputSel>,
    pub output: String,
    pub monitor: Option<String>,
}

pub struct Engine {
    _streams: Vec<Stream>,
    pub summary: String,
}

/// Ring buffer capacity in seconds.
const RING_SECONDS: f32 = 0.5;
/// Latency the output tries to keep (seconds of audio buffered).
const TARGET_LATENCY: f32 = 0.015;
/// Above this the output drops samples to catch up (clock drift).
const MAX_LATENCY: f32 = 0.060;

fn ring(rate: u32) -> (HeapProd<f32>, HeapCons<f32>) {
    HeapRb::<f32>::new((rate as f32 * RING_SECONDS) as usize).split()
}

impl Engine {
    pub fn start(s: EngineSettings, shared: Arc<Shared>) -> Result<Engine> {
        if s.guitar.is_none() && s.mic.is_none() {
            bail!("pick at least a guitar input or a microphone");
        }
        let default_host = cpal::default_host();
        let mut streams = Vec::new();
        let mut summary = Vec::new();

        // ---- main output (virtual cable) ----
        let out_dev = find_output(&default_host, &s.output)?;
        let out_cfg = out_dev
            .default_output_config()
            .context("output device has no default config")?;
        let out_rate = out_cfg.sample_rate();
        summary.push(format!("Out: {} @ {} Hz", s.output, out_rate));

        // ---- optional monitor output (your headphones, guitar only) ----
        let mon = match &s.monitor {
            Some(name) => {
                let d = find_output(&default_host, name)?;
                let c = d.default_output_config()?;
                summary.push(format!("Monitor: {name}"));
                Some((d, c))
            }
            None => None,
        };

        let (g_main_p, g_main_c) = ring(out_rate);
        let (m_main_p, m_main_c) = ring(out_rate);
        let mon_ring = mon.as_ref().map(|(_, c)| ring(c.sample_rate()));
        let (g_mon_p, g_mon_c) = match mon_ring {
            Some((p, c)) => (Some(p), Some(c)),
            None => (None, None),
        };

        // ---- guitar input ----
        let guitar_active = s.guitar.is_some();
        if let Some(sel) = &s.guitar {
            let host = host_by_name(sel.host.as_deref())?;
            let dev = find_input(&host, &sel.device)?;
            let cfg = dev.default_input_config()?;
            let in_rate = cfg.sample_rate();
            summary.push(format!("Guitar: {} @ {} Hz", sel.device, in_rate));

            let sh = shared.clone();
            let mut gate = NoiseGate::new(in_rate);
            let mut rs_main = Resampler::new(in_rate, out_rate);
            let mut rs_mon = mon.as_ref().map(|(_, c)| Resampler::new(in_rate, c.sample_rate()));
            let mut prod_main = g_main_p;
            let mut prod_mon = g_mon_p;
            let mut tmp: Vec<f32> = Vec::with_capacity(16_384);
            let mut peak = Peak::default();

            let on_mono = move |mono: &mut [f32]| {
                let gain = if sh.guitar_muted.load(Ordering::Relaxed) {
                    0.0
                } else {
                    db_to_lin(sh.guitar_gain_db.get())
                };
                for x in mono.iter_mut() {
                    *x *= gain;
                }
                gate.process(mono, sh.guitar_gate_db.get());
                // VST3 chain will process `mono` here (see vst.rs / roadmap).
                peak.feed(mono);
                sh.guitar_level.set(peak.value);

                tmp.clear();
                rs_main.process(mono, &mut tmp);
                prod_main.push_slice(&tmp);
                if let (Some(rs), Some(p)) = (rs_mon.as_mut(), prod_mon.as_mut()) {
                    tmp.clear();
                    rs.process(mono, &mut tmp);
                    p.push_slice(&tmp);
                }
            };
            streams.push(build_input(&dev, &cfg, sel.channel, on_mono, shared.clone())?);
        }

        // ---- mic input ----
        let mic_active = s.mic.is_some();
        if let Some(sel) = &s.mic {
            let host = host_by_name(sel.host.as_deref())?;
            let dev = find_input(&host, &sel.device)?;
            let cfg = dev.default_input_config()?;
            let in_rate = cfg.sample_rate();
            summary.push(format!("Mic: {} @ {} Hz", sel.device, in_rate));

            let sh = shared.clone();
            let mut gate = NoiseGate::new(in_rate);
            let mut rs = Resampler::new(in_rate, out_rate);
            let mut prod = m_main_p;
            let mut tmp: Vec<f32> = Vec::with_capacity(16_384);
            let mut peak = Peak::default();

            let on_mono = move |mono: &mut [f32]| {
                let gain = if sh.mic_muted.load(Ordering::Relaxed) {
                    0.0
                } else {
                    db_to_lin(sh.mic_gain_db.get())
                };
                for x in mono.iter_mut() {
                    *x *= gain;
                }
                gate.process(mono, sh.mic_gate_db.get());
                peak.feed(mono);
                sh.mic_level.set(peak.value);
                tmp.clear();
                rs.process(mono, &mut tmp);
                prod.push_slice(&tmp);
            };
            streams.push(build_input(&dev, &cfg, sel.channel, on_mono, shared.clone())?);
        }

        // ---- main output stream: guitar + mic ----
        {
            let sh = shared.clone();
            let mut g = g_main_c;
            let mut m = m_main_c;
            let target = (out_rate as f32 * TARGET_LATENCY) as usize;
            let max = (out_rate as f32 * MAX_LATENCY) as usize;
            let mut peak = Peak::default();
            let mut primed_g = false;
            let mut primed_m = false;

            let fill = move |buf: &mut [f32], channels: usize| {
                let frames = buf.len() / channels;
                let master = db_to_lin(sh.master_gain_db.get());
                drift_control(&mut g, &mut primed_g, target, max, frames);
                drift_control(&mut m, &mut primed_m, target, max, frames);
                let mut short = false;
                let mut p = 0f32;
                for f in 0..frames {
                    let gs = if guitar_active && primed_g { g.try_pop() } else { Some(0.0) };
                    let ms = if mic_active && primed_m { m.try_pop() } else { Some(0.0) };
                    short |= (guitar_active && primed_g && gs.is_none()) || (mic_active && primed_m && ms.is_none());
                    let v = soft_clip((gs.unwrap_or(0.0) + ms.unwrap_or(0.0)) * master);
                    p = p.max(v.abs());
                    for c in 0..channels {
                        buf[f * channels + c] = v;
                    }
                }
                if short {
                    sh.underruns.fetch_add(1, Ordering::Relaxed);
                }
                peak.value = if p > peak.value { p } else { peak.value * 0.92 };
                sh.out_level.set(peak.value);
            };
            streams.push(build_output(&out_dev, &out_cfg, fill, shared.clone())?);
        }

        // ---- monitor output stream: guitar only ----
        if let (Some((dev, cfg)), Some(mut c)) = (mon, g_mon_c) {
            let sh = shared.clone();
            let rate = cfg.sample_rate();
            let target = (rate as f32 * TARGET_LATENCY) as usize;
            let max = (rate as f32 * MAX_LATENCY) as usize;
            let mut primed = false;
            let fill = move |buf: &mut [f32], channels: usize| {
                let frames = buf.len() / channels;
                let gain = db_to_lin(sh.monitor_gain_db.get());
                drift_control(&mut c, &mut primed, target, max, frames);
                for f in 0..frames {
                    let v = if primed { c.try_pop().unwrap_or(0.0) } else { 0.0 };
                    let v = soft_clip(v * gain);
                    for ch in 0..channels {
                        buf[f * channels + ch] = v;
                    }
                }
            };
            streams.push(build_output(&dev, &cfg, fill, shared.clone())?);
        }

        for st in &streams {
            st.play().map_err(|e| anyhow!("could not start stream: {e}"))?;
        }

        if let Ok(mut e) = shared.last_error.lock() {
            *e = None;
        }
        Ok(Engine {
            _streams: streams,
            summary: summary.join("  ·  "),
        })
    }
}

/// Keep the buffered amount near `target`: wait for it before starting,
/// and drop the excess when the input clock runs faster than the output.
fn drift_control(c: &mut HeapCons<f32>, primed: &mut bool, target: usize, max: usize, frames: usize) {
    let have = c.occupied_len();
    if !*primed {
        if have >= target + frames {
            *primed = true;
        }
        return;
    }
    if have > max + frames {
        c.skip(have - target - frames);
    }
    if have == 0 {
        // Input stalled: re-prime so we do not crackle sample by sample.
        *primed = false;
    }
}

// ---------------------------------------------------------------------------
// Generic stream builders (one per sample format)
// ---------------------------------------------------------------------------

fn build_input<F>(
    dev: &Device,
    cfg: &cpal::SupportedStreamConfig,
    chan: ChannelSel,
    on_mono: F,
    shared: Arc<Shared>,
) -> Result<Stream>
where
    F: FnMut(&mut [f32]) + Send + 'static,
{
    let config: StreamConfig = cfg.config();
    match cfg.sample_format() {
        SampleFormat::F32 => input_typed::<f32, F>(dev, config, chan, on_mono, shared),
        SampleFormat::I16 => input_typed::<i16, F>(dev, config, chan, on_mono, shared),
        SampleFormat::I32 => input_typed::<i32, F>(dev, config, chan, on_mono, shared),
        SampleFormat::U16 => input_typed::<u16, F>(dev, config, chan, on_mono, shared),
        SampleFormat::F64 => input_typed::<f64, F>(dev, config, chan, on_mono, shared),
        other => bail!("unsupported input sample format {other:?}"),
    }
}

fn input_typed<T, F>(
    dev: &Device,
    config: StreamConfig,
    chan: ChannelSel,
    mut on_mono: F,
    shared: Arc<Shared>,
) -> Result<Stream>
where
    T: SizedSample,
    f32: FromSample<T>,
    F: FnMut(&mut [f32]) + Send + 'static,
{
    let channels = config.channels as usize;
    let mut mono: Vec<f32> = Vec::with_capacity(16_384);
    let pick = match chan {
        ChannelSel::Index(i) if (i as usize) < channels => Some(i as usize),
        _ => None,
    };
    let err_shared = shared.clone();
    let stream = dev
        .build_input_stream::<T, _, _>(
            config,
            move |data: &[T], _| {
                let frames = data.len() / channels;
                mono.clear();
                match pick {
                    Some(ch) => {
                        for f in 0..frames {
                            mono.push(<f32 as FromSample<T>>::from_sample_(data[f * channels + ch]));
                        }
                    }
                    None => {
                        let inv = 1.0 / channels as f32;
                        for f in 0..frames {
                            let mut acc = 0.0;
                            for c in 0..channels {
                                acc += <f32 as FromSample<T>>::from_sample_(data[f * channels + c]);
                            }
                            mono.push(acc * inv);
                        }
                    }
                }
                on_mono(&mut mono);
            },
            move |e| err_shared.report_error(e),
            None,
        )
        .map_err(|e| anyhow!("could not open input: {e}"))?;
    Ok(stream)
}

fn build_output<F>(dev: &Device, cfg: &cpal::SupportedStreamConfig, fill: F, shared: Arc<Shared>) -> Result<Stream>
where
    F: FnMut(&mut [f32], usize) + Send + 'static,
{
    let config: StreamConfig = cfg.config();
    match cfg.sample_format() {
        SampleFormat::F32 => output_typed::<f32, F>(dev, config, fill, shared),
        SampleFormat::I16 => output_typed::<i16, F>(dev, config, fill, shared),
        SampleFormat::I32 => output_typed::<i32, F>(dev, config, fill, shared),
        SampleFormat::U16 => output_typed::<u16, F>(dev, config, fill, shared),
        SampleFormat::F64 => output_typed::<f64, F>(dev, config, fill, shared),
        other => bail!("unsupported output sample format {other:?}"),
    }
}

fn output_typed<T, F>(dev: &Device, config: StreamConfig, mut fill: F, shared: Arc<Shared>) -> Result<Stream>
where
    T: SizedSample + FromSample<f32>,
    F: FnMut(&mut [f32], usize) + Send + 'static,
{
    let channels = config.channels as usize;
    let mut scratch: Vec<f32> = Vec::with_capacity(32_768);
    let err_shared = shared.clone();
    let stream = dev
        .build_output_stream::<T, _, _>(
            config,
            move |data: &mut [T], _| {
                scratch.clear();
                scratch.resize(data.len(), 0.0);
                fill(&mut scratch, channels);
                for (o, s) in data.iter_mut().zip(scratch.iter()) {
                    *o = T::from_sample(*s);
                }
            },
            move |e| err_shared.report_error(e),
            None,
        )
        .map_err(|e| anyhow!("could not open output: {e}"))?;
    Ok(stream)
}
