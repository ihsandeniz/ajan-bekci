#![cfg(unix)]
//! Sarmalayıcı davranış testleri: gerçek `ajan-bekci` ikilisi + sahte süreç ağaçları (bash, sleep, setsid).
//! Kaynak: kaynak depo 34d792b `tests/agac.rs` (orada çekirdek testinde ikili yoktu, burada tamamlandı).
//! Her test kendine özgü `sleep <sayı>` işareti kullanır; yalnız kendi süreçlerini sayar ve öldürür.
//! stdin her zaman /dev/null: terminal yok → ön plan devri devreye girmez.

use std::fs;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU32, Ordering};
use std::thread::sleep;
use std::time::{Duration, Instant};

const BEKCI: &str = env!("CARGO_BIN_EXE_ajan-bekci");
static SAYAC: AtomicU32 = AtomicU32::new(0);

fn isaret() -> String {
    let n = SAYAC.fetch_add(1, Ordering::SeqCst);
    format!("9{:03}{:02}1", std::process::id() % 1000, n)
}

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
        if stat.rsplit(')').next().and_then(|s| s.split_whitespace().next()) != Some("Z") {
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
        sleep(Duration::from_millis(50));
    }
    kosul()
}

fn bekci(arg: &[&str], betik: &str) -> Child {
    Command::new(BEKCI)
        .args(["--kafes", "yok"])
        .args(arg)
        .args(["--", "bash", "-c", betik])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap()
}

fn sinyal(pid: i32, s: &str) {
    Command::new("kill").args([&format!("-{s}"), &pid.to_string()]).status().unwrap();
}

struct Supurge(String);
impl Drop for Supurge {
    fn drop(&mut self) {
        for p in uyuyanlar(&self.0) {
            sinyal(p, "KILL");
        }
    }
}

fn bitis(mut c: Child, tavan: Duration) -> Option<i32> {
    let bas = Instant::now();
    while bas.elapsed() < tavan {
        if let Ok(Some(d)) = c.try_wait() {
            return d.code();
        }
        sleep(Duration::from_millis(20));
    }
    let _ = c.kill();
    None
}

#[test]
fn cikis_kodu_aynen_doner() {
    assert_eq!(bitis(bekci(&[], "exit 7"), Duration::from_secs(10)), Some(7));
}

#[test]
fn baslatilamayan_komut_127() {
    let c = Command::new(BEKCI).args(["--kafes", "yok", "--", "/yok/boyle-bir-komut"]).stdin(Stdio::null()).spawn().unwrap();
    assert_eq!(bitis(c, Duration::from_secs(10)), Some(127));
}

#[test]
fn hatali_kullanim_2() {
    for a in [&["--"][..], &["--grup", "c", "--", "true"], &["--bilinmeyen"]] {
        let c = Command::new(BEKCI).args(a).stdin(Stdio::null()).stderr(Stdio::null()).spawn().unwrap();
        assert_eq!(bitis(c, Duration::from_secs(10)), Some(2), "{a:?}");
    }
}

#[test]
fn ajan_bekci_pid_cocuga_gecer() {
    let c = bekci(&[], "echo $AJAN_BEKCI_PID $PPID");
    let pid = c.id().to_string();
    let cikti = c.wait_with_output().unwrap();
    let s = String::from_utf8_lossy(&cikti.stdout);
    let alan: Vec<&str> = s.split_whitespace().collect();
    assert_eq!(alan, [pid.as_str(), pid.as_str()]);
}

#[test]
fn ajan_cikinca_setsid_torunu_supurulur() {
    let i = isaret();
    let _s = Supurge(i.clone());
    let c = bekci(&[], &format!("setsid -f sleep {i}; sleep 0.3; exit 0"));
    assert_eq!(bitis(c, Duration::from_secs(10)), Some(0));
    assert!(uyuyanlar(&i).is_empty(), "kaçan torun süpürülmeli");
}

#[test]
fn sigterm_setsidli_torunu_da_kapatir() {
    let i = isaret();
    let _s = Supurge(i.clone());
    let c = bekci(&["--grup", "b"], &format!("setsid sleep {i} & sleep {i} & wait"));
    assert!(bekle_ki(Duration::from_secs(5), || uyuyanlar(&i).len() == 2));
    sinyal(c.id() as i32, "TERM");
    assert!(bitis(c, Duration::from_secs(10)).is_some());
    assert!(uyuyanlar(&i).is_empty());
}

#[test]
fn sighup_kapanis_sayilir() {
    let i = isaret();
    let _s = Supurge(i.clone());
    let c = bekci(&[], &format!("sleep {i}"));
    assert!(bekle_ki(Duration::from_secs(5), || uyuyanlar(&i).len() == 1));
    sinyal(c.id() as i32, "HUP");
    assert!(bitis(c, Duration::from_secs(10)).is_some());
    assert!(uyuyanlar(&i).is_empty());
}

#[test]
fn sigint_sarmalayiciyi_kapatmaz() {
    let i = isaret();
    let _s = Supurge(i.clone());
    let mut c = bekci(&["--grup", "b"], &format!("sleep {i}"));
    assert!(bekle_ki(Duration::from_secs(5), || uyuyanlar(&i).len() == 1));
    sinyal(c.id() as i32, "INT");
    sleep(Duration::from_millis(700));
    assert!(c.try_wait().unwrap().is_none(), "SIGINT sarmalayıcıyı kapatmamalı");
    assert_eq!(uyuyanlar(&i).len(), 1, "SIGINT ajanı kapatmamalı (sarmalayıcıdan)");
    sinyal(c.id() as i32, "TERM");
    assert!(bitis(c, Duration::from_secs(10)).is_some());
}

#[test]
fn sigtermi_yok_sayan_ajan_bekle_sonrasi_sigkill() {
    let i = isaret();
    let _s = Supurge(i.clone());
    let c = bekci(&["--bekle", "0.5"], &format!("trap '' TERM; while true; do sleep {i}; done"));
    assert!(bekle_ki(Duration::from_secs(5), || uyuyanlar(&i).len() == 1));
    let bas = Instant::now();
    sinyal(c.id() as i32, "TERM");
    assert!(bitis(c, Duration::from_secs(10)).is_some());
    assert!(bas.elapsed() < Duration::from_secs(5));
    assert!(uyuyanlar(&i).is_empty());
}

#[test]
fn ebeveyn_sigkill_yerse_agac_temizlenir() {
    let i = isaret();
    let _s = Supurge(i.clone());
    // ara kabuk → bekçi → ajan(setsid torunlu). Ara kabuk SIGKILL → pdeathsig ile bekçiye SIGTERM.
    let mut ara = Command::new("bash")
        .args(["-c", &format!("{BEKCI} --kafes yok -- bash -c 'setsid -f sleep {i}; sleep {i}' < /dev/null & wait")])
        .stdin(Stdio::null())
        .spawn()
        .unwrap();
    assert!(bekle_ki(Duration::from_secs(5), || uyuyanlar(&i).len() == 2));
    sinyal(ara.id() as i32, "KILL");
    let _ = ara.wait();
    assert!(bekle_ki(Duration::from_secs(8), || uyuyanlar(&i).is_empty()), "pdeathsig sonrası ağaç temizlenmeli");
}

#[test]
fn cgroup_kafesi_scope_icinde_calistirir() {
    let rt = std::env::var_os("XDG_RUNTIME_DIR").map(std::path::PathBuf::from);
    if !rt.is_some_and(|d| d.join("bus").exists()) {
        eprintln!("systemd kullanıcı veriyolu yok — atlandı");
        return;
    }
    let birim = format!("ajan-bekci-test-{}-{}", std::process::id(), SAYAC.fetch_add(1, Ordering::SeqCst));
    let c = Command::new(BEKCI)
        .args(["--birim", &birim, "--", "bash", "-c", "cat /proc/self/cgroup; echo $AJAN_BEKCI_PID"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let pid = c.id().to_string();
    let cikti = c.wait_with_output().unwrap();
    let s = String::from_utf8_lossy(&cikti.stdout);
    assert!(s.contains(&format!("/{birim}.scope")), "ajan scope'ta olmalı: {s}");
    assert!(s.trim_end().ends_with(&pid), "systemd-run exec → PID değişmemeli: {s}");
}
