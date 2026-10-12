#![cfg(unix)]
//! Kafes davranış testleri: gerçek süreç ağaçları (bash, sleep, setsid).
//! Kaynak: kaynak depo 34d792b `tests/agac.rs` — orada aynı davranışlar bekçi İKİLİSİ üzerinden
//! sınanıyordu; sarmalayıcının kendi testleri ayrı olduğu için burada doğrudan `Kafes` üzerinden sınanır.
//!
//! `Kafes::kur` bu test sürecini subreaper yapar ve `topla` `waitpid(-1)` ile HER çocuğu toplar →
//! testler aynı süreçte paralel koşarsa birbirinin çocuğunu yutar. Hepsi tek kilitle sıralanır.
//! Her test kendine özgü bir `sleep <sayı>` işareti kullanır; yalnız kendi süreçlerini sayar.

use std::ffi::OsString;
use std::fs;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{Duration, Instant};

use ajan_bekci_cekirdek::Kafes;

static KILIT: Mutex<()> = Mutex::new(());
static SAYAC: AtomicU32 = AtomicU32::new(0);

fn sessiz(_: &str) {}

/// Bu teste özgü uyku süresi: `sleep 8<pid%1000><sayaç>1`.
fn isaret() -> String {
    let n = SAYAC.fetch_add(1, Ordering::SeqCst);
    format!("8{:03}{:02}1", std::process::id() % 1000, n)
}

/// cmdline'ı tam olarak `sleep <isaret>` olan canlı (zombi olmayan) süreçler.
fn uyuyanlar(isaret: &str) -> Vec<i32> {
    let hedef = format!("sleep\0{isaret}\0");
    let mut v = Vec::new();
    for g in fs::read_dir("/proc").unwrap().flatten() {
        let Ok(pid) = g.file_name().to_string_lossy().parse::<i32>() else { continue };
        let Ok(cmd) = fs::read(g.path().join("cmdline")) else { continue };
        if cmd != hedef.as_bytes() {
            continue;
        }
        let stat = fs::read_to_string(g.path().join("stat")).unwrap_or_default();
        let durum = stat.rsplit(')').next().and_then(|s| s.split_whitespace().next());
        if durum != Some("Z") {
            v.push(pid);
        }
    }
    v
}

fn bekle_ki<F: FnMut() -> bool>(tavan: Duration, mut kosul: F) -> bool {
    let bas = Instant::now();
    while bas.elapsed() < tavan {
        if kosul() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    kosul()
}

fn bash(betik: &str) -> Vec<OsString> {
    ["bash", "-c", betik].iter().map(OsString::from).collect()
}

/// Çocuk çıkana kadar topla (tavan 10 sn).
fn kodu_bekle(k: &mut Kafes) -> Option<u8> {
    let mut kod = None;
    bekle_ki(Duration::from_secs(10), || {
        kod = k.topla();
        kod.is_some()
    });
    kod
}

/// Test başarısız olsa da kendi işaretli süreçlerini bırakmasın.
struct Supurge(String);
impl Drop for Supurge {
    fn drop(&mut self) {
        for p in uyuyanlar(&self.0) {
            let _ = nix::sys::signal::kill(nix::unistd::Pid::from_raw(p), nix::sys::signal::Signal::SIGKILL);
        }
    }
}

#[test]
fn cikis_kodu_aynen_doner() {
    let _k = KILIT.lock().unwrap_or_else(|e| e.into_inner());
    let mut k = Kafes::kur(sessiz).unwrap();
    k.baslat(&bash("exit 7"), &[], ajan_bekci_cekirdek::Grup::Kendi).unwrap();
    assert_eq!(kodu_bekle(&mut k), Some(7));
}

#[test]
fn sinyalle_olen_cocuk_128_arti_n() {
    let _k = KILIT.lock().unwrap_or_else(|e| e.into_inner());
    let mut k = Kafes::kur(sessiz).unwrap();
    k.baslat(&bash("kill -9 $$"), &[], ajan_bekci_cekirdek::Grup::Kendi).unwrap();
    assert_eq!(kodu_bekle(&mut k), Some(128 + 9));
}

#[test]
fn baslatilamayan_komut_127() {
    let _k = KILIT.lock().unwrap_or_else(|e| e.into_inner());
    let mut k = Kafes::kur(sessiz).unwrap();
    assert_eq!(k.baslat(&[OsString::from("/yok/boyle-bir-komut")], &[], ajan_bekci_cekirdek::Grup::Kendi), Err(127));
}

#[test]
fn ortam_degiskeni_cocuga_gecer_ve_cocuk_kendi_grubunda() {
    let _k = KILIT.lock().unwrap_or_else(|e| e.into_inner());
    let dosya = std::env::temp_dir().join(format!("ajan-bekci-ortam-{}", isaret()));
    let betik = format!("echo \"$AB_DENEME $(ps -o pgid= -p $$ | tr -d ' ') $$\" > {}", dosya.display());
    let mut k = Kafes::kur(sessiz).unwrap();
    k.baslat(&bash(&betik), &[(OsString::from("AB_DENEME"), OsString::from("merhaba"))], ajan_bekci_cekirdek::Grup::Kendi).unwrap();
    assert_eq!(kodu_bekle(&mut k), Some(0));
    let icerik = fs::read_to_string(&dosya).unwrap();
    let _ = fs::remove_file(&dosya);
    let alan: Vec<&str> = icerik.split_whitespace().collect();
    assert_eq!(alan[0], "merhaba");
    assert_eq!(alan[1], alan[2], "çocuk kendi süreç grubunun lideri olmalı (pgid = pid)");
}

#[test]
fn kapanis_setsidli_torunu_da_kapatir() {
    let _k = KILIT.lock().unwrap_or_else(|e| e.into_inner());
    let i = isaret();
    let _s = Supurge(i.clone());
    let mut k = Kafes::kur(sessiz).unwrap();
    k.baslat(&bash(&format!("setsid sleep {i} & sleep {i} & wait")), &[], ajan_bekci_cekirdek::Grup::Kendi).unwrap();
    assert!(bekle_ki(Duration::from_secs(5), || uyuyanlar(&i).len() == 2), "iki uyuyan başlamalı");
    assert!(k.kalan() >= 3); // bash + iki sleep
    k.kapanisi_baslat();
    assert!(bekle_ki(Duration::from_secs(5), || {
        k.topla();
        k.kalan() == 0
    }));
    assert!(uyuyanlar(&i).is_empty());
}

#[test]
fn cocuk_cikinca_kacak_torun_kafeste_kalir_ve_supurulur() {
    let _k = KILIT.lock().unwrap_or_else(|e| e.into_inner());
    let i = isaret();
    let _s = Supurge(i.clone());
    let mut k = Kafes::kur(sessiz).unwrap();
    k.baslat(&bash(&format!("setsid -f sleep {i}; exit 0")), &[], ajan_bekci_cekirdek::Grup::Kendi).unwrap();
    assert_eq!(kodu_bekle(&mut k), Some(0));
    // subreaper: torun init'e değil bize bağlandı → hâlâ sayılıyor
    assert!(bekle_ki(Duration::from_secs(5), || k.kalan() == 1 && uyuyanlar(&i).len() == 1));
    k.kapanisi_baslat();
    assert!(bekle_ki(Duration::from_secs(5), || {
        k.topla();
        k.kalan() == 0
    }));
}

#[test]
fn sigtermi_yok_sayan_cocuk_oldur_ile_biter() {
    let _k = KILIT.lock().unwrap_or_else(|e| e.into_inner());
    let i = isaret();
    let _s = Supurge(i.clone());
    let mut k = Kafes::kur(sessiz).unwrap();
    k.baslat(&bash(&format!("trap '' TERM; sleep {i} & wait; sleep {i}")), &[], ajan_bekci_cekirdek::Grup::Kendi).unwrap();
    assert!(bekle_ki(Duration::from_secs(5), || uyuyanlar(&i).len() == 1));
    k.kapanisi_baslat();
    // sleep TERM'i yok saymaz ama bash yok sayar ve ikinci uykuya geçer
    std::thread::sleep(Duration::from_millis(500));
    k.topla();
    assert!(k.kalan() >= 1, "TERM'i yok sayan bash yaşamalı");
    k.oldur();
    assert!(bekle_ki(Duration::from_secs(5), || {
        k.topla();
        k.kalan() == 0
    }));
    assert_eq!(k.topla(), Some(128 + 9));
}

#[test]
fn bize_baglanan_oksuzler_zombi_kalmaz() {
    let _k = KILIT.lock().unwrap_or_else(|e| e.into_inner());
    let mut k = Kafes::kur(sessiz).unwrap();
    // Her biri anında ölen 5 setsid'li torun: ebeveynleri (bash) çıkınca bize bağlanırlar.
    k.baslat(&bash("for n in 1 2 3 4 5; do setsid -f true; done; sleep 0.3; exit 0"), &[], ajan_bekci_cekirdek::Grup::Kendi).unwrap();
    assert_eq!(kodu_bekle(&mut k), Some(0));
    std::thread::sleep(Duration::from_millis(300));
    k.topla();
    let ben = std::process::id() as i32;
    let zombiler = ajan_bekci_cekirdek::agac::torunlar(ben, &ajan_bekci_cekirdek::agac::tum_surecler())
        .into_iter()
        .filter(|s| s.zombi())
        .count();
    assert_eq!(zombiler, 0);
}

#[test]
fn oksuzu_durdur_yalniz_verilen_agaci_kapatir() {
    let _k = KILIT.lock().unwrap_or_else(|e| e.into_inner());
    let i = isaret();
    let j = isaret();
    let _s = (Supurge(i.clone()), Supurge(j.clone()));
    // Kafes yok: iki bağımsız ağaç, yalnız birincisi durdurulur.
    let mut a = std::process::Command::new("bash").args(["-c", &format!("sleep {i} & wait")]).spawn().unwrap();
    let mut b = std::process::Command::new("sleep").arg(&j).spawn().unwrap();
    assert!(bekle_ki(Duration::from_secs(5), || uyuyanlar(&i).len() == 1 && uyuyanlar(&j).len() == 1));
    ajan_bekci_cekirdek::oksuzu_durdur(a.id() as i32, Duration::from_secs(2), sessiz);
    let _ = a.wait();
    assert!(bekle_ki(Duration::from_secs(5), || uyuyanlar(&i).is_empty()));
    assert_eq!(uyuyanlar(&j).len(), 1, "başka ağaca dokunulmamalı");
    let _ = b.kill();
    let _ = b.wait();
}
