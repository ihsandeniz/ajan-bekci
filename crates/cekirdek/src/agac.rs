//! Süreç ağacını /proc'tan okur.
//!
//! Bekçi subreaper olduğu için ağacından kopan her öksüz (setsid'li torun dahil) init'e
//! değil bekçiye bağlanır. Bu yüzden "bekçinin altındaki her şey" = /proc'taki ppid
//! zincirini bekçiden aşağı doğru yürümek. Liste her seferinde TAZE okunur: eski bir
//! listeyi tutmak, PID yeniden kullanıldığında başkasının sürecini öldürmek demektir.

use std::collections::HashMap;
use std::fs;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Surec {
    pub pid: i32,
    pub durum: char,
    pub ppid: i32,
}

impl Surec {
    /// Zombi ölmüştür; sinyal işe yaramaz, yalnız `waitpid` ile toplanır.
    pub fn zombi(&self) -> bool {
        self.durum == 'Z' || self.durum == 'X'
    }
}

/// `/proc/<pid>/stat` satırı: `pid (komut adı) durum ppid …`.
/// Komut adı boşluk ve parantez içerebilir → SON `)`'den sonrasını oku.
pub fn stat_ayristir(satir: &str) -> Option<Surec> {
    let ac = satir.find('(')?;
    let kapa = satir.rfind(')')?;
    let pid = satir[..ac].trim().parse().ok()?;
    let mut alanlar = satir.get(kapa + 1..)?.split_whitespace();
    let durum = alanlar.next()?.chars().next()?;
    let ppid = alanlar.next()?.parse().ok()?;
    Some(Surec { pid, durum, ppid })
}

pub fn tum_surecler() -> Vec<Surec> {
    let Ok(dizin) = fs::read_dir("/proc") else { return Vec::new() };
    dizin
        .filter_map(|g| g.ok())
        .filter(|g| g.file_name().to_string_lossy().bytes().all(|b| b.is_ascii_digit()))
        .filter_map(|g| fs::read_to_string(g.path().join("stat")).ok())
        .filter_map(|s| stat_ayristir(&s))
        .collect()
}

/// `kok`'un bütün torunları (kok hariç), ağaç sırasıyla (önce ebeveyn).
pub fn torunlar(kok: i32, surecler: &[Surec]) -> Vec<Surec> {
    let mut cocuklar: HashMap<i32, Vec<Surec>> = HashMap::new();
    for s in surecler {
        cocuklar.entry(s.ppid).or_default().push(*s);
    }
    let mut sonuc = Vec::new();
    let mut sira = vec![kok];
    while let Some(p) = sira.pop() {
        for c in cocuklar.get(&p).into_iter().flatten() {
            if c.pid != kok {
                sonuc.push(*c);
                sira.push(c.pid);
            }
        }
    }
    sonuc
}

/// Canlı (zombi olmayan) torunlar — sinyal gönderilecekler ve "ağaç boş mu" ölçüsü.
pub fn canli_torunlar(kok: i32) -> Vec<i32> {
    torunlar(kok, &tum_surecler()).into_iter().filter(|s| !s.zombi()).map(|s| s.pid).collect()
}

#[cfg(test)]
mod testler {
    use super::*;

    #[test]
    fn stat_basit() {
        let s = stat_ayristir("1234 (python3) S 1200 1234 1234 0 -1").unwrap();
        assert_eq!(s, Surec { pid: 1234, durum: 'S', ppid: 1200 });
    }

    #[test]
    fn stat_adinda_bosluk_ve_parantez() {
        let s = stat_ayristir("77 (a) b (c)) Z 5 77 77").unwrap();
        assert_eq!((s.pid, s.durum, s.ppid), (77, 'Z', 5));
        assert!(s.zombi());
    }

    #[test]
    fn stat_bozuk() {
        assert!(stat_ayristir("").is_none());
        assert!(stat_ayristir("abc (x) S 1").is_none());
        assert!(stat_ayristir("12 (x)").is_none());
    }

    #[test]
    fn torunlar_zinciri_ve_kardesleri_bulur() {
        let s = |pid, ppid| Surec { pid, durum: 'S', ppid };
        // 10 → 11 → 12 ; 10 → 13 ; 99 başka ağaç
        let liste = [s(10, 1), s(11, 10), s(12, 11), s(13, 10), s(99, 1)];
        let mut t: Vec<i32> = torunlar(10, &liste).iter().map(|x| x.pid).collect();
        t.sort();
        assert_eq!(t, vec![11, 12, 13]);
        assert!(torunlar(12, &liste).is_empty());
    }

    #[test]
    fn kendi_surecimizi_okuyabiliyoruz() {
        let ben = std::process::id() as i32;
        assert!(tum_surecler().iter().any(|s| s.pid == ben));
    }
}
