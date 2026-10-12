#![cfg(target_os = "linux")]
//! `tara` sahte senaryo (kabul ölçütü): 10 sahte kaçak 10/10 bulunur, 3 kontrol çıkmaz.
//! - K1 ×4: 2 çıplak `systemd-run --user --scope --unit=ajan-bekci-…` + 2 gerçek bekçi kill -9 yedi
//!   (setsid torunu bekçisiz scope'ta kaldı — ölçümdeki S4 senaryosunun aynısı).
//! - K2 ×6: 4 ölü `CLAUDE_PID` + 2 ölü `AJAN_BEKCI_PID` taşıyan `sleep`.
//! - Kontrol: canlı `CLAUDE_PID` (test sürecinin kendisi), canlı bekçinin scope'u, işaretsiz sleep.
//!
//! Testler yalnız kendi işaretli `sleep <sayı>`larına ve kendi açtığı bekçiye sinyal yollar; `tara`
//! kendisi hiçbir sinyal göndermez. systemd kullanıcı veriyolu yoksa K1 kısmı atlanır (yazılır).

use std::fs;
use std::process::{Child, Command, Stdio};
use std::thread::sleep;
use std::time::{Duration, Instant};

const BEKCI: &str = env!("CARGO_BIN_EXE_ajan-bekci");

fn isaret(n: u32) -> String {
    format!("8{:05}{:02}1", std::process::id() % 100_000, n)
}

fn uyuyanlar(isaret: &str) -> Vec<i32> {
    let hedef = format!("sleep\0{isaret}\0");
    fs::read_dir("/proc")
        .unwrap()
        .flatten()
        .filter_map(|g| {
            let pid = g.file_name().to_string_lossy().parse::<i32>().ok()?;
            (fs::read(g.path().join("cmdline")).ok()? == hedef.as_bytes()).then_some(pid)
        })
        .collect()
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

fn veriyolu() -> bool {
    std::env::var_os("DBUS_SESSION_BUS_ADDRESS").is_some()
        || std::env::var_os("XDG_RUNTIME_DIR").is_some_and(|d| std::path::Path::new(&d).join("bus").exists())
}

/// Kendi açtığımız işaretli sleep'leri ve çocukları temizler (yalnız işaretle, yalnız bizimkiler).
struct Supurge(Vec<String>, Vec<Child>);
impl Drop for Supurge {
    fn drop(&mut self) {
        for c in &mut self.1 {
            let _ = c.kill();
            let _ = c.wait();
        }
        for i in &self.0 {
            for p in uyuyanlar(i) {
                let _ = Command::new("kill").args(["-KILL", &p.to_string()]).status();
            }
        }
    }
}

/// Başlayıp toplanmış (ölü) bir PID.
fn olu_pid() -> i32 {
    let mut c = Command::new("true").spawn().unwrap();
    let pid = c.id() as i32;
    c.wait().unwrap();
    pid
}

fn tara_json() -> (String, i32) {
    // Türkçe ortam + --lang tr: JSON yine de İngilizce olmalı (aşağıda denetlenir).
    let c = Command::new(BEKCI).args(["--lang", "tr", "tara", "--json"]).env("LANG", "tr_TR.UTF-8").output().unwrap();
    (String::from_utf8(c.stdout).unwrap(), c.status.code().unwrap_or(-1))
}

fn seviye(json: &str, pid: i32) -> Option<String> {
    let anahtar = format!("{{\"pid\":{pid},\"kanit\":\"");
    let i = json.find(&anahtar)? + anahtar.len();
    Some(json[i..i + 2].to_owned())
}

fn sleep_ortamla(i: &str, ortam: &[(&str, String)]) -> Child {
    let mut k = Command::new("sleep");
    k.arg(i).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
    k.env_remove("CLAUDE_PID").env_remove("AJAN_BEKCI_PID").env_remove("AJAN_BEKCI_KAFES");
    for (a, d) in ortam {
        k.env(a, d);
    }
    k.spawn().unwrap()
}

#[test]
fn sahte_kacaklar_10_10_bulunur_kontroller_cikmaz() {
    let mut beklenen: Vec<(i32, &str)> = Vec::new();
    let mut s = Supurge(Vec::new(), Vec::new());
    let olu = olu_pid();

    // K2: 4 ölü CLAUDE_PID + 2 ölü AJAN_BEKCI_PID.
    for n in 0..6 {
        let i = isaret(n);
        let degisken = if n < 4 { "CLAUDE_PID" } else { "AJAN_BEKCI_PID" };
        let c = sleep_ortamla(&i, &[(degisken, olu.to_string())]);
        beklenen.push((c.id() as i32, "K2"));
        s.0.push(i);
        s.1.push(c);
    }

    // Kontroller: canlı CLAUDE_PID (biz, sleep'ten önce başladık) + işaretsiz.
    let canli = sleep_ortamla(&isaret(20), &[("CLAUDE_PID", std::process::id().to_string())]);
    let isaretsiz = sleep_ortamla(&isaret(21), &[]);
    let mut kontroller = vec![canli.id() as i32, isaretsiz.id() as i32];
    s.0.extend([isaret(20), isaret(21)]);
    s.1.extend([canli, isaretsiz]);

    let k1 = veriyolu();
    if k1 {
        // K1 çıplak: scope'ta yalnız sleep, bekçi hiç yok.
        for n in 6..8 {
            let i = isaret(n);
            let c = Command::new("systemd-run")
                .args(["--user", "--scope", "--quiet", "--collect", &format!("--unit=ajan-bekci-t{i}"), "--", "sleep", &i])
                .stdin(Stdio::null())
                .spawn()
                .unwrap();
            beklenen.push((c.id() as i32, "K1")); // systemd-run exec eder → PID = sleep
            s.0.push(i);
            s.1.push(c);
        }
        // K1 gerçek: bekçi setsid torunuyla başlar, bekçiye kill -9 → torun bekçisiz scope'ta kalır.
        for n in 8..10 {
            let i = isaret(n);
            let mut b = Command::new(BEKCI)
                .args(["--birim", &format!("ajan-bekci-t{i}"), "--", "bash", "-c", &format!("setsid sleep {i} & wait")])
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .unwrap();
            assert!(bekle_ki(Duration::from_secs(5), || uyuyanlar(&i).len() == 1), "torun başlamadı");
            b.kill().unwrap(); // yalnız kendi açtığımız bekçi
            b.wait().unwrap();
            let p = uyuyanlar(&i)[0];
            let cg = fs::read_to_string(format!("/proc/{p}/cgroup")).unwrap();
            assert!(cg.contains(&format!("/ajan-bekci-t{i}.scope")), "torun bekçinin scope'unda olmalı: {cg}");
            beklenen.push((p, "K1"));
            s.0.push(i);
        }
        // Kontrol: canlı bekçinin scope'u (bekçi + sleep çocuğu).
        let i = isaret(22);
        let b = Command::new(BEKCI)
            .args(["--birim", &format!("ajan-bekci-t{i}"), "--", "sleep", &i])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        assert!(bekle_ki(Duration::from_secs(5), || uyuyanlar(&i).len() == 1));
        kontroller.push(uyuyanlar(&i)[0]);
        kontroller.push(b.id() as i32);
        s.0.push(i);
        s.1.push(b);
    } else {
        eprintln!("systemd kullanıcı veriyolu yok — K1 kısmı atlandı (yalnız 6 K2 ölçüldü)");
    }

    // systemd-run'ın scope'a geçişi bitsin.
    assert!(bekle_ki(Duration::from_secs(5), || {
        let (j, _) = tara_json();
        beklenen.iter().all(|(p, _)| seviye(&j, *p).is_some())
    }));
    assert!(fs::metadata(format!("/proc/{olu}")).is_err(), "ölü PID yeniden kullanılmış — ölçüm geçersiz");

    let (j, kod) = tara_json();
    let bulunan = beklenen.iter().filter(|(p, sv)| seviye(&j, *p).as_deref() == Some(*sv)).count();
    assert_eq!(bulunan, beklenen.len(), "bulunan {bulunan}/{} · {j}", beklenen.len());
    if k1 {
        assert_eq!(bulunan, 10);
    }
    for p in &kontroller {
        assert_eq!(seviye(&j, *p), None, "kontrol {p} çıkmamalı: {j}");
    }
    assert_eq!(kod, 3, "K1/K2 varken çıkış 3");
    assert!(j.contains("\"ayrinti\":\"CLAUDE_PID=") && j.contains(" dead"), "JSON İngilizce olmalı: {j}");
    assert!(!j.contains("ölü") && !j.contains("içinde") && !j.contains("bekçi"), "JSON'da Türkçe değer: {j}");
    eprintln!("sahte senaryo: {bulunan}/{} bulundu, {} kontrol temiz", beklenen.len(), kontroller.len());
}

#[test]
fn tara_kullanim_hatasi_2_ve_tablo() {
    let c = Command::new(BEKCI).args(["tara", "--yanlis"]).output().unwrap();
    assert_eq!(c.status.code(), Some(2));
    let tablo = |dil: &[(&str, &str)], arg: &[&str]| {
        let mut k = Command::new(BEKCI);
        k.args(arg).arg("tara").env_remove("LC_ALL").env_remove("LC_MESSAGES").env_remove("LANG");
        k.envs(dil.iter().copied());
        String::from_utf8(k.output().unwrap().stdout).unwrap()
    };
    let t = tablo(&[], &[]);
    assert!(t.starts_with("    PID  LEVEL AGENT") && t.contains("no process was signalled"), "{t}");
    let t = tablo(&[("LANG", "tr_TR.UTF-8")], &[]);
    assert!(t.starts_with("    PID  KANIT AJAN") && t.contains("hiçbir sürece sinyal gönderilmedi"), "{t}");
    let t = tablo(&[("LANG", "tr_TR.UTF-8")], &["--lang", "en"]);
    assert!(t.starts_with("    PID  LEVEL"), "{t}");
}
