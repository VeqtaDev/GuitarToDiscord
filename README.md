# Guitar To Discord

Plug your guitar into your sound card, keep your headset mic, and send **both** to Discord as a single microphone.

Guitar To Discord is a small, open-source Windows app written in Rust. It captures your audio interface (Focusrite, Scarlett, Audient, MOTU…) and your usual mic, mixes them in real time, and plays the mix into a virtual audio cable that Discord (or OBS, Teams, TeamSpeak…) uses as its input device.

**Website:** https://veqtadev.github.io/GuitarToDiscord/

## Features

- Guitar + voice mix, with separate gain, mute and noise gate per source
- Pick the input channel (Input 1 / Input 2 of your interface, or all channels to mono)
- Automatic resampling (44.1 kHz ↔ 48 kHz) and clock-drift correction
- Soft clipper on the output, so Discord never gets a clipped signal
- Headphone monitoring: hear your guitar only, your voice only, or both (switch live)
- Auto-detects your audio interface, headset and virtual cable
- WASAPI by default, native ASIO with a feature flag
- VST3 hosting on the guitar: amp sims and effects run live, with their own editor windows
- Plugin chain with on/off, reordering and saved plugin settings
- Each plugin is test-loaded in a separate process first, so a crashing plugin cannot take the app down
- Black liquid-glass UI, settings saved in `%APPDATA%\GuitarToDiscord\config.json`

## Setup

1. Install **[VB-Cable](https://vb-audio.com/Cable/)** (free). It adds a playback device `CABLE Input` and a recording device `CABLE Output`.
2. Optional: in *Control Panel › Sound › Recording*, open `CABLE Output` › Properties and rename it to `Guitar To Discord`. (The app has a button that opens this panel.)
3. Run Guitar To Discord. Pick your sound card (guitar), your mic, and `CABLE Input` as the output. Click **Démarrer**.
   To hear yourself, turn on **Retour casque**, pick your headphones and choose *Guitare*, *Voix* or *Les deux*.
4. In Discord: *Settings › Voice & Video › Input Device* › `CABLE Output` (or `Guitar To Discord`).
5. In Discord, turn off **Krisp / noise suppression** and **echo cancellation**, or your guitar will be cut out.

Voicemeeter users can also pick a Voicemeeter input as the output; the app shows which recording device to select in Discord.

### Why a virtual cable?

A new input device on Windows needs a signed kernel-mode audio driver. VB-Cable is free and reliable, so the app plays into it rather than shipping its own driver. A dedicated "Guitar To Discord" driver is on the roadmap.

## Build

```powershell
cargo build --release
```

The binary is `target\release\guitar-to-discord.exe`.

### ASIO

WASAPI works with every Focusrite interface. For the lowest latency with ASIO:

1. Download the [Steinberg ASIO SDK](https://www.steinberg.net/developers/) and set `CPAL_ASIO_DIR` to its folder.
2. Install LLVM (for `bindgen`) and set `LIBCLANG_PATH` if needed.
3. `cargo build --release --features asio`
4. Choose **ASIO** in the *Pilote* menu of the Guitar card.

### Self-test

```powershell
cargo run -- --selftest
```

Starts the engine without a window for 3 seconds and prints the levels.

## Roadmap

- [x] Guitar + mic mix into a virtual cable
- [x] Gain, mute, noise gate
- [x] Headphone monitoring: guitar, voice or both
- [x] VST3 scan and chain editor
- [x] VST3 processing on the guitar signal
- [x] Plugin editors (GUI) and saved plugin settings
- [ ] Own virtual audio driver ("Guitar To Discord" device, no VB-Cable needed)
- [ ] System tray mode

## Project layout

| Path | Role |
| --- | --- |
| `src/audio.rs` | Device discovery, capture/playback streams, mixer |
| `src/dsp.rs` | Resampler, noise gate, soft clip, meters |
| `src/vst.rs` | VST3 discovery |
| `src/vst_host.rs` | VST3 hosting: loading, processing, editors, state |
| `src/theme.rs` | Liquid-glass egui widgets |
| `src/app.rs` | Application UI |
| `docs/` | Website (deployed to GitHub Pages) |

## License

MIT. VB-Cable is a product of VB-Audio Software and is not bundled.
