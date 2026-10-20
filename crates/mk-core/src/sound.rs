//! 三段提示音在进程内合成（22050Hz / 8bit / 单声道，与原版资源格式参数一致，但波形自造）。
//!
//! 合成部分是**纯数学、完全可移植**的 —— 三平台跑出逐位相同的 PCM。
//! 只有「把这段 WAV 播出去」需要平台支持，所以这里只定义 [`Player`] 接口，
//! 由 `mk-host` 提供实现（Windows: winmm / macOS: AudioQueue / Linux: ALSA，另有静音降级）。

use std::f64::consts::PI;

pub const RATE: u32 = 22050;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    Open,
    Boom,
    Cheer,
}

impl Kind {
    pub const ALL: [Kind; 3] = [Kind::Open, Kind::Boom, Kind::Cheer];

    fn index(self) -> usize {
        match self {
            Kind::Open => 0,
            Kind::Boom => 1,
            Kind::Cheer => 2,
        }
    }
}

/// 把一段 WAV 播出去。宿主后端实现它；没有音频后端时用 [`SilentPlayer`]。
pub trait Player {
    /// 异步播放；已在播的同类音不打断。`wav` 在调用返回前有效即可。
    fn play(&mut self, wav: &[u8]);
    /// 立刻停掉所有正在播的声音（菜单里关掉声音、退出时调用）。
    fn stop_all(&mut self);
}

/// 静音降级：无声卡、无桌面会话、CI 环境都用它。
pub struct SilentPlayer;

impl Player for SilentPlayer {
    fn play(&mut self, _wav: &[u8]) {}
    fn stop_all(&mut self) {}
}

/// 音频门面：持有合成结果与后端，并记住菜单里的「声音」开关。
pub struct Audio {
    clips: [Vec<u8>; 3],
    player: Box<dyn Player>,
    pub enabled: bool,
}

impl Audio {
    pub fn new(player: Box<dyn Player>) -> Audio {
        Audio {
            clips: [
                wav(&blip(0.045, 1200.0, 0.35)),
                wav(&boom(0.42)),
                wav(&cheer(0.62)),
            ],
            player,
            enabled: true,
        }
    }

    /// 关掉声音时什么都不做。
    pub fn play(&mut self, k: Kind) {
        if !self.enabled {
            return;
        }
        let clip = self.clips[k.index()].clone();
        self.player.play(&clip);
    }

    pub fn stop_all(&mut self) {
        self.player.stop_all();
    }

    /// 供测试直接取合成结果（不经过后端）。
    pub fn clip(&self, k: Kind) -> &[u8] {
        &self.clips[k.index()]
    }
}

// ── 波形 ────────────────────────────────────────────────────────────────────

fn blip(seconds: f64, freq: f64, level: f64) -> Vec<u8> {
    let n = (RATE as f64 * seconds) as usize;
    (0..n)
        .map(|i| {
            let t = i as f64 / RATE as f64;
            let env = (-t * 55.0).exp();
            let v = (2.0 * PI * freq * t).sin() * env * level;
            byte_of(v)
        })
        .collect()
}

fn boom(seconds: f64) -> Vec<u8> {
    let n = (RATE as f64 * seconds) as usize;
    let mut lfsr: u32 = 0xACE1;
    let mut phase = 0.0f64;
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        let t = i as f64 / RATE as f64;
        let env = (-t * 9.0).exp();
        lfsr ^= lfsr << 13;
        lfsr ^= lfsr >> 17;
        lfsr ^= lfsr << 5;
        let noise = (lfsr & 0xFF) as f64 / 127.5 - 1.0;
        phase += 2.0 * PI * (110.0 - 70.0 * t) / RATE as f64;
        let v = (noise * 0.65 + phase.sin() * 0.55) * env;
        out.push(byte_of(v));
    }
    out
}

fn cheer(seconds: f64) -> Vec<u8> {
    const NOTES: [f64; 4] = [523.25, 659.25, 783.99, 1046.5];
    let n = (RATE as f64 * seconds) as usize;
    let slice = seconds / NOTES.len() as f64;
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        let t = i as f64 / RATE as f64;
        let step = ((t / slice) as usize).min(NOTES.len() - 1);
        let local = t - step as f64 * slice;
        let env = (local * 200.0).min(1.0) * (-local * 4.5).exp();
        let f = NOTES[step];
        let v = ((2.0 * PI * f * t).sin() * 0.55
            + (2.0 * PI * f * 2.0 * t).sin() * 0.20
            + (2.0 * PI * f * 3.0 * t).sin() * 0.08)
            * env
            * 0.8;
        out.push(byte_of(v));
    }
    out
}

#[inline]
fn byte_of(v: f64) -> u8 {
    (128.0 + round_even(v.clamp(-1.0, 1.0) * 127.0)) as u8
}

/// 银行家舍入，保证同样输入在不同平台得到相同的采样值。
///
/// **不要**换成 `f64::round` —— 那是「四舍五入远离零」，在 `.5` 上会差 1 个 LSB，
/// 三平台的 PCM 就不再逐位相同了。
fn round_even(x: f64) -> f64 {
    let f = x.floor();
    let d = x - f;
    // 恰好落在 .5 上时舍向偶数，其余按普通四舍五入。
    // 先算「要不要进位」再统一进位，语义与逐个分支等价，但两条分支不会重复。
    let round_up = if d == 0.5 { f % 2.0 != 0.0 } else { d > 0.5 };
    if round_up {
        f + 1.0
    } else {
        f
    }
}

/// 包一个 44 字节的 WAV 头（PCM / 单声道 / 8bit）。
fn wav(pcm: &[u8]) -> Vec<u8> {
    let data_len = pcm.len() as u32;
    let mut buf = Vec::with_capacity(44 + pcm.len());
    buf.extend_from_slice(b"RIFF");
    buf.extend_from_slice(&(36 + data_len).to_le_bytes());
    buf.extend_from_slice(b"WAVE");
    buf.extend_from_slice(b"fmt ");
    buf.extend_from_slice(&16u32.to_le_bytes());
    buf.extend_from_slice(&1u16.to_le_bytes()); // PCM
    buf.extend_from_slice(&1u16.to_le_bytes()); // mono
    buf.extend_from_slice(&RATE.to_le_bytes());
    buf.extend_from_slice(&RATE.to_le_bytes()); // byte rate
    buf.extend_from_slice(&1u16.to_le_bytes()); // block align
    buf.extend_from_slice(&8u16.to_le_bytes()); // bits
    buf.extend_from_slice(b"data");
    buf.extend_from_slice(&data_len.to_le_bytes());
    buf.extend_from_slice(pcm);
    buf
}

#[cfg(test)]
mod tests {
    use super::*;

    /// WAV 头必须是 44 字节且各字段自洽。
    #[test]
    fn wav_header_is_well_formed() {
        let a = Audio::new(Box::new(SilentPlayer));
        for k in Kind::ALL {
            let c = a.clip(k);
            assert!(c.len() > 44);
            assert_eq!(&c[0..4], b"RIFF");
            assert_eq!(&c[8..12], b"WAVE");
            assert_eq!(&c[36..40], b"data");
            let riff_len = u32::from_le_bytes(c[4..8].try_into().unwrap());
            assert_eq!(riff_len as usize, c.len() - 8);
            let data_len = u32::from_le_bytes(c[40..44].try_into().unwrap());
            assert_eq!(data_len as usize, c.len() - 44);
        }
    }

    /// 银行家舍入：`.5` 必须舍向偶数，这是跨平台逐位一致的保证。
    #[test]
    fn round_even_is_bankers() {
        assert_eq!(round_even(0.5), 0.0);
        assert_eq!(round_even(1.5), 2.0);
        assert_eq!(round_even(2.5), 2.0);
        assert_eq!(round_even(-0.5), 0.0);
        assert_eq!(round_even(-1.5), -2.0);
    }

    /// 关掉声音后后端不应被调用。
    #[test]
    fn disabled_audio_does_not_reach_the_backend() {
        struct Counter(std::rc::Rc<std::cell::Cell<usize>>);
        impl Player for Counter {
            fn play(&mut self, _: &[u8]) {
                self.0.set(self.0.get() + 1);
            }
            fn stop_all(&mut self) {}
        }
        let n = std::rc::Rc::new(std::cell::Cell::new(0));
        let mut a = Audio::new(Box::new(Counter(n.clone())));
        a.enabled = false;
        a.play(Kind::Open);
        assert_eq!(n.get(), 0);
        a.enabled = true;
        a.play(Kind::Open);
        assert_eq!(n.get(), 1);
    }
}
