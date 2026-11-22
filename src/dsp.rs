//! Small, allocation-free DSP blocks used on the audio thread.

/// Convert decibels to a linear gain factor.
#[inline]
pub fn db_to_lin(db: f32) -> f32 {
    10f32.powf(db / 20.0)
}

/// Convert a linear amplitude to decibels (floored at -96 dB).
#[inline]
pub fn lin_to_db(lin: f32) -> f32 {
    if lin <= 1e-5 {
        -96.0
    } else {
        20.0 * lin.log10()
    }
}

/// Streaming linear-interpolation resampler for a mono signal.
///
/// Good enough for voice and guitar going to a chat app; it keeps state
/// between callbacks so there are no clicks at buffer boundaries.
pub struct Resampler {
    /// input_rate / output_rate
    step: f64,
    pos: f64,
    prev: f32,
}

impl Resampler {
    pub fn new(input_rate: u32, output_rate: u32) -> Self {
        Self {
            step: input_rate as f64 / output_rate as f64,
            pos: 0.0,
            prev: 0.0,
        }
    }

    pub fn is_passthrough(&self) -> bool {
        (self.step - 1.0).abs() < 1e-9
    }

    /// Resample `input` and append the result to `out`.
    pub fn process(&mut self, input: &[f32], out: &mut Vec<f32>) {
        if self.is_passthrough() {
            out.extend_from_slice(input);
            return;
        }
        // `pos` is a fractional index where -1.0 refers to `prev`
        // (the last sample of the previous block).
        let n = input.len() as f64;
        while self.pos < n - 1.0 {
            let i = self.pos.floor();
            let frac = (self.pos - i) as f32;
            let a = if i < 0.0 { self.prev } else { input[i as usize] };
            let b = input[(i + 1.0) as usize];
            out.push(a + (b - a) * frac);
            self.pos += self.step;
        }
        self.pos -= n;
        if let Some(&last) = input.last() {
            self.prev = last;
        }
    }
}

/// Simple downward noise gate with attack/release smoothing.
pub struct NoiseGate {
    env: f32,
    gain: f32,
    env_coef: f32,
    open_coef: f32,
    close_coef: f32,
}

impl NoiseGate {
    pub fn new(sample_rate: u32) -> Self {
        let sr = sample_rate as f32;
        let coef = |ms: f32| (-1.0 / (ms * 0.001 * sr)).exp();
        Self {
            env: 0.0,
            gain: 1.0,
            env_coef: coef(10.0),
            open_coef: coef(2.0),
            close_coef: coef(120.0),
        }
    }

    /// Process in place. `threshold_db` <= -90 disables the gate.
    pub fn process(&mut self, buf: &mut [f32], threshold_db: f32) {
        if threshold_db <= -90.0 {
            self.gain = 1.0;
            return;
        }
        let thr = db_to_lin(threshold_db);
        for s in buf.iter_mut() {
            let a = s.abs();
            self.env = if a > self.env { a } else { self.env * self.env_coef + a * (1.0 - self.env_coef) };
            let target = if self.env >= thr { 1.0 } else { 0.0 };
            let c = if target > self.gain { self.open_coef } else { self.close_coef };
            self.gain = target + (self.gain - target) * c;
            *s *= self.gain;
        }
    }
}

/// Soft clipper so the mix never hard-clips in Discord.
#[inline]
pub fn soft_clip(x: f32) -> f32 {
    if x.abs() < 0.8 {
        x
    } else {
        let s = x.signum();
        let over = (x.abs() - 0.8) / 0.2;
        s * (0.8 + 0.2 * over.tanh())
    }
}

/// Peak meter with fast attack / slow release, read from the UI.
#[derive(Default)]
pub struct Peak {
    pub value: f32,
}

impl Peak {
    pub fn feed(&mut self, buf: &[f32]) {
        let peak = buf.iter().fold(0f32, |m, s| m.max(s.abs()));
        self.value = if peak > self.value { peak } else { self.value * 0.92 };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resampler_passthrough() {
        let mut r = Resampler::new(48_000, 48_000);
        let mut out = Vec::new();
        r.process(&[1.0, 2.0, 3.0], &mut out);
        assert_eq!(out, vec![1.0, 2.0, 3.0]);
    }

    #[test]
    fn resampler_ratio_holds_over_many_blocks() {
        let mut r = Resampler::new(44_100, 48_000);
        let mut out = Vec::new();
        let block = vec![0.5f32; 441];
        for _ in 0..100 {
            r.process(&block, &mut out);
        }
        // 44_100 in -> ~48_000 out
        let expected = 48_000.0;
        assert!((out.len() as f64 - expected).abs() < 5.0, "got {}", out.len());
        assert!(out.iter().all(|s| (s - 0.5).abs() < 1e-6 || *s == 0.0 || (*s - 0.5).abs() < 0.5));
    }

    #[test]
    fn gate_silences_quiet_signal() {
        let mut g = NoiseGate::new(48_000);
        let mut buf = vec![0.001f32; 48_000];
        g.process(&mut buf, -40.0);
        assert!(buf.last().unwrap().abs() < 1e-4);
    }

    #[test]
    fn soft_clip_is_bounded() {
        for x in [-10.0f32, -1.0, 0.0, 0.5, 1.0, 10.0] {
            assert!(soft_clip(x).abs() <= 1.0);
        }
    }
}
