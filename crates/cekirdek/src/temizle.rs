//! `tara --temizle` — K1/K2 kalıntılarını kapatır (güvenlik kemeri: sinyalden hemen önce yeniden doğrulama).
//!
//! Kemer, sırayla:
//! 1. **Plan** yalnız K1/K2'den, yalnız kendi uid'imizden kurulur. K3 asla girmez. Beyaz listedekiler
//!    ve canlı bir ajanın (claude, codex, ajan-bekci) ağacındakiler plan dışı kalır.
//! 2. Plan **kuru liste** olarak önce basılır; `--kuru` ise burada durulur (sinyal yok).
//! 3. Uygulama yalnız o listeyi işler. Her sinyalden HEMEN önce:
//!    - taze anlık görüntüde süreç hâlâ K1/K2 ve korunmuyor mu,
//!    - uid + başlangıç anı + exe listelendiği anla aynı mı (PID başkasına geçmiş olabilir).
//!    Biri tutmazsa sinyal yok, "atlandı" yazılır.
//! 4. SIGTERM → `bekle` → hâlâ yaşayan ve yeniden doğrulanan → SIGKILL.
//!
//! Karar mantığı `Sistem` üzerinden yürür: testler sahte bir sistemle PID yeniden kullanımını,
//! arada değişen kanıtı ve beyaz listeyi gerçek süreç öldürmeden sınar.

use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

use crate::dil::m;
use crate::tara::{Bilgi, Bulgu};

/// Plandaki bir hedef: listelendiği andaki kimliğiyle.
#[derive(Debug, Clone, PartialEq)]
pub struct Hedef {
    pub pid: i32,
    pub seviye: &'static str,
    pub baslangic: u64,
    pub exe: Option<String>,
    pub komut: String,
    pub ayrinti: String,
}

/// Sinyalden hemen önce okunan kimlik. Süreç yok ya da zombi → `None`.
#[derive(Debug, Clone, PartialEq)]
pub struct Kimlik {
    pub uid: u32,
    pub baslangic: u64,
    pub exe: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Sonuc {
    Kapandi(&'static str),
    Atlandi(String),
    Kaldi,
}

/// Beyaz liste: satır başına `exe <tam yol>` ya da `komut <alt dizgi>`; `#` yorum.
#[derive(Debug, Clone, Default)]
pub struct BeyazListe {
    exe: Vec<String>,
    komut: Vec<String>,
}

impl BeyazListe {
    pub fn ayristir(metin: &str) -> Result<Self, String> {
        let mut b = Self::default();
        for (n, satir) in metin.lines().enumerate() {
            let satir = satir.trim();
            if satir.is_empty() || satir.starts_with('#') {
                continue;
            }
            match satir.split_once(char::is_whitespace) {
                Some(("exe", d)) if !d.trim().is_empty() => b.exe.push(d.trim().to_owned()),
                Some(("komut", d)) if !d.trim().is_empty() => b.komut.push(d.trim().to_owned()),
                _ => {
                    return Err(match crate::dil::simdi() {
                        crate::dil::Dil::En => format!("allow list line {}: expected 'exe <path>' or 'komut <substring>'", n + 1),
                        crate::dil::Dil::Tr => format!("beyaz liste satır {}: 'exe <yol>' ya da 'komut <alt dizgi>' bekleniyor", n + 1),
                    });
                }
            }
        }
        Ok(b)
    }

    pub fn eslesir(&self, exe: Option<&str>, komut: &str) -> bool {
        exe.is_some_and(|e| self.exe.iter().any(|x| x == e)) || self.komut.iter().any(|k| komut.contains(k.as_str()))
    }

    pub fn bos(&self) -> bool {
        self.exe.is_empty() && self.komut.is_empty()
    }
}

/// Canlı ajan süreçleri (claude, codex, ajan-bekci; zombi değil) ve bütün torunları.
pub fn korunanlar(liste: &[Bilgi]) -> HashSet<i32> {
    let ajan = |b: &Bilgi| {
        let ad = |a: &str| matches!(a, "claude" | "codex" | "ajan-bekci");
        b.durum != 'Z' && b.durum != 'X' && (ad(&b.ad) || b.exe_adi.as_deref().is_some_and(ad))
    };
    let mut cocuklar: HashMap<i32, Vec<i32>> = HashMap::new();
    for b in liste {
        cocuklar.entry(b.ppid).or_default().push(b.pid);
    }
    let mut korunan = HashSet::new();
    let mut sira: Vec<i32> = liste.iter().filter(|b| ajan(b)).map(|b| b.pid).collect();
    while let Some(p) = sira.pop() {
        if korunan.insert(p) {
            sira.extend(cocuklar.get(&p).into_iter().flatten());
        }
    }
    korunan
}

/// Plan: K1/K2 → beyaz liste, canlı ajan ağacı ve (`sadece` verildiyse) PID süzgecinden geçer.
/// Dönüş: (hedefler, plan dışı bırakılan K1/K2'ler ve nedenleri).
pub fn planla(
    bulgular: &[Bulgu],
    liste: &[Bilgi],
    beyaz: &BeyazListe,
    sadece: Option<&[i32]>,
) -> (Vec<Hedef>, Vec<(i32, String)>) {
    let korunan = korunanlar(liste);
    let mut hedefler = Vec::new();
    let mut disarida = Vec::new();
    for b in bulgular.iter().filter(|b| matches!(b.kanit.seviye(), "K1" | "K2")) {
        if sadece.is_some_and(|s| !s.contains(&b.pid)) {
            continue; // istenmeyen: sessizce dışarıda
        }
        if korunan.contains(&b.pid) {
            disarida.push((b.pid, m("in a live agent tree", "canlı ajan ağacında").to_owned()));
        } else if beyaz.eslesir(b.exe_yol.as_deref(), &b.komut) {
            disarida.push((b.pid, m("on the allow list", "beyaz listede").to_owned()));
        } else {
            hedefler.push(Hedef {
                pid: b.pid,
                seviye: b.kanit.seviye(),
                baslangic: b.baslangic,
                exe: b.exe_yol.clone(),
                komut: b.komut.clone(),
                ayrinti: b.kanit.ayrinti(),
            });
        }
    }
    (hedefler, disarida)
}

/// Uygulamanın dünyaya baktığı yüz (gerçeği Linux'ta `LinuxSistem`, testte sahte).
pub trait Sistem {
    /// Taze anlık görüntüde hâlâ K1/K2 olan ve korunmayan PID'ler.
    fn gecerli_kanit(&mut self) -> HashSet<i32>;
    /// Sinyalden hemen önce kimlik; süreç yok/zombi → `None`.
    fn kimlik(&mut self, pid: i32) -> Option<Kimlik>;
    fn sinyal(&mut self, pid: i32, oldur: bool);
    fn uyu(&mut self, sure: Duration);
}

/// Hedef hâlâ aynı süreç mi (uid + başlangıç anı + exe).
fn ayni(h: &Hedef, k: &Kimlik, uid: u32) -> Result<(), String> {
    if k.uid != uid {
        return Err(format!("{} ({})", m("uid changed", "uid değişti"), k.uid));
    }
    if k.baslangic != h.baslangic {
        return Err(m("start time changed (PID reused)", "başlangıç anı değişti (PID başkasına geçmiş)").into());
    }
    if k.exe != h.exe {
        return Err(m("exe changed", "exe değişti").into());
    }
    Ok(())
}

/// Planı uygula. Her sinyalden hemen önce kanıt + kimlik yeniden doğrulanır; tutmayan atlanır.
pub fn uygula(plan: &[Hedef], uid: u32, s: &mut dyn Sistem, bekle: Duration) -> Vec<(i32, Sonuc)> {
    let mut sonuc: HashMap<i32, Sonuc> = HashMap::new();
    let yasiyor = |s: &mut dyn Sistem, h: &Hedef| s.kimlik(h.pid).is_some_and(|k| ayni(h, &k, uid).is_ok());

    // 1. SIGTERM
    let kanit = s.gecerli_kanit();
    let mut gonderilen = Vec::new();
    for h in plan {
        if !kanit.contains(&h.pid) {
            sonuc.insert(h.pid, Sonuc::Atlandi(m("evidence gone (no longer K1/K2, or protected)", "kanıt artık yok (K1/K2 değil ya da korunuyor)").into()));
            continue;
        }
        match s.kimlik(h.pid) {
            None => {
                sonuc.insert(h.pid, Sonuc::Atlandi(m("already gone", "zaten kapanmış").into()));
            }
            Some(k) => match ayni(h, &k, uid) {
                Err(e) => {
                    sonuc.insert(h.pid, Sonuc::Atlandi(e));
                }
                Ok(()) => {
                    s.sinyal(h.pid, false);
                    gonderilen.push(h);
                }
            },
        }
    }

    // 2. bekle
    let son = Instant::now() + bekle;
    loop {
        let kalan = gonderilen.iter().filter(|h| yasiyor(s, h)).count();
        if kalan == 0 || Instant::now() >= son {
            break;
        }
        s.uyu(Duration::from_millis(50));
    }

    // 3. SIGKILL — yalnız hâlâ aynı kimlikle yaşayan ve kanıtı süren
    let kanit = s.gecerli_kanit();
    let mut olduruldu = Vec::new();
    for h in gonderilen {
        match s.kimlik(h.pid) {
            None => {
                sonuc.insert(h.pid, Sonuc::Kapandi("SIGTERM"));
            }
            Some(k) => {
                if let Err(e) = ayni(h, &k, uid) {
                    // Eski süreç kapandı, PID başkasına geçti: o yeni sürece dokunma.
                    sonuc.insert(h.pid, Sonuc::Atlandi(format!("{} {e}", m("after SIGTERM", "SIGTERM sonrası"))));
                } else if !kanit.contains(&h.pid) {
                    sonuc.insert(h.pid, Sonuc::Atlandi(m("after SIGTERM evidence gone", "SIGTERM sonrası kanıt artık yok").into()));
                } else {
                    s.sinyal(h.pid, true);
                    olduruldu.push(h);
                }
            }
        }
    }
    let son = Instant::now() + Duration::from_secs(2);
    while olduruldu.iter().any(|h| yasiyor(s, h)) && Instant::now() < son {
        s.uyu(Duration::from_millis(20));
    }
    for h in olduruldu {
        let d = if yasiyor(s, h) { Sonuc::Kaldi } else { Sonuc::Kapandi("SIGKILL") };
        sonuc.insert(h.pid, d);
    }
    plan.iter().map(|h| (h.pid, sonuc.remove(&h.pid).unwrap_or(Sonuc::Kaldi))).collect()
}

/// Gerçek Linux sistemi: /proc + kill(2). Yalnız `uygula`nın doğruladığı PID'lere sinyal gider.
pub struct LinuxSistem {
    pub beyaz: BeyazListe,
}

impl Sistem for LinuxSistem {
    fn gecerli_kanit(&mut self) -> HashSet<i32> {
        let (liste, bulgular, _) = crate::tara::anlik();
        let (hedefler, _) = planla(&bulgular, &liste, &self.beyaz, None);
        hedefler.into_iter().map(|h| h.pid).collect()
    }

    fn kimlik(&mut self, pid: i32) -> Option<Kimlik> {
        let (durum, baslangic) = crate::tara::durum_ve_baslangic(pid)?;
        if durum == 'Z' || durum == 'X' {
            return None;
        }
        let status = std::fs::read_to_string(format!("/proc/{pid}/status")).ok()?;
        let uid = status.lines().find_map(|l| l.strip_prefix("Uid:"))?.split_whitespace().next()?.parse().ok()?;
        Some(Kimlik { uid, baslangic, exe: crate::tara::exe_oku(pid) })
    }

    fn sinyal(&mut self, pid: i32, oldur: bool) {
        use nix::sys::signal::{Signal, kill};
        let s = if oldur { Signal::SIGKILL } else { Signal::SIGTERM };
        let _ = kill(nix::unistd::Pid::from_raw(pid), s);
    }

    fn uyu(&mut self, sure: Duration) {
        std::thread::sleep(sure);
    }
}

#[cfg(test)]
mod testler {
    use super::*;
    use crate::tara::Kanit;

    const BEN: u32 = 1000;

    /// Sahte sistem: süreç tablosu + sinyal günlüğü. SIGTERM'e uyan/uymayan ve sinyal anında
    /// PID'i başkasına geçen süreçler kurulabilir.
    #[derive(Default)]
    struct Sahte {
        surec: HashMap<i32, Kimlik>,
        kanit: HashSet<i32>,
        inatci: HashSet<i32>,
        /// İlk `kimlik` okumasından SONRA (listeleme ile sinyal arası) başkasına geçecek PID'ler.
        degisecek: HashMap<i32, Kimlik>,
        sinyaller: Vec<(i32, bool)>,
    }

    impl Sistem for Sahte {
        fn gecerli_kanit(&mut self) -> HashSet<i32> {
            self.kanit.clone()
        }
        fn kimlik(&mut self, pid: i32) -> Option<Kimlik> {
            if let Some(k) = self.degisecek.remove(&pid) {
                self.surec.insert(pid, k);
            }
            self.surec.get(&pid).cloned()
        }
        fn sinyal(&mut self, pid: i32, oldur: bool) {
            self.sinyaller.push((pid, oldur));
            if oldur || !self.inatci.contains(&pid) {
                self.surec.remove(&pid);
            }
        }
        fn uyu(&mut self, _: Duration) {}
    }

    fn hedef(pid: i32, bas: u64) -> Hedef {
        Hedef { pid, seviye: "K2", baslangic: bas, exe: Some("/usr/bin/sleep".into()), komut: "sleep".into(), ayrinti: String::new() }
    }

    fn kimlik(bas: u64) -> Kimlik {
        Kimlik { uid: BEN, baslangic: bas, exe: Some("/usr/bin/sleep".into()) }
    }

    fn sahte(pidler: &[(i32, u64)]) -> Sahte {
        let mut s = Sahte::default();
        for (p, b) in pidler {
            s.surec.insert(*p, kimlik(*b));
            s.kanit.insert(*p);
        }
        s
    }

    #[test]
    fn dogrulanan_sigterm_inatci_sigkill() {
        let mut s = sahte(&[(10, 1), (11, 2)]);
        s.inatci.insert(11);
        let r = uygula(&[hedef(10, 1), hedef(11, 2)], BEN, &mut s, Duration::ZERO);
        assert_eq!(r, vec![(10, Sonuc::Kapandi("SIGTERM")), (11, Sonuc::Kapandi("SIGKILL"))]);
        assert_eq!(s.sinyaller, vec![(10, false), (11, false), (11, true)]);
    }

    #[test]
    fn pid_yeniden_kullanimi_atlanir_sinyal_yok() {
        // Listelendiğinde başlangıç 1; sinyalden hemen önce aynı PID başlangıç 99 olan başka süreç.
        let mut s = sahte(&[(10, 99)]);
        let r = uygula(&[hedef(10, 1)], BEN, &mut s, Duration::ZERO);
        assert!(matches!(&r[0].1, Sonuc::Atlandi(e) if e.contains("start time")), "{r:?}");
        assert!(s.sinyaller.is_empty());
    }

    #[test]
    fn sigterm_ile_sigkill_arasinda_pid_gecerse_sigkill_yok() {
        let mut s = sahte(&[(10, 1)]);
        s.inatci.insert(10);
        // SIGTERM'den sonra ilk kimlik okumasında 10 artık başka biri (başlangıç 50).
        let r = {
            let plan = [hedef(10, 1)];
            // ilk okuma (SIGTERM öncesi) gerçek kimliği görsün, ikincisi değişmiş olanı
            let mut ara = Ara { ic: s, sayac: 0 };
            let r = uygula(&plan, BEN, &mut ara, Duration::ZERO);
            s = ara.ic;
            r
        };
        assert!(matches!(&r[0].1, Sonuc::Atlandi(e) if e.contains("start time")), "{r:?}");
        assert_eq!(s.sinyaller, vec![(10, false)], "SIGKILL gitmemeli");
    }

    /// İkinci `kimlik` okumasından itibaren PID 10'u başkasına geçirir.
    struct Ara {
        ic: Sahte,
        sayac: u32,
    }
    impl Sistem for Ara {
        fn gecerli_kanit(&mut self) -> HashSet<i32> {
            self.ic.gecerli_kanit()
        }
        fn kimlik(&mut self, pid: i32) -> Option<Kimlik> {
            self.sayac += 1;
            if self.sayac == 2 {
                self.ic.degisecek.insert(pid, kimlik(50));
            }
            self.ic.kimlik(pid)
        }
        fn sinyal(&mut self, pid: i32, oldur: bool) {
            self.ic.sinyal(pid, oldur)
        }
        fn uyu(&mut self, d: Duration) {
            self.ic.uyu(d)
        }
    }

    #[test]
    fn exe_ve_uid_degisirse_atlanir() {
        let mut s = sahte(&[(10, 1), (11, 2)]);
        s.surec.get_mut(&10).unwrap().exe = Some("/usr/bin/firefox".into());
        s.surec.get_mut(&11).unwrap().uid = 0;
        let r = uygula(&[hedef(10, 1), hedef(11, 2)], BEN, &mut s, Duration::ZERO);
        assert!(matches!(&r[0].1, Sonuc::Atlandi(e) if e.contains("exe")), "{r:?}");
        assert!(matches!(&r[1].1, Sonuc::Atlandi(e) if e.contains("uid")), "{r:?}");
        assert!(s.sinyaller.is_empty());
    }

    #[test]
    fn arada_kaniti_kaybolan_ya_da_kapanan_atlanir() {
        let mut s = sahte(&[(10, 1)]);
        s.kanit.clear(); // ör. bu arada canlı bir ajanın ağacına girdi
        let r = uygula(&[hedef(10, 1), hedef(12, 3)], BEN, &mut s, Duration::ZERO);
        assert!(r.iter().all(|(_, d)| matches!(d, Sonuc::Atlandi(_))), "{r:?}");
        assert!(s.sinyaller.is_empty());
    }

    fn bilgi(pid: i32, ppid: i32, ad: &str) -> Bilgi {
        Bilgi { pid, ppid, durum: 'S', uid: BEN, ad: ad.into(), ..Default::default() }
    }

    fn bulgu(pid: i32, kanit: Kanit, komut: &str, exe: &str) -> Bulgu {
        Bulgu { pid, kanit, ajan: "?", yas_sn: 0, rss_kb: 0, komut: komut.into(), baslangic: 5, exe_yol: Some(exe.into()) }
    }

    #[test]
    fn plan_k3_beyaz_ve_canli_agac_disarida() {
        let k2 = || Kanit::K2 { degisken: "CLAUDE_PID", hedef: 4242, neden: "ölü" };
        let liste = vec![bilgi(1, 0, "systemd"), bilgi(800, 1, "claude"), bilgi(801, 800, "bash"), bilgi(802, 801, "sleep"), bilgi(900, 1, "sleep")];
        let bulgular = vec![
            bulgu(802, k2(), "sleep 5", "/usr/bin/sleep"),
            bulgu(900, Kanit::K1 { birim: "ajan-bekci-1.scope".into() }, "sleep 6", "/usr/bin/sleep"),
            bulgu(901, k2(), "python3 sunucu.py", "/usr/bin/python3"),
            bulgu(902, Kanit::K3 { neden: "öksüz · node".into() }, "node x", "/usr/bin/node"),
            bulgu(903, k2(), "sleep 7", "/usr/bin/sleep"),
        ];
        let beyaz = BeyazListe::ayristir("# yorum\nkomut sunucu.py\n").unwrap();
        let (h, d) = planla(&bulgular, &liste, &beyaz, None);
        assert_eq!(h.iter().map(|x| x.pid).collect::<Vec<_>>(), vec![900, 903]);
        assert_eq!(d, vec![(802, "in a live agent tree".into()), (901, "on the allow list".into())]);
        let (h, _) = planla(&bulgular, &liste, &beyaz, Some(&[903]));
        assert_eq!(h.iter().map(|x| x.pid).collect::<Vec<_>>(), vec![903]);
    }

    #[test]
    fn beyaz_liste_bicimi() {
        let b = BeyazListe::ayristir("exe /usr/bin/python3\n\n  komut  npm start \n").unwrap();
        assert!(b.eslesir(Some("/usr/bin/python3"), "x"));
        assert!(b.eslesir(None, "node npm start --x"));
        assert!(!b.eslesir(Some("/usr/bin/python"), "y"));
        assert!(BeyazListe::ayristir("python3\n").is_err());
        assert!(BeyazListe::ayristir("").unwrap().bos());
    }
}
