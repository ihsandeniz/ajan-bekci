//! `tara` — ölmüş bir ajandan kalan süreçleri kanıt seviyesiyle LİSTELER. Hiçbir sürece sinyal
//! göndermez (temizlik `temizle.rs`'te).
//!
//! Kanıt seviyeleri:
//! - **K1 kesin:** `ajan-bekci-*.scope` (ya da süreçteki `AJAN_BEKCI_KAFES` adlı scope) içinde,
//!   o scope'ta canlı bir `ajan-bekci` kalmamışken duran süreç. cgroup üyeliği ortam değişkeninden
//!   bağımsızdır: `env -i` ile başlayan torun da yakalanır.
//! - **K2 güçlü:** environ'da `CLAUDE_PID=X` ya da `AJAN_BEKCI_PID=X`; X ölü ya da X'in başlangıç anı
//!   bu süreçten SONRA (PID başkasına geçmiş). Canlı ajanın torunu X'i canlı ve kendinden önce görür.
//! - **K3 ipucu:** ebeveyni PID 1 ya da kullanıcı systemd'si, üstüne ajan izi (claude/codex ortam
//!   işareti, adında mcp, `node`) ya da süreç grubu/sid lideri ölü. Yalnız "şüpheli"; ASLA temizlenmez.
//!
//! Codex PID taşıyan bir ortam işareti bırakmıyor (ölçümlerde böyle bir işaret görülmedi):
//! sarmalanmamış codex'in kaçağı yalnız K3; `ajan-bekci -- codex` ile sarmalanmışsa K1/K2.
//!
//! environ'dan yalnız bu dosyadaki dört anahtar okunur; değerleri yalnız `CLAUDE_PID`/`AJAN_BEKCI_PID`/
//! `AJAN_BEKCI_KAFES` için tutulur, başka hiçbir değişken saklanmaz ya da basılmaz.

use crate::dil::m;
use std::collections::{HashMap, HashSet};
use std::fs;

use nix::libc;

/// Ortamdan okunan ajan izleri (başka değişken tutulmaz).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Isaret {
    pub claude_pid: Option<i32>,
    pub bekci_pid: Option<i32>,
    pub bekci_kafes: Option<String>,
    /// `CLAUDECODE` anahtarı var mı (değeri tutulmaz).
    pub claudecode: bool,
    /// `CODEX_` ile başlayan bir anahtar var mı (adı/değeri tutulmaz).
    pub codex: bool,
}

/// Bir sürecin `tara` için gereken özeti.
#[derive(Debug, Clone, Default)]
pub struct Bilgi {
    pub pid: i32,
    pub ppid: i32,
    pub pgid: i32,
    pub sid: i32,
    pub durum: char,
    pub uid: u32,
    /// Açılıştan beri saat tıkı (`/proc/<pid>/stat` 22. alan).
    pub baslangic: u64,
    pub ad: String,
    pub komut: String,
    pub exe_adi: Option<String>,
    /// exe'nin tam yolu (` (deleted)` eki atılmış) — `--temizle` kimlik doğrulaması.
    pub exe_yol: Option<String>,
    pub rss_kb: u64,
    /// cgroup yolunun son parçası (ör. `ajan-bekci-123.scope`).
    pub cgroup_yaprak: String,
    pub isaret: Isaret,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Kanit {
    K1 { birim: String },
    K2 { degisken: &'static str, hedef: i32, neden: &'static str },
    K3 { neden: String },
}

impl Kanit {
    pub fn seviye(&self) -> &'static str {
        match self {
            Kanit::K1 { .. } => "K1",
            Kanit::K2 { .. } => "K2",
            Kanit::K3 { .. } => "K3",
        }
    }

    pub fn ayrinti(&self) -> String {
        match self {
            Kanit::K1 { birim } => match crate::dil::simdi() {
                crate::dil::Dil::En => format!("in {birim}, no guard alive"),
                crate::dil::Dil::Tr => format!("{birim} içinde, bekçisi yok"),
            },
            Kanit::K2 { degisken, hedef, neden } => format!("{degisken}={hedef} {neden}"),
            Kanit::K3 { neden } => neden.clone(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct Bulgu {
    pub pid: i32,
    pub kanit: Kanit,
    pub ajan: &'static str,
    pub yas_sn: u64,
    pub rss_kb: u64,
    pub komut: String,
    /// Kimlik (`--temizle`): listelendiği andaki başlangıç anı (tık) ve exe yolu.
    pub baslangic: u64,
    pub exe_yol: Option<String>,
}

/// Komut satırı tabloda/JSON'da bu kadar karakterle kesilir.
const KOMUT_TAVAN: usize = 160;

fn ajan_adi(i: &Isaret) -> &'static str {
    if i.claude_pid.is_some() || i.claudecode {
        "claude"
    } else if i.codex {
        "codex"
    } else if i.bekci_pid.is_some() || i.bekci_kafes.is_some() {
        m("guard", "bekçi")
    } else {
        "?"
    }
}

fn bekci_mi(b: &Bilgi) -> bool {
    b.ad == "ajan-bekci" || b.exe_adi.as_deref() == Some("ajan-bekci")
}

fn zombi(b: &Bilgi) -> bool {
    b.durum == 'Z' || b.durum == 'X'
}

/// K1 adayı mı: yaprak `ajan-bekci-*.scope` ya da sürecin kendi `AJAN_BEKCI_KAFES` adı + `.scope`.
fn bekci_scope(b: &Bilgi) -> Option<&str> {
    let y = b.cgroup_yaprak.as_str();
    let ad = y.strip_suffix(".scope")?;
    let kendi = b.isaret.bekci_kafes.as_deref() == Some(ad);
    (ad.starts_with("ajan-bekci-") || kendi).then_some(y)
}

/// Karar mantığı (platformdan ve /proc'tan bağımsız; testler sahte listeyle besler).
/// `liste` bütün süreçleri (her uid) içerir: K2'nin "X ölü mü / sonra mı başladı" sorusu için.
/// Bulgu yalnız `uid`'e ait, `haric`te olmayan, zombi olmayan süreçler için üretilir.
pub fn sinifla(liste: &[Bilgi], uid: u32, haric: &[i32], simdi: u64, hz: u64) -> Vec<Bulgu> {
    let pidler: HashMap<i32, &Bilgi> = liste.iter().map(|b| (b.pid, b)).collect();
    // Kullanıcı systemd'si (ve PID 1): öksüzlerin bağlandığı yerler.
    let mut kokler: HashSet<i32> = liste.iter().filter(|b| b.ad == "systemd" && b.uid == uid).map(|b| b.pid).collect();
    kokler.insert(1);
    // İçinde canlı bekçi olan scope'lar yaşıyor: üyeleri K1 değil.
    let canli_scope: HashSet<&str> =
        liste.iter().filter(|b| bekci_mi(b) && !zombi(b)).map(|b| b.cgroup_yaprak.as_str()).collect();
    let canli = |p: i32| pidler.get(&p).is_some_and(|b| !zombi(b));

    let mut sonuc = Vec::new();
    for b in liste {
        if b.uid != uid || zombi(b) || haric.contains(&b.pid) || b.pid == 1 {
            continue;
        }
        let kanit = if let Some(y) = bekci_scope(b).filter(|y| !canli_scope.contains(y)) {
            Some(Kanit::K1 { birim: y.to_owned() })
        } else if let Some(k) = k2(b, &pidler) {
            Some(k)
        } else if kokler.contains(&b.ppid) && !kokler.contains(&b.pid) {
            k3(b, &canli)
        } else {
            None
        };
        if let Some(kanit) = kanit {
            let mut komut = if b.komut.is_empty() { format!("[{}]", b.ad) } else { b.komut.clone() };
            if let Some((i, _)) = komut.char_indices().nth(KOMUT_TAVAN) {
                komut.truncate(i);
                komut.push('…');
            }
            sonuc.push(Bulgu {
                pid: b.pid,
                kanit,
                ajan: ajan_adi(&b.isaret),
                yas_sn: simdi.saturating_sub(b.baslangic) / hz.max(1),
                rss_kb: b.rss_kb,
                komut,
                baslangic: b.baslangic,
                exe_yol: b.exe_yol.clone(),
            });
        }
    }
    sonuc.sort_by_key(|g| (g.kanit.seviye(), g.pid));
    sonuc
}

fn k2(b: &Bilgi, pidler: &HashMap<i32, &Bilgi>) -> Option<Kanit> {
    let adaylar = [("CLAUDE_PID", b.isaret.claude_pid), ("AJAN_BEKCI_PID", b.isaret.bekci_pid)];
    for (degisken, hedef) in adaylar {
        let Some(hedef) = hedef else { continue };
        if hedef == b.pid {
            continue; // kendisi (ör. bekçinin kendi ortamı) — kanıt değil
        }
        let neden = match pidler.get(&hedef) {
            None => m("dead", "ölü"),
            Some(h) if zombi(h) => m("dead (zombie)", "ölü (zombi)"),
            Some(h) if h.baslangic > b.baslangic => m("started later (PID reused)", "sonradan başlamış (PID başkasına geçmiş)"),
            Some(_) => continue,
        };
        return Some(Kanit::K2 { degisken, hedef, neden });
    }
    None
}

fn k3(b: &Bilgi, canli: &dyn Fn(i32) -> bool) -> Option<Kanit> {
    let mut neden = Vec::new();
    if b.isaret.claudecode || b.isaret.claude_pid.is_some() {
        neden.push(m("claude trace", "claude izi"));
    }
    if b.isaret.codex {
        neden.push(m("codex trace", "codex izi"));
    }
    if b.isaret.bekci_pid.is_some() || b.isaret.bekci_kafes.is_some() {
        neden.push(m("guard trace", "bekçi izi"));
    }
    let kucuk = format!("{} {}", b.ad, b.komut).to_lowercase();
    if kucuk.contains("mcp") {
        neden.push(m("mcp in name", "adında mcp"));
    }
    if b.ad == "node" {
        neden.push("node");
    }
    // Ölçülen codex kaçağı: ppid 1, pgid = ölü MCP, sid = ölü ajan. Lider kendisi değilse ve ölüyse ipucu.
    if b.pgid != b.pid && b.pgid > 0 && !canli(b.pgid) && b.sid != b.pid && b.sid > 0 && !canli(b.sid) {
        neden.push(m("group and sid leaders dead", "grup ve sid lideri ölü"));
    }
    (!neden.is_empty()).then(|| Kanit::K3 { neden: format!("{} · {}", m("orphan", "öksüz"), neden.join(", ")) })
}

// ---------- /proc okuma ----------

/// `/proc/<pid>/stat`: pid, ad, durum, ppid, pgid, sid, başlangıç (22. alan).
fn stat_oku(pid: &str) -> Option<(char, i32, i32, i32, u64, String)> {
    let s = fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    let ac = s.find('(')?;
    let kapa = s.rfind(')')?;
    let ad = s[ac + 1..kapa].to_owned();
    let a: Vec<&str> = s.get(kapa + 1..)?.split_whitespace().collect();
    // a[0]=durum(3) a[1]=ppid(4) a[2]=pgrp(5) a[3]=sid(6) … a[19]=starttime(22)
    Some((a.first()?.chars().next()?, a.get(1)?.parse().ok()?, a.get(2)?.parse().ok()?, a.get(3)?.parse().ok()?, a.get(19)?.parse().ok()?, ad))
}

/// environ'dan YALNIZ ajan anahtarlarını çıkar. Diğer değişkenler okunur ama hiçbir yere kopyalanmaz.
pub fn isaret_ayristir(environ: &[u8]) -> Isaret {
    let mut i = Isaret::default();
    for parca in environ.split(|b| *b == 0) {
        let (anahtar, deger) = match parca.iter().position(|b| *b == b'=') {
            Some(n) => (&parca[..n], &parca[n + 1..]),
            None => (parca, &[][..]),
        };
        let pid = || std::str::from_utf8(deger).ok().and_then(|d| d.trim().parse::<i32>().ok()).filter(|p| *p > 0);
        match anahtar {
            b"CLAUDE_PID" => i.claude_pid = pid(),
            b"AJAN_BEKCI_PID" => i.bekci_pid = pid(),
            b"AJAN_BEKCI_KAFES" => i.bekci_kafes = std::str::from_utf8(deger).ok().map(str::to_owned),
            b"CLAUDECODE" => i.claudecode = true,
            a if a.starts_with(b"CODEX_") => i.codex = true,
            _ => {}
        }
    }
    i
}

/// `/proc/<pid>/exe` tam yolu; silinmiş ikilinin ` (deleted)` eki atılır.
pub fn exe_oku(pid: i32) -> Option<String> {
    let y = fs::read_link(format!("/proc/{pid}/exe")).ok()?.to_string_lossy().into_owned();
    Some(y.strip_suffix(" (deleted)").map(str::to_owned).unwrap_or(y))
}

/// `/proc/<pid>/stat`'tan yalnız durum + başlangıç anı (`--temizle` yeniden doğrulaması).
pub fn durum_ve_baslangic(pid: i32) -> Option<(char, u64)> {
    stat_oku(&pid.to_string()).map(|(d, _, _, _, b, _)| (d, b))
}

fn bilgi_oku(pid_s: &str, uid: u32) -> Option<Bilgi> {
    let (durum, ppid, pgid, sid, baslangic, ad) = stat_oku(pid_s)?;
    let durum_dosyasi = fs::read_to_string(format!("/proc/{pid_s}/status")).unwrap_or_default();
    let alan = |anahtar: &str| {
        durum_dosyasi.lines().find_map(|l| l.strip_prefix(anahtar)).and_then(|d| d.split_whitespace().next()).and_then(|d| d.parse::<u64>().ok())
    };
    let suid = alan("Uid:").map_or(u32::MAX, |u| u as u32);
    let mut b = Bilgi { pid: pid_s.parse().ok()?, ppid, pgid, sid, durum, uid: suid, baslangic, ad, ..Default::default() };
    // Ayrıntı yalnız kendi uid'imiz için (başkasının environ'u zaten okunamaz).
    if suid == uid {
        b.rss_kb = alan("VmRSS:").unwrap_or(0);
        b.komut = fs::read(format!("/proc/{pid_s}/cmdline"))
            .map(|c| String::from_utf8_lossy(&c).trim_end_matches('\0').replace('\0', " "))
            .unwrap_or_default();
        b.exe_yol = exe_oku(b.pid);
        b.exe_adi = b.exe_yol.as_deref().and_then(|y| y.rsplit('/').next()).map(str::to_owned);
        b.cgroup_yaprak = fs::read_to_string(format!("/proc/{pid_s}/cgroup"))
            .ok()
            .and_then(|c| c.lines().find_map(|l| l.strip_prefix("0::")).map(|y| y.rsplit('/').next().unwrap_or("").to_owned()))
            .unwrap_or_default();
        b.isaret = fs::read(format!("/proc/{pid_s}/environ")).map(|e| isaret_ayristir(&e)).unwrap_or_default();
    }
    Some(b)
}

/// /proc'taki bütün süreçlerin özeti (her uid; ayrıntı yalnız kendi uid'imiz için).
pub fn surecleri_oku(uid: u32) -> Vec<Bilgi> {
    let Ok(dizin) = fs::read_dir("/proc") else { return Vec::new() };
    dizin
        .filter_map(|g| g.ok())
        .map(|g| g.file_name().to_string_lossy().into_owned())
        .filter(|a| a.bytes().all(|b| b.is_ascii_digit()))
        .filter_map(|a| bilgi_oku(&a, uid))
        .collect()
}

/// Bu makinede tara: kendi uid'imiz, kendimiz ve atalarımız hariç.
pub fn tara() -> Vec<Bulgu> {
    anlik().1
}

/// Taze anlık görüntü: (bütün süreçler, bulgular, uid).
pub fn anlik() -> (Vec<Bilgi>, Vec<Bulgu>, u32) {
    // SAFETY: yalnız sorgu.
    let (uid, hz) = unsafe { (libc::getuid(), libc::sysconf(libc::_SC_CLK_TCK)) };
    let hz = u64::try_from(hz).unwrap_or(100).max(1);
    let liste = surecleri_oku(uid);
    let simdi = fs::read_to_string("/proc/uptime")
        .ok()
        .and_then(|u| u.split_whitespace().next()?.parse::<f64>().ok())
        .map_or(0, |s| (s * hz as f64) as u64);
    let pidler: HashMap<i32, i32> = liste.iter().map(|b| (b.pid, b.ppid)).collect();
    let mut haric = vec![std::process::id() as i32];
    while let Some(p) = pidler.get(haric.last().unwrap()).copied().filter(|p| *p > 1 && !haric.contains(p)) {
        haric.push(p);
    }
    let bulgular = sinifla(&liste, uid, &haric, simdi, hz);
    (liste, bulgular, uid)
}

/// JSON metni için kaçış (bağımlılıksız).
fn json_metin(s: &str) -> String {
    let mut o = String::with_capacity(s.len() + 2);
    o.push('"');
    for c in s.chars() {
        match c {
            '"' => o.push_str("\\\""),
            '\\' => o.push_str("\\\\"),
            '\n' => o.push_str("\\n"),
            '\r' => o.push_str("\\r"),
            '\t' => o.push_str("\\t"),
            c if (c as u32) < 0x20 => o.push_str(&format!("\\u{:04x}", c as u32)),
            c => o.push(c),
        }
    }
    o.push('"');
    o
}

pub fn json(bulgular: &[Bulgu]) -> String {
    let satirlar: Vec<String> = bulgular
        .iter()
        .map(|b| {
            format!(
                "{{\"pid\":{},\"kanit\":{},\"ayrinti\":{},\"ajan\":{},\"yas_sn\":{},\"rss_kb\":{},\"komut\":{}}}",
                b.pid,
                json_metin(b.kanit.seviye()),
                json_metin(&b.kanit.ayrinti()),
                json_metin(b.ajan),
                b.yas_sn,
                b.rss_kb,
                json_metin(&b.komut)
            )
        })
        .collect();
    format!("[{}]", satirlar.join(",\n "))
}

fn yas_metni(sn: u64) -> String {
    match sn {
        s if s < 120 => format!("{s}{}", m("s", "sn")),
        s if s < 7200 => format!("{}{}", s / 60, m("m", "dk")),
        s if s < 172_800 => format!("{}{}", s / 3600, m("h", "sa")),
        s => format!("{}{}", s / 86_400, m("d", "g")),
    }
}

pub fn tablo(bulgular: &[Bulgu]) -> String {
    let mut o = format!(
        "{:>7}  {:<5} {:<6} {:>5} {:>8}  {}\n",
        "PID",
        m("LEVEL", "KANIT"),
        m("AGENT", "AJAN"),
        m("AGE", "YAŞ"),
        "RSS",
        m("COMMAND · detail", "KOMUT · ayrıntı")
    );
    for b in bulgular {
        let ayrinti = b.kanit.ayrinti();
        o.push_str(&format!(
            "{:>7}  {:<5} {:<6} {:>5} {:>6}MB  {} · {ayrinti}\n",
            b.pid,
            b.kanit.seviye(),
            b.ajan,
            yas_metni(b.yas_sn),
            b.rss_kb / 1024,
            b.komut
        ));
    }
    let say = |s: &str| bulgular.iter().filter(|b| b.kanit.seviye() == s).count();
    let (k1, k2, k3) = (say("K1"), say("K2"), say("K3"));
    o.push_str(&match crate::dil::simdi() {
        crate::dil::Dil::En => format!("K1 certain {k1} · K2 strong {k2} · K3 suspect {k3} (K3 is never cleaned) · no process was signalled\n"),
        crate::dil::Dil::Tr => format!("K1 kesin {k1} · K2 güçlü {k2} · K3 şüpheli {k3} (K3 asla temizlenmez) · hiçbir sürece sinyal gönderilmedi\n"),
    });
    o
}

#[cfg(test)]
mod testler {
    use super::*;

    const BEN: u32 = 1000;

    fn s(pid: i32, ppid: i32, bas: u64, ad: &str) -> Bilgi {
        Bilgi { pid, ppid, pgid: pid, sid: pid, durum: 'S', uid: BEN, baslangic: bas, ad: ad.into(), komut: ad.into(), ..Default::default() }
    }

    fn seviye(b: &[Bulgu], pid: i32) -> Option<&'static str> {
        b.iter().find(|x| x.pid == pid).map(|x| x.kanit.seviye())
    }

    fn temel() -> Vec<Bilgi> {
        vec![s(1, 0, 0, "systemd"), s(861, 1, 10, "systemd"), s(500, 1, 50, "kitty"), s(600, 500, 60, "zsh")]
    }

    #[test]
    fn mesru_oksuzler_hicbir_seviyede_cikmaz() {
        // Hyprland exec'iyle PID 1'e bağlanan uygulamalar + user systemd servisleri: iz yok → bulgu yok.
        let mut l = temel();
        l.push(s(700, 861, 70, "python3"));
        let b = sinifla(&l, BEN, &[], 1000, 100);
        assert!(b.is_empty(), "{b:?}");
    }

    #[test]
    fn k2_canli_claudeun_torunu_cikmaz_olunun_cikar() {
        let mut l = temel();
        l.push(s(800, 600, 80, "claude"));
        let mut canli = s(801, 800, 81, "bash");
        canli.isaret.claude_pid = Some(800);
        let mut olu = s(900, 1, 90, "sleep");
        olu.isaret.claude_pid = Some(4242);
        l.extend([canli, olu]);
        let b = sinifla(&l, BEN, &[], 1000, 100);
        assert_eq!(seviye(&b, 801), None);
        assert_eq!(seviye(&b, 900), Some("K2"));
    }

    #[test]
    fn k2_pid_yeniden_kullanimi_sonra_baslamis() {
        let mut l = temel();
        l.push(s(800, 600, 200, "firefox")); // 800 artık başka biri, sonra başladı
        let mut eski = s(900, 1, 90, "sleep");
        eski.isaret.claude_pid = Some(800);
        l.push(eski);
        let b = sinifla(&l, BEN, &[], 1000, 100);
        assert!(matches!(&b[0].kanit, Kanit::K2 { neden, .. } if neden.contains("later")));
    }

    #[test]
    fn k2_zombi_hedef_olu_sayilir_ve_bekci_pid_de_kanit() {
        let mut l = temel();
        let mut z = s(800, 1, 50, "claude");
        z.durum = 'Z';
        let mut a = s(900, 1, 90, "sleep");
        a.isaret.claude_pid = Some(800);
        let mut c = s(901, 1, 91, "sleep");
        c.isaret.bekci_pid = Some(4243);
        l.extend([z, a, c]);
        let b = sinifla(&l, BEN, &[], 1000, 100);
        assert_eq!(seviye(&b, 900), Some("K2"));
        assert_eq!(seviye(&b, 901), Some("K2"));
        assert_eq!(seviye(&b, 800), None, "zombi listelenmez");
    }

    #[test]
    fn k1_bekcisiz_scope_kesin_canli_bekcili_scope_degil() {
        let mut l = temel();
        let mut kalan = s(900, 861, 90, "sleep");
        kalan.cgroup_yaprak = "ajan-bekci-777.scope".into();
        let mut bekci = s(910, 600, 91, "ajan-bekci");
        bekci.cgroup_yaprak = "ajan-bekci-910.scope".into();
        let mut ajan = s(911, 910, 92, "codex");
        ajan.cgroup_yaprak = "ajan-bekci-910.scope".into();
        let mut adli = s(920, 1, 93, "sleep");
        adli.cgroup_yaprak = "deneme.scope".into();
        adli.isaret.bekci_kafes = Some("deneme".into());
        let mut baska = s(930, 861, 94, "sleep");
        baska.cgroup_yaprak = "donanim-bekci.service".into();
        l.extend([kalan, bekci, ajan, adli, baska]);
        let b = sinifla(&l, BEN, &[], 1000, 100);
        assert_eq!(seviye(&b, 900), Some("K1"));
        assert_eq!(seviye(&b, 920), Some("K1"));
        assert_eq!(seviye(&b, 911), None);
        assert_eq!(seviye(&b, 910), None);
        assert_eq!(seviye(&b, 930), None);
    }

    #[test]
    fn k3_yalniz_oksuz_ve_izli() {
        let mut l = temel();
        let mut mcp = s(900, 1, 90, "node");
        mcp.komut = "node /x/mcp-server.js".into();
        let mut gomulu = s(901, 600, 91, "node"); // öksüz değil
        gomulu.komut = "node mcp".into();
        let mut arac = s(902, 1, 92, "sleep"); // ölçülen codex kaçağının şekli
        arac.pgid = 4000;
        arac.sid = 3999;
        let mut cx = s(903, 861, 93, "bash");
        cx.isaret.codex = true;
        l.extend([mcp, gomulu, arac, cx]);
        let b = sinifla(&l, BEN, &[], 1000, 100);
        assert_eq!(seviye(&b, 900), Some("K3"));
        assert_eq!(seviye(&b, 901), None);
        assert_eq!(seviye(&b, 902), Some("K3"));
        assert_eq!(seviye(&b, 903), Some("K3"));
        assert_eq!(b.iter().find(|x| x.pid == 903).unwrap().ajan, "codex");
    }

    #[test]
    fn baska_uid_haric_ve_kok_listelenmez() {
        let mut l = temel();
        let mut el = s(900, 1, 90, "sleep");
        el.isaret.claude_pid = Some(4242);
        el.uid = 0;
        let mut ben = s(901, 1, 91, "sleep");
        ben.isaret.claude_pid = Some(4242);
        l.extend([el, ben]);
        let b = sinifla(&l, BEN, &[901], 1000, 100);
        assert!(b.is_empty(), "{b:?}");
    }

    #[test]
    fn isaret_yalniz_ajan_anahtarlarini_tutar() {
        let i = isaret_ayristir(b"PATH=/usr/bin\0CLAUDE_PID=50413\0GIZLI=abc\0CLAUDECODE=1\0CODEX_SHELL=1\0AJAN_BEKCI_KAFES=ab-1\0AJAN_BEKCI_PID=x\0");
        assert_eq!(i, Isaret { claude_pid: Some(50413), bekci_pid: None, bekci_kafes: Some("ab-1".into()), claudecode: true, codex: true });
        assert!(!format!("{i:?}").contains("abc"));
    }

    #[test]
    fn json_kacis_ve_tablo() {
        let b = vec![Bulgu { pid: 7, kanit: Kanit::K3 { neden: "a\"b".into() }, ajan: "?", yas_sn: 3, rss_kb: 2048, komut: "x\ny".into(), baslangic: 1, exe_yol: None }];
        assert_eq!(json(&b), r#"[{"pid":7,"kanit":"K3","ayrinti":"a\"b","ajan":"?","yas_sn":3,"rss_kb":2048,"komut":"x\ny"}]"#);
        assert!(tablo(&b).contains("K3 suspect 1"));
        assert_eq!(json(&[]), "[]");
    }

    #[test]
    fn gercek_procta_kendimiz_okunur() {
        let ben = std::process::id() as i32;
        let uid = unsafe { libc::getuid() };
        let l = surecleri_oku(uid);
        let b = l.iter().find(|b| b.pid == ben).unwrap();
        assert!(b.baslangic > 0 && !b.cgroup_yaprak.is_empty());
        assert!(tara().iter().all(|g| g.pid != ben));
    }
}
