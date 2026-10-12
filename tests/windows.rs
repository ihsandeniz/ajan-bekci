#![cfg(windows)]
//! Windows sarmalayıcı — yalnız CI'da (windows-latest) koşar; gerçek Windows makinesi yok.
//! Sahte ajan `tests/veri/sahte-ajan.js` (node): MCP benzeri torun (+ onun çocuğu), `start /b` torunu,
//! `detached` torun. Her senaryo yalnız kendi açtığı süreçlere bakar; temizlikte yalnız kendi PID'leri.
//!
//! Senaryolar: normal çıkış + kod aktarımı → torun 0 · bekçiye TerminateProcess → torun 0
//! (KILL_ON_JOB_CLOSE) · iç içe iş (bekçi içinde bekçi) · `tara` v0.1 K3 (ebeveyni ölü node.exe).

use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU32, Ordering};
use std::thread::sleep;
use std::time::{Duration, Instant};

const BEKCI: &str = env!("CARGO_BIN_EXE_ajan-bekci");
static SAYAC: AtomicU32 = AtomicU32::new(0);
const TORUNLAR: [&str; 5] = [".ajan", ".mcp", ".mcp.arac", ".sb", ".ayrik"];

fn veri(ad: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests").join("veri").join(ad)
}

/// Senaryoya özgü dosya öneki (PID dosyaları buraya yazılır).
fn onek() -> PathBuf {
    let n = SAYAC.fetch_add(1, Ordering::SeqCst);
    let d = std::env::temp_dir().join(format!("ajan-bekci-test-{}-{n}", std::process::id()));
    let _ = std::fs::create_dir_all(&d);
    d.join("p")
}

fn yasiyor(pid: u32) -> bool {
    let c = Command::new("tasklist").args(["/FI", &format!("PID eq {pid}"), "/NH", "/FO", "CSV"]).output().unwrap();
    String::from_utf8_lossy(&c.stdout).contains(&format!("\"{pid}\""))
}

fn bekle_ki<F: FnMut() -> bool>(tavan: Duration, mut kosul: F) -> bool {
    let bas = Instant::now();
    while bas.elapsed() < tavan {
        if kosul() {
            return true;
        }
        sleep(Duration::from_millis(100));
    }
    kosul()
}

/// Bütün PID dosyaları yazılınca (pid, ad) listesi.
fn pidler(onek: &Path) -> Vec<(u32, &'static str)> {
    assert!(
        bekle_ki(Duration::from_secs(20), || TORUNLAR.iter().all(|e| Path::new(&format!("{}{e}", onek.display())).exists())),
        "torunlar başlamadı: {}",
        onek.display()
    );
    sleep(Duration::from_millis(200)); // dosya yazımı bitsin
    TORUNLAR
        .iter()
        .map(|e| (std::fs::read_to_string(format!("{}{e}", onek.display())).unwrap().trim().parse().unwrap(), *e))
        .collect()
}

/// Kendi PID'lerimizi temizler (test kırılsa bile; yalnız bu senaryonun yazdığı PID'ler).
struct Supurge(PathBuf, Vec<Child>);
impl Drop for Supurge {
    fn drop(&mut self) {
        for c in &mut self.1 {
            let _ = c.kill();
            let _ = c.wait();
        }
        for e in TORUNLAR {
            if let Ok(p) = std::fs::read_to_string(format!("{}{e}", self.0.display())) {
                let _ = Command::new("taskkill").args(["/F", "/PID", p.trim()]).output();
            }
        }
    }
}

fn bekci(arg: &[&str]) -> Command {
    let mut k = Command::new(BEKCI);
    k.args(arg).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
    k
}

fn ajan_arg(onek: &Path, kip: &[&str]) -> Vec<String> {
    let mut v = vec!["node".to_owned(), veri("sahte-ajan.js").display().to_string(), onek.display().to_string()];
    v.extend(kip.iter().map(|s| (*s).to_owned()));
    v
}

fn hepsi_oldu(p: &[(u32, &str)]) -> bool {
    bekle_ki(Duration::from_secs(10), || p.iter().all(|(pid, _)| !yasiyor(*pid)))
}

fn kalanlar(p: &[(u32, &str)]) -> Vec<String> {
    p.iter().filter(|(pid, _)| yasiyor(*pid)).map(|(pid, ad)| format!("{ad}={pid}")).collect()
}

#[test]
fn cikis_kodu_aktarilir() {
    assert_eq!(bekci(&["--", "cmd", "/c", "exit 7"]).status().unwrap().code(), Some(7));
    assert_eq!(bekci(&["--", "node", "-e", "process.exit(3)"]).status().unwrap().code(), Some(3));
    assert_eq!(bekci(&["--", "olmayan-komut-xyz"]).status().unwrap().code(), Some(127));
}

#[test]
fn ajan_cikinca_torunlar_supurulur() {
    let o = onek();
    let _s = Supurge(o.clone(), Vec::new());
    let arg = ajan_arg(&o, &["cik", "5"]);
    let mut k = bekci(&["--"]);
    k.args(&arg);
    let kod = k.status().unwrap().code();
    let p = pidler(&o);
    assert_eq!(kod, Some(5), "ajanın çıkış kodu aktarılmalı");
    assert!(hepsi_oldu(&p), "kalan torun: {:?}", kalanlar(&p));
}

#[test]
fn bekciye_terminateprocess_torun_0() {
    let o = onek();
    let arg = ajan_arg(&o, &["bekle"]);
    let mut k = bekci(&["--"]);
    k.args(&arg);
    let b = k.spawn().unwrap();
    let mut s = Supurge(o.clone(), vec![b]);
    let p = pidler(&o);
    assert!(p.iter().all(|(pid, _)| yasiyor(*pid)), "başlangıçta hepsi canlı olmalı");
    s.1[0].kill().unwrap(); // TerminateProcess — bekçi hiçbir şey yapamadan ölür
    s.1[0].wait().unwrap();
    assert!(hepsi_oldu(&p), "KILL_ON_JOB_CLOSE torunları öldürmeliydi, kalan: {:?}", kalanlar(&p));
}

#[test]
fn ic_ice_is_dis_bekci_olunce_hepsi_olur_kod_aktarilir() {
    // Kod iki kat bekçiden geçer.
    let o = onek();
    let _s = Supurge(o.clone(), Vec::new());
    let arg = ajan_arg(&o, &["cik", "9"]);
    let mut k = bekci(&["--", BEKCI, "--"]);
    k.args(&arg);
    assert_eq!(k.status().unwrap().code(), Some(9));
    let p = pidler(&o);
    assert!(hepsi_oldu(&p), "iç içe normal çıkış, kalan: {:?}", kalanlar(&p));

    // Dış bekçiye TerminateProcess: iç bekçi de torunlar da ölür.
    let o = onek();
    let arg = ajan_arg(&o, &["bekle"]);
    let mut k = bekci(&["--", BEKCI, "--"]);
    k.args(&arg);
    let b = k.spawn().unwrap();
    let mut s = Supurge(o.clone(), vec![b]);
    let p = pidler(&o);
    s.1[0].kill().unwrap();
    s.1[0].wait().unwrap();
    assert!(hepsi_oldu(&p), "iç içe TerminateProcess, kalan: {:?}", kalanlar(&p));
}

#[test]
fn tara_v01_ebeveyni_olu_node_k3_listelenir() {
    use std::os::windows::process::CommandExt;
    // Bekçisiz: `cmd /c start "" /b node uyu.js` → cmd çıkar, node'un ebeveyni ölü → K3.
    // cmd satırı `raw_arg` ile: Rust'ın `\"` kaçışını cmd anlamaz.
    let o = onek();
    let _s = Supurge(o.clone(), Vec::new());
    let d = format!("{}.sb", o.display());
    let mut cmd = Command::new("cmd");
    cmd.raw_arg(format!("/c start \"\" /b node \"{}\" \"{d}\"", veri("uyu.js").display()))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    let durum = sureli(cmd, "cmd start /b");
    eprintln!("cmd çıktı: {durum:?}");
    assert!(bekle_ki(Duration::from_secs(10), || Path::new(&d).exists()), "start /b node başlamadı");
    sleep(Duration::from_millis(200));
    let pid: u32 = std::fs::read_to_string(&d).unwrap().trim().parse().unwrap();
    eprintln!("öksüz node {pid}");
    let (kod, j) = sureli_cikti(&["tara", "--json"], "tara --json");
    assert_eq!(kod, Some(0), "Windows tara yalnız K3 → çıkış 0");
    assert!(j.contains(&format!("{{\"pid\":{pid},\"kanit\":\"K3\"")), "ebeveyni ölü node K3 olmalı: {j}");
    assert!(yasiyor(pid), "tara hiçbir sürece dokunmamalı");
    let (kod, _) = sureli_cikti(&["tara", "--temizle"], "tara --temizle");
    assert_eq!(kod, Some(1), "Windows'ta --temizle yok (v0.2)");
}

/// Komutu en çok 30 sn bekle; aşarsa öldür ve hangi aşamada asıldığını yazarak kır.
fn sureli(mut k: Command, asama: &str) -> Option<i32> {
    let mut c = k.spawn().unwrap();
    let bas = Instant::now();
    loop {
        if let Some(d) = c.try_wait().unwrap() {
            return d.code();
        }
        if bas.elapsed() > Duration::from_secs(30) {
            let _ = c.kill();
            panic!("{asama} 30 sn'de bitmedi (asılı)");
        }
        sleep(Duration::from_millis(50));
    }
}

/// `ajan-bekci <arg>` çıktısını dosyaya alarak (boru yok → boru ucunu tutan torun bekletemez) süreli koş.
fn sureli_cikti(arg: &[&str], asama: &str) -> (Option<i32>, String) {
    let yol = std::env::temp_dir().join(format!("ajan-bekci-cikti-{}-{}", std::process::id(), SAYAC.fetch_add(1, Ordering::SeqCst)));
    let f = std::fs::File::create(&yol).unwrap();
    let mut k = Command::new(BEKCI);
    k.args(arg).stdin(Stdio::null()).stdout(f).stderr(Stdio::null());
    let kod = sureli(k, asama);
    let metin = std::fs::read_to_string(&yol).unwrap_or_default();
    let _ = std::fs::remove_file(&yol);
    (kod, metin)
}
