//! 音频输出后端。
//!
//! 用 `cpal` 而不是 `rodio`：这个项目的音频是**进程内合成的原始 PCM**，
//! 不需要任何解码器，而 `rodio` 的默认 features 会拉进 `symphonia`
//! （mp3/aac/flac/vorbis 解码器），那是好几 MB 的无用代码。
//!
//! `mk-core` 负责把 8bit / 22050Hz 单声道的 PCM 合成好并包成 WAV；
//! 这里只做「拆掉 WAV 头、把采样喂给声卡」。

use mk_core::sound::Player;

/// 打开默认音频后端。
///
/// 没有音频能力时（`--no-default-features`，交叉编译到 Linux 的常见做法）返回静音
/// 实现 —— 游戏照常运行，只是没有声音。调用方不需要区分两种实现。
pub fn open_default() -> Box<dyn Player> {
    #[cfg(feature = "audio")]
    {
        Box::new(CpalPlayer::new())
    }
    #[cfg(not(feature = "audio"))]
    {
        Box::new(mk_core::sound::SilentPlayer)
    }
}

/// 每个**输出帧**应消耗多少个源采样。
///
/// 22050 → 44100 要插值，每帧只前进 0.5 个源采样；反过来才是 2.0。
/// 早期写成 `out_rate / RATE`，方向恰好颠倒：44.1kHz 设备上音效会快一倍、高八度。
fn source_step(out_rate: u32) -> f64 {
    mk_core::sound::RATE as f64 / out_rate.max(1) as f64
}

#[cfg(feature = "audio")]
mod cpal_backend {
    use super::decode_pcm8;
    use std::collections::VecDeque;
    use std::sync::{Arc, Mutex};

    use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

    use mk_core::sound::Player;

    /// 最多缓存的采样数，防止连续点击时无限堆积。
    const MAX_QUEUED: usize = 22050 * 2;

    struct Shared {
        queue: VecDeque<f32>,
        /// 源采样率固定 22050，输出设备可能是 44100/48000，这里做线性重采样。
        ratio: f64,
        pos: f64,
    }

    /// 基于 `cpal` 的播放后端。设备不可用时静默降级（`stream` 为 `None`）。
    pub struct CpalPlayer {
        shared: Arc<Mutex<Shared>>,
        /// `cpal::Stream` 在部分平台上不是 `Send`，必须留在创建它的线程里。
        _stream: Option<cpal::Stream>,
    }

    impl CpalPlayer {
        /// 打开默认输出设备。任何一步失败都返回一个静音实例，不 panic。
        pub fn new() -> CpalPlayer {
            let shared = Arc::new(Mutex::new(Shared {
                queue: VecDeque::new(),
                ratio: 1.0,
                pos: 0.0,
            }));
            let stream = Self::build(&shared);
            CpalPlayer {
                shared,
                _stream: stream,
            }
        }

        fn build(shared: &Arc<Mutex<Shared>>) -> Option<cpal::Stream> {
            let host = cpal::default_host();
            let device = host.default_output_device()?;
            let config = device.default_output_config().ok()?;
            let out_rate = config.sample_rate().0 as f64;
            let channels = config.channels() as usize;

            {
                let mut s = shared.lock().ok()?;
                s.ratio = super::source_step(out_rate as u32);
            }

            let shared = shared.clone();
            let stream = device
                .build_output_stream(
                    &config.into(),
                    move |out: &mut [f32], _| {
                        let Ok(mut s) = shared.lock() else {
                            out.fill(0.0);
                            return;
                        };
                        // 队列空时**不要**推进 `pos`：早期写法会让它一直涨，
                        // 等下一段音效入队时 `pos` 早已越界，整段被当成静音消费掉 ——
                        // 表现就是「闲置一会儿之后第一次音效没声音」。
                        if s.queue.is_empty() {
                            out.fill(0.0);
                            s.pos = 0.0;
                            return;
                        }
                        for frame in out.chunks_mut(channels) {
                            if s.queue.is_empty() {
                                s.pos = 0.0;
                                for sample in frame.iter_mut() {
                                    *sample = 0.0;
                                }
                                continue;
                            }
                            // 线性重采样：按 ratio 推进源位置。
                            let idx = s.pos.floor() as usize;
                            let frac = (s.pos - idx as f64) as f32;
                            let a = s.queue.get(idx).copied().unwrap_or(0.0);
                            let b = s.queue.get(idx + 1).copied().unwrap_or(a);
                            let v = a + (b - a) * frac;
                            for sample in frame.iter_mut() {
                                *sample = v;
                            }
                            s.pos += s.ratio;
                            // 播完就把已经走过的采样丢掉。
                            while s.pos >= 1.0 && !s.queue.is_empty() {
                                s.queue.pop_front();
                                s.pos -= 1.0;
                            }
                        }
                    },
                    |_| {},
                    None,
                )
                .ok()?;
            let _ = stream.play();
            Some(stream)
        }
    }

    impl Default for CpalPlayer {
        fn default() -> Self {
            Self::new()
        }
    }

    impl Player for CpalPlayer {
        fn play(&mut self, wav: &[u8]) {
            let Some(samples) = decode_pcm8(wav) else {
                return;
            };
            let Ok(mut s) = self.shared.lock() else {
                return;
            };
            if s.queue.len() + samples.len() > MAX_QUEUED {
                // 队列太满就丢掉旧数据，宁可截断也不要延迟累积。
                let drop = (s.queue.len() + samples.len()).saturating_sub(MAX_QUEUED);
                for _ in 0..drop.min(s.queue.len()) {
                    s.queue.pop_front();
                }
            }
            s.queue.extend(samples);
        }

        fn stop_all(&mut self) {
            if let Ok(mut s) = self.shared.lock() {
                s.queue.clear();
                s.pos = 0.0;
            }
        }
    }
} // mod cpal_backend

#[cfg(feature = "audio")]
pub use cpal_backend::CpalPlayer;

/// 拆掉 44 字节的 WAV 头，把 8bit 无符号 PCM 转成 `[-1, 1]` 的 f32。
fn decode_pcm8(wav: &[u8]) -> Option<Vec<f32>> {
    if wav.len() < 44 || &wav[0..4] != b"RIFF" || &wav[36..40] != b"data" {
        return None;
    }
    let data_len = u32::from_le_bytes(wav[40..44].try_into().ok()?) as usize;
    let body = wav.get(44..44 + data_len)?;
    Some(body.iter().map(|&b| (b as f32 - 128.0) / 127.0).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 重采样比例的方向：输出率更高时，每帧应消耗**更少**的源采样。
    #[test]
    fn source_step_direction() {
        assert!((source_step(mk_core::sound::RATE) - 1.0).abs() < 1e-9);
        assert!((source_step(44100) - 0.5).abs() < 1e-9);
        assert!(source_step(48000) < 0.5);
        // 反向输出率（如 11025）时每帧消耗 2 个源采样
        assert!((source_step(11025) - 2.0).abs() < 1e-9);
    }

    #[test]
    fn decode_rejects_non_wav() {
        assert!(decode_pcm8(b"not a wav at all, definitely not").is_none());
        assert!(decode_pcm8(&[]).is_none());
    }

    /// 用真实的合成产物测解码，避免手工拼一个对不上的头。
    #[test]
    fn decode_maps_8bit_to_unit_range() {
        use mk_core::sound::{Audio, Kind, SilentPlayer};
        let a = Audio::new(Box::new(SilentPlayer));
        let wav = a.clip(Kind::Open);
        let s = decode_pcm8(wav).unwrap();
        assert_eq!(s.len(), wav.len() - 44);
        assert!(s.iter().all(|v| (-1.0..=1.0).contains(v)));
        // 8bit 无符号的中心值是 128 → 0.0。
        assert!(s.iter().any(|v| v.abs() < 1e-6), "应存在静音采样");
        assert!(s.iter().any(|&v| v > 0.3), "应存在正峰");
    }

    /// 数据段长度超出实际字节时必须拒绝，不能越界读。
    #[test]
    fn decode_rejects_truncated_data() {
        use mk_core::sound::{Audio, Kind, SilentPlayer};
        let a = Audio::new(Box::new(SilentPlayer));
        let mut wav = a.clip(Kind::Boom).to_vec();
        wav.truncate(50);
        assert!(decode_pcm8(&wav).is_none());
    }
}
