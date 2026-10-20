//! 最佳时间持久化。
//!
//! 原版写在 `HKCU\Software\Microsoft\winmine` 的 `Time1..3` / `Name1..3`；
//! 这里写本地 JSON，不碰注册表、不占用微软的键名。
//!
//! 路径用各平台的标准缓存目录：
//! Windows `%LOCALAPPDATA%\MineKeeper\scores.json`、
//! macOS `~/Library/Caches/MineKeeper/scores.json`、
//! Linux `~/.cache/MineKeeper/scores.json`。

use std::path::{Path, PathBuf};

use crate::game::Kind;

/// 「还没有纪录」的秒数哨兵，与数码管上限一致。
pub const NO_RECORD: i32 = 999;

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Entry {
    pub seconds: i32,
    pub name: String,
}

impl Default for Entry {
    fn default() -> Self {
        Entry {
            seconds: NO_RECORD,
            name: "匿名".to_string(),
        }
    }
}

impl Entry {
    pub fn has_record(&self) -> bool {
        self.seconds < NO_RECORD && !self.name.trim().is_empty()
    }
}

/// 固定字段顺序，保证写出的 JSON 稳定、可读。
#[derive(Default, serde::Serialize, serde::Deserialize)]
struct Dto {
    #[serde(default)]
    beginner: Entry,
    #[serde(default)]
    intermediate: Entry,
    #[serde(default)]
    expert: Entry,
}

pub struct Store {
    best: [Entry; 3],
    pub path: PathBuf,
}

impl Store {
    fn slot(k: Kind) -> Option<usize> {
        match k {
            Kind::Beginner => Some(0),
            Kind::Intermediate => Some(1),
            Kind::Expert => Some(2),
            Kind::Custom => None,
        }
    }

    /// 默认路径：`<cache>/MineKeeper/scores.json`。
    pub fn default_path() -> PathBuf {
        let base = dirs_cache_dir().unwrap_or_else(std::env::temp_dir);
        base.join("MineKeeper").join("scores.json")
    }

    /// 读纪录；读不到或格式不对就退回默认值，不打扰玩家。
    pub fn load(path: impl AsRef<Path>) -> Store {
        let path = path.as_ref().to_path_buf();
        let mut s = Store {
            best: [Entry::default(), Entry::default(), Entry::default()],
            path,
        };
        let Ok(raw) = std::fs::read(&s.path) else {
            return s;
        };
        let Ok(d) = serde_json::from_slice::<Dto>(&raw) else {
            return s;
        };
        s.best = [
            clamp_entry(d.beginner),
            clamp_entry(d.intermediate),
            clamp_entry(d.expert),
        ];
        s
    }

    pub fn load_default() -> Store {
        Store::load(Self::default_path())
    }

    pub fn get(&self, k: Kind) -> Entry {
        Self::slot(k)
            .map(|i| self.best[i].clone())
            .unwrap_or_default()
    }

    /// 判断这个秒数是否破了该档纪录。自定义档不记纪录。
    pub fn is_record(&self, k: Kind, seconds: i32) -> bool {
        match Self::slot(k) {
            Some(i) => seconds < self.best[i].seconds,
            None => false,
        }
    }

    /// 写入一条纪录并落盘。
    pub fn record(&mut self, k: Kind, seconds: i32, name: &str) {
        let Some(i) = Self::slot(k) else { return };
        let mut name = name.trim().to_string();
        if name.is_empty() {
            name = "匿名".to_string();
        }
        self.best[i] = Entry {
            seconds: seconds.clamp(0, NO_RECORD),
            name,
        };
        self.save();
    }

    /// 清空全部纪录。
    pub fn clear(&mut self) {
        self.best = [Entry::default(), Entry::default(), Entry::default()];
        self.save();
    }

    /// 落盘；目录只读等情况静默降级为不持久化。
    pub fn save(&self) {
        let Some(dir) = self.path.parent() else {
            return;
        };
        if std::fs::create_dir_all(dir).is_err() {
            return;
        }
        let dto = Dto {
            beginner: self.best[0].clone(),
            intermediate: self.best[1].clone(),
            expert: self.best[2].clone(),
        };
        if let Ok(raw) = serde_json::to_vec_pretty(&dto) {
            let _ = std::fs::write(&self.path, raw);
        }
    }
}

fn clamp_entry(mut e: Entry) -> Entry {
    e.seconds = e.seconds.clamp(0, NO_RECORD);
    if e.name.trim().is_empty() {
        e.name = "匿名".to_string();
    }
    e
}

/// 各平台的标准缓存目录。只用标准库，不引入 `dirs` crate。
fn dirs_cache_dir() -> Option<PathBuf> {
    #[cfg(windows)]
    {
        std::env::var_os("LOCALAPPDATA").map(PathBuf::from)
    }
    #[cfg(target_os = "macos")]
    {
        std::env::var_os("HOME").map(|h| PathBuf::from(h).join("Library").join("Caches"))
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        std::env::var_os("XDG_CACHE_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".cache")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 每个测试用独立目录 —— 用 `process::id()` 会让并行跑的测试互相清空对方的文件。
    fn temp_store() -> Store {
        use std::sync::atomic::{AtomicU32, Ordering};
        static N: AtomicU32 = AtomicU32::new(0);
        let n = N.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("mk-test-{}-{}", std::process::id(), n));
        let _ = std::fs::remove_dir_all(&dir);
        Store::load(dir.join("scores.json"))
    }

    #[test]
    fn record_round_trips_through_disk() {
        let mut s = temp_store();
        assert!(s.is_record(Kind::Expert, 100));
        s.record(Kind::Expert, 100, " 张三 ");
        assert_eq!(s.get(Kind::Expert).seconds, 100);
        assert_eq!(s.get(Kind::Expert).name, "张三");
        assert!(!s.is_record(Kind::Expert, 120));

        let reloaded = Store::load(&s.path);
        assert_eq!(reloaded.get(Kind::Expert).name, "张三");
        let _ = std::fs::remove_dir_all(s.path.parent().unwrap());
    }

    #[test]
    fn custom_difficulty_never_records() {
        let mut s = temp_store();
        s.record(Kind::Custom, 5, "x");
        assert!(!s.is_record(Kind::Custom, 1));
        assert_eq!(s.get(Kind::Custom), Entry::default());
    }

    #[test]
    fn empty_name_falls_back_to_anonymous() {
        let mut s = temp_store();
        s.record(Kind::Beginner, 10, "   ");
        assert_eq!(s.get(Kind::Beginner).name, "匿名");
        let _ = std::fs::remove_dir_all(s.path.parent().unwrap());
    }
}
