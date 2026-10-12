#![cfg(target_os = "linux")]
//! `tara --temizle` sahte senaryo: kuru liste ↔ gerçek temizlik farkı 0, yanlış öldürme 0.
//!
//! 🔴 Her `--temizle` çağrısı `--sadece <bu testin işaretli PID'leri>` ile koşar: makinedeki gerçek bir
//! kalıntı plana hiç giremez (testler gerçek makinede gerçek süreç öldürmez). Sahteler
//! `setsid -f` ile PID 1'e bağlanır (canlı ajan ağacının dışı). Temizlik yalnız kendi işaretli `sleep`'leri.
//!
//! Hedefler: K2 ×3 (ölü CLAUDE_PID) · K2 SIGTERM'i yok sayan ×1 (SIGKILL beklenir) · K1 ×2 (bekçisiz scope).
//! Kontroller (ölmemeli): beyaz listede K2 · canlı bekçinin ağacında K2 · `--sadece` dışı K2 · işaretsiz öksüz.

use std::fs;
use std::process::{Child, Command, Stdio};
use std::thread::sleep;
use std::time::{Duration, Instant};

const BEKCI: &str = env!("CARGO_BIN_EXE_ajan-bekci");

fn isaret(n: u32) -> String {
    format!("7{:05}{:02}1", std::process::id() % 100_000, n)
}

fn uyuyanlar(isaret: &str) -> Vec<i32> {
    // systemd-run komutu tam yola çevirir: `/usr/bin/sleep <i>` da bizimdir.
    let hedefler = [format!("sleep\0{isaret}\0"), format!("/usr/bin/sleep\0{isaret}\0")];
    fs::read_dir("/proc")
        .unwrap()
        .flatten()
        .filter_map(|g| {
            let pid = g.file_name().to_string_lossy().parse::<i32>().ok()?;
            let cmd = fs::read(g.path().join("cmdline")).ok()?;
            if !hedefler.iter().any(|h| cmd == h.as_bytes()) {
                return None;
            }
            let stat = fs::read_to_string(g.path().join("stat")).ok()?;
            (stat.rsplit(')').next()?.split_whitespace().next()? != "Z").then_some(pid)
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

struct Supurge(Vec<String>, Vec<Child>, Option<std::path::PathBuf>);
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
        if let Some(y) = &self.2 {
            let _ = fs::remove_file(y);
        }
    }
}

fn olu_pid() -> i32 {
    let mut c = Command::new("true").spawn().unwrap();
    let pid = c.id() as i32;
    c.wait().unwrap();
    pid
}

/// `setsid -f env -u … CLAUDE_PID=<olu> [bash -c 'trap "" TERM; exec'] sleep <i>` → PID 1'e bağlı öksüz.
fn oksuz(i: &str, claude_pid: Option<i32>, inatci: bool) -> i32 {
    let mut k = Command::new("setsid");
    k.args(["-f", "env", "-u", "CLAUDE_PID", "-u", "AJAN_BEKCI_PID", "-u", "AJAN_BEKCI_KAFES"]);
    if let Some(p) = claude_pid {
        k.arg(format!("CLAUDE_PID={p}"));
    }
    if inatci {
        k.args(["bash", "-c", &format!("trap '' TERM; exec sleep {i}")]);
    } else {
        k.args(["sleep", i]);
    }
    k.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).status().unwrap();
    assert!(bekle_ki(Duration::from_secs(5), || uyuyanlar(i).len() == 1), "{i} başlamadı");
    uyuyanlar(i)[0]
}

fn temizle(arg: &[&str]) -> (String, i32) {
    // Satır önekleri dile bağlı: Türkçe sabitlenir (İngilizcesi aşağıda ayrıca denetlenir).
    let c = Command::new(BEKCI).args(["--lang", "tr", "tara", "--temizle"]).args(arg).output().unwrap();
    (String::from_utf8(c.stdout).unwrap(), c.status.code().unwrap_or(-1))
}

fn satirlar(metin: &str, onek: &str) -> Vec<i32> {
    let mut v: Vec<i32> = metin
        .lines()
        .filter_map(|l| l.strip_prefix(onek)?.split_whitespace().next()?.parse().ok())
        .collect();
    v.sort();
    v
}

#[test]
fn kuru_ve_gercek_fark_0_yanlis_oldurme_0() {
    let mut s = Supurge(Vec::new(), Vec::new(), None);
    let olu = olu_pid();
    let mut hedef = Vec::new();
    let mut inatci_pid = 0;

    for n in 0..4 {
        let i = isaret(n);
        s.0.push(i.clone());
        let p = oksuz(&i, Some(olu), n == 3);
        if n == 3 {
            inatci_pid = p;
        }
        hedef.push(p);
    }
    if veriyolu() {
        for n in 4..6 {
            let i = isaret(n);
            s.0.push(i.clone());
            Command::new("setsid")
                .args(["-f", "systemd-run", "--user", "--scope", "--quiet", "--collect", &format!("--unit=ajan-bekci-t{i}"), "--", "sleep", &i])
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .unwrap();
            assert!(bekle_ki(Duration::from_secs(5), || uyuyanlar(&i).len() == 1), "scope sleep başlamadı");
            hedef.push(uyuyanlar(&i)[0]);
        }
    } else {
        eprintln!("systemd kullanıcı veriyolu yok — K1 hedefleri atlandı");
    }

    // Kontroller.
    let beyaz_i = isaret(10);
    s.0.push(beyaz_i.clone());
    let beyaz_pid = oksuz(&beyaz_i, Some(olu), false);
    let disari_i = isaret(11);
    s.0.push(disari_i.clone());
    let disari_pid = oksuz(&disari_i, Some(olu), false); // --sadece'ye VERİLMEZ
    let isaretsiz_i = isaret(12);
    s.0.push(isaretsiz_i.clone());
    let isaretsiz_pid = oksuz(&isaretsiz_i, None, false);
    let agac_i = isaret(13);
    s.0.push(agac_i.clone());
    let b = Command::new(BEKCI)
        .args(["--kafes", "yok", "--", "env", &format!("CLAUDE_PID={olu}"), "sleep", &agac_i])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    s.1.push(b);
    assert!(bekle_ki(Duration::from_secs(5), || uyuyanlar(&agac_i).len() == 1));
    let agac_pid = uyuyanlar(&agac_i)[0];

    let beyaz_yol = std::env::temp_dir().join(format!("ajan-bekci-beyaz-{}", std::process::id()));
    fs::write(&beyaz_yol, format!("# test\nkomut sleep {beyaz_i}\n")).unwrap();
    s.2 = Some(beyaz_yol.clone());

    let mut sadece: Vec<i32> = hedef.clone();
    sadece.extend([beyaz_pid, isaretsiz_pid, agac_pid]);
    let sadece = sadece.iter().map(|p| p.to_string()).collect::<Vec<_>>().join(",");
    let beyaz = beyaz_yol.to_str().unwrap();
    assert!(fs::metadata(format!("/proc/{olu}")).is_err(), "ölü PID yeniden kullanılmış — ölçüm geçersiz");

    // systemd-run'ın scope'a geçişini bekle: kuru liste bütün hedefleri görene kadar.
    let mut kuru = String::new();
    assert!(bekle_ki(Duration::from_secs(5), || {
        kuru = temizle(&["--kuru", "--beyaz", beyaz, "--sadece", &sadece]).0;
        satirlar(&kuru, "HEDEF ").len() == hedef.len()
    }), "{kuru}");
    let mut beklenen = hedef.clone();
    beklenen.sort();
    assert_eq!(satirlar(&kuru, "HEDEF "), beklenen, "{kuru}");
    assert!(kuru.contains("hiçbir sürece sinyal gönderilmedi"));
    assert_eq!(satirlar(&kuru, "DIŞARIDA "), { let mut v = vec![beyaz_pid, agac_pid]; v.sort(); v }, "{kuru}");
    assert!(hedef.iter().all(|p| fs::metadata(format!("/proc/{p}")).is_ok()), "--kuru hiçbir şeyi öldürmemeli");
    // Aynı kuru plan İngilizce (son --lang kazanır).
    let en = temizle(&["--lang", "en", "--kuru", "--beyaz", beyaz, "--sadece", &sadece]).0;
    assert!(en.starts_with("DRY LIST — ") && en.contains("no process was signalled"), "{en}");
    assert_eq!(satirlar(&en, "TARGET "), beklenen, "{en}");
    assert_eq!(satirlar(&en, "OUTSIDE ").len(), 2, "{en}");

    let (gercek, kod) = temizle(&["--beyaz", beyaz, "--sadece", &sadece, "--bekle", "1"]);
    assert_eq!(satirlar(&gercek, "HEDEF "), satirlar(&kuru, "HEDEF "), "iki koşunun planı aynı olmalı");
    assert_eq!(satirlar(&gercek, "KAPANDI "), beklenen, "kuru ↔ gerçek fark 0 olmalı: {gercek}");
    assert!(!gercek.contains("ATLANDI") && !gercek.contains("KALDI"), "{gercek}");
    assert!(gercek.contains(&format!("KAPANDI {inatci_pid} SIGKILL")), "TERM'i yok sayan SIGKILL yemeli: {gercek}");
    assert_eq!(kod, 0);
    assert!(bekle_ki(Duration::from_secs(3), || s.0[..hedef.len()].iter().all(|i| uyuyanlar(i).is_empty())));

    // Yanlış öldürme 0: kontrollerin hepsi yaşıyor.
    for (ad, i) in [("beyaz", &beyaz_i), ("--sadece dışı", &disari_i), ("işaretsiz", &isaretsiz_i), ("canlı bekçi ağacı", &agac_i)] {
        assert_eq!(uyuyanlar(i).len(), 1, "kontrol öldü: {ad}");
    }
    let _ = disari_pid;
    eprintln!("temizle sahte: {} hedef kuru = gerçek, 4 kontrol yaşıyor", hedef.len());
}

#[test]
fn temizle_kullanim_hatalari_2() {
    for a in [&["tara", "--kuru"][..], &["tara", "--temizle", "--json"], &["tara", "--temizle", "--sadece", "x"], &["tara", "--temizle", "--bekle"]] {
        let c = Command::new(BEKCI).args(a).output().unwrap();
        assert_eq!(c.status.code(), Some(2), "{a:?}");
    }
    // Bozuk beyaz liste → 2, hiçbir şey yapılmaz.
    let y = std::env::temp_dir().join(format!("ajan-bekci-bozuk-{}", std::process::id()));
    fs::write(&y, "python3\n").unwrap();
    let c = Command::new(BEKCI).args(["tara", "--temizle", "--kuru", "--beyaz", y.to_str().unwrap()]).output().unwrap();
    let _ = fs::remove_file(&y);
    assert_eq!(c.status.code(), Some(2));
}
