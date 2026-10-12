//! ajan-bekci — `ajan-bekci [seçenekler] -- <komut> [arg…]`
//!
//! Ajanı (claude, codex …) çocuk olarak başlatır; ajan nasıl biterse bitsin (kendi çıkışı,
//! terminal kapandı = SIGHUP, SIGTERM, ajan çöktü) ağacındaki her şeyi süpürür:
//! SIGTERM → `--bekle` sn → SIGKILL. Sarmalayıcı alt-toplayıcıdır (setsid'le kopan torunlar da
//! ona bağlanır) ve ebeveyni ölünce SIGTERM alır.
//!
//! `--kafes cgroup` (varsayılan): systemd varsa kendini `systemd-run --user --scope` içinde yeniden
//! başlatır (aynı PID, exec). Sarmalayıcı SIGKILL yese bile kalan torunlar adı belli bir scope'ta
//! durur (`tara` K1 kanıtı). systemd yoksa sessizce düz kip.
//!
//! `--grup b` (varsayılan): ajan sarmalayıcının süreç grubunda kalır → Ctrl-Z ikisini birlikte durdurur,
//! kabuk `fg` ile ikisini sürdürür. `--grup a` (ajana kendi grup + terminal ön planı) duman testini geçti
//! ama iş denetimini bozdu: Ctrl-Z yalnız ajanı durdurur, sarmalayıcı beklemede kalır, kabuk geri gelmez
//! (ölçüldü: `docs/olcumler.md` O2).
//!
//! Ctrl-C (SIGINT) sarmalayıcıyı kapatmaz: ajanın işidir. Günlük yalnız `--log` dosyasına yazılır;
//! ajanın terminaline (TUI) hiçbir şey basılmaz.
//!
//! `ajan-bekci tara [--json]`: ölmüş ajanlardan kalan süreçleri kanıt seviyesiyle (K1 kesin · K2 güçlü ·
//! K3 şüpheli) listeler; hiçbir sürece sinyal göndermez. Çıkış 0 = K1/K2 yok · 3 = K1/K2 var.
//! `tara --temizle`: önce kuru listeyi basar, sonra yalnız o listedeki K1/K2'leri her sinyalden hemen önce
//! kimlik (uid + başlangıç anı + exe) ve kanıt yeniden doğrulanarak kapatır. Beyaz liste ve canlı ajan
//! ağacı plan dışı. `--kuru` sinyal göndermez.
//!
//! Çıkış kodu: ajanın kodu (sinyalle öldüyse 128+n) · 2 kullanım · 127 başlatılamadı · 1 kafes kurulamadı.
//!
//! Dil: çıktı varsayılan İngilizce; `LC_ALL`/`LC_MESSAGES`/`LANG` `tr*` ise ya da `--lang tr` ile Türkçe
//! (`--`'dan önce herhangi bir yerde). `--json` her zaman İngilizce. `-h/--help`, `-V/--version`.

use std::ffi::OsString;
use std::fs::File;
use std::io::Write;
use std::sync::{Mutex, OnceLock};
use std::time::Duration;
use std::time::Instant;

use ajan_bekci_cekirdek::Grup;
use ajan_bekci_cekirdek::Kafes;
use ajan_bekci_cekirdek::dil::{self, Dil, m};

const SURUM: &str = env!("CARGO_PKG_VERSION");

const KULLANIM_EN: &str = "usage: ajan-bekci [options] -- <command> [args…]
       ajan-bekci tara [--json]
       ajan-bekci tara --temizle [--kuru] [--beyaz FILE] [--sadece PID,…] [--bekle SEC]";
const KULLANIM_TR: &str = "kullanım: ajan-bekci [seçenekler] -- <komut> [arg…]
          ajan-bekci tara [--json]
          ajan-bekci tara --temizle [--kuru] [--beyaz DOSYA] [--sadece PID,…] [--bekle SN]";

const YARDIM_EN: &str = "Run the agent as a child; however it ends (exit, terminal closed, crash) every process
left in its tree is stopped: SIGTERM, then SIGKILL after --bekle seconds.

options:
  --bekle SEC           grace period before SIGKILL (default 3)
  --kafes cgroup|yok    cgroup: run inside a systemd user scope when available (default)
  --grup a|b            b: agent shares the wrapper's process group (default)
  --birim NAME          scope unit name (default ajan-bekci-<pid>)
  --log FILE            append a diagnostic log to FILE (nothing is printed to the terminal)
  --lang en|tr          output language (default: from LC_ALL/LC_MESSAGES/LANG, else en)
  -h, --help            show this help
  -V, --version         show version

tara                    list leftovers of dead agents with evidence (K1 certain · K2 strong ·
                        K3 suspect); sends no signals. exit 0 = no K1/K2 · 3 = K1/K2 found
  --json                machine-readable output (always English)
  --temizle             print the dry list, then stop only those K1/K2 processes, re-checking
                        identity before every signal. exit 0 = all stopped · 4 = skipped/left
  --kuru                dry run: list only, send nothing
  --beyaz FILE          allow list (default ${XDG_CONFIG_HOME:-~/.config}/ajan-bekci/beyaz-liste)
  --sadece PID,…        act only on these PIDs

exit: agent's code (128+n if killed by a signal) · 2 usage · 127 could not start · 1 no cage";
const YARDIM_TR: &str = "Ajanı çocuk olarak başlatır; ajan nasıl biterse bitsin (çıkış, terminal kapandı, çökme)
ağacında kalan her süreç durdurulur: SIGTERM, --bekle sn sonra SIGKILL.

seçenekler:
  --bekle SN            SIGKILL öncesi bekleme (varsayılan 3)
  --kafes cgroup|yok    cgroup: varsa systemd kullanıcı scope'u içinde çalış (varsayılan)
  --grup a|b            b: ajan sarmalayıcının süreç grubunda kalır (varsayılan)
  --birim AD            scope birim adı (varsayılan ajan-bekci-<pid>)
  --log DOSYA           tanı günlüğünü DOSYA'ya ekle (terminale hiçbir şey basılmaz)
  --lang en|tr          çıktı dili (varsayılan: LC_ALL/LC_MESSAGES/LANG, yoksa en)
  -h, --help            bu yardımı göster
  -V, --version         sürümü göster

tara                    ölmüş ajanlardan kalanları kanıtıyla listeler (K1 kesin · K2 güçlü ·
                        K3 şüpheli); sinyal göndermez. çıkış 0 = K1/K2 yok · 3 = K1/K2 var
  --json                makine okunur çıktı (her zaman İngilizce)
  --temizle             kuru listeyi basar, sonra yalnız o K1/K2'leri her sinyalden önce
                        kimliği yeniden doğrulayarak durdurur. çıkış 0 = hepsi · 4 = atlanan/kalan
  --kuru                kuru çalışma: yalnız listele, sinyal yok
  --beyaz DOSYA         beyaz liste (varsayılan ${XDG_CONFIG_HOME:-~/.config}/ajan-bekci/beyaz-liste)
  --sadece PID,…        yalnız bu PID'lere uygula

çıkış: ajanın kodu (sinyalle öldüyse 128+n) · 2 kullanım · 127 başlatılamadı · 1 kafes yok";

fn kullanim() -> &'static str {
    m(KULLANIM_EN, KULLANIM_TR)
}

fn yardim() -> String {
    let baslik = m("keeps an AI agent's process tree from outliving it", "yapay zekâ ajanının süreç ağacı ajandan sonra yaşamasın");
    format!("ajan-bekci {SURUM} — {baslik}\n\n{}\n\n{}\n", kullanim(), m(YARDIM_EN, YARDIM_TR))
}

/// İlk ayrıştırma (dil + yardım), komut ayrıştırmasından önce.
#[derive(Debug, PartialEq)]
enum On {
    /// Kalan argümanlar (`--lang` çıkarılmış).
    Calis(Vec<OsString>),
    Yardim,
    Surum,
}

/// Dili seçer (ortam, sonra `--lang` ezer) ve `--`'dan önceki `--lang`/`-h`/`-V`'yi işler.
/// `--`'dan sonrası ajanın argümanıdır, dokunulmaz.
fn on_ayristir(arg: Vec<OsString>, ortam: Dil) -> Result<On, String> {
    dil::ayarla(ortam);
    let mut kalan = Vec::with_capacity(arg.len());
    let mut dil_degeri: Option<String> = None;
    let mut yardim = None;
    let mut it = arg.into_iter();
    while let Some(a) = it.next() {
        match a.to_str() {
            Some("--") => {
                kalan.push(a);
                kalan.extend(it);
                break;
            }
            Some("--lang") => {
                let Some(d) = it.next() else { return Err(m("--lang needs a value", "--lang değer ister").into()) };
                dil_degeri = Some(d.to_string_lossy().into_owned());
            }
            Some(s) if s.starts_with("--lang=") => dil_degeri = Some(s["--lang=".len()..].to_owned()),
            Some("-h" | "--help") => yardim = yardim.or(Some(On::Yardim)),
            Some("-V" | "--version") => yardim = yardim.or(Some(On::Surum)),
            _ => kalan.push(a),
        }
    }
    if let Some(d) = dil_degeri {
        let Some(secim) = dil::ayristir(&d) else {
            return Err(format!("--lang en|tr, {} {d}", m("got", "gelen")));
        };
        dil::ayarla(secim);
    }
    Ok(yardim.unwrap_or(On::Calis(kalan)))
}

/// Ortak giriş: dil + yardım/sürüm. `Some(arg)` = çalışmaya devam.
fn giris() -> Option<Vec<OsString>> {
    let ortam = dil::ortamdan(|ad| std::env::var(ad).ok());
    match on_ayristir(std::env::args_os().skip(1).collect(), ortam) {
        Ok(On::Calis(a)) => Some(a),
        Ok(On::Yardim) => {
            let _ = std::io::stdout().write_all(yardim().as_bytes());
            std::process::exit(0);
        }
        Ok(On::Surum) => {
            let _ = writeln!(std::io::stdout(), "ajan-bekci {SURUM}");
            std::process::exit(0);
        }
        Err(e) => {
            let _ = writeln!(std::io::stderr(), "ajan-bekci: {e}\n{}", kullanim());
            std::process::exit(2);
        }
    }
}
/// Kafes içinde yeniden başlatıldığımızı gösteren işaret (değeri birim adı).
#[cfg(unix)]
const KAFES_ISARETI: &str = "AJAN_BEKCI_KAFES";

static GUNLUK: OnceLock<Mutex<File>> = OnceLock::new();

fn log(satir: &str) {
    if let Some(f) = GUNLUK.get()
        && let Ok(mut f) = f.lock()
    {
        let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs_f64()).unwrap_or(0.0);
        let _ = writeln!(f, "{t:.3} [{}] {satir}", std::process::id());
    }
}

struct Ayar {
    bekle: Duration,
    kafes: bool,
    grup: Grup,
    birim: Option<String>,
    komut: Vec<OsString>,
}

fn ayristir(arg: &[OsString]) -> Result<Ayar, String> {
    let mut a = Ayar { bekle: Duration::from_secs(3), kafes: true, grup: Grup::Ortak, birim: None, komut: Vec::new() };
    let mut i = 0;
    let deger = |i: usize| arg.get(i + 1).and_then(|d| d.to_str()).ok_or_else(|| format!("{} {}", arg[i].to_string_lossy(), m("needs a value", "değer ister")));
    while i < arg.len() {
        match arg[i].to_str() {
            Some("--") => {
                a.komut = arg[i + 1..].to_vec();
                break;
            }
            Some("--bekle") => {
                a.bekle = Duration::from_secs_f64(deger(i)?.parse::<f64>().map_err(|_| m("--bekle needs a number", "--bekle sayı ister"))?);
                i += 1;
            }
            Some("--kafes") => {
                a.kafes = match deger(i)? {
                    "cgroup" => true,
                    "yok" => false,
                    d => return Err(format!("--kafes cgroup|yok, {} {d}", m("got", "gelen"))),
                };
                i += 1;
            }
            Some("--grup") => {
                a.grup = match deger(i)? {
                    "a" => Grup::KendiOnPlan,
                    "b" => Grup::Ortak,
                    d => return Err(format!("--grup a|b, {} {d}", m("got", "gelen"))),
                };
                i += 1;
            }
            Some("--birim") => {
                a.birim = Some(deger(i)?.to_owned());
                i += 1;
            }
            Some("--log") => {
                let yol = deger(i)?;
                let f = File::options().create(true).append(true).open(yol).map_err(|e| format!("--log {yol}: {e}"))?;
                let _ = GUNLUK.set(Mutex::new(f));
                i += 1;
            }
            _ => return Err(format!("{}: {}", m("unknown option", "bilinmeyen seçenek"), arg[i].to_string_lossy())),
        }
        i += 1;
    }
    if a.komut.is_empty() {
        return Err(m("no command (try --help)", "komut yok (--help'e bak)").into());
    }
    Ok(a)
}

/// systemd kullanıcı oturumu varsa kendini bir scope içinde yeniden başlat (exec: PID değişmez,
/// terminal ön planı korunur). Döndüyse düz kipteyiz.
#[cfg(unix)]
fn kafese_gir(birim: Option<&str>) {
    use std::os::unix::process::CommandExt;
    if std::env::var_os(KAFES_ISARETI).is_some() {
        return; // zaten içerideyiz
    }
    // systemd-run veriyolu bulamazsa exec'ten SONRA hata verip çıkar → ajan hiç başlamaz.
    // Bu yüzden önce veriyolu soketi aranır; yoksa düz kip.
    let veriyolu = std::env::var_os("DBUS_SESSION_BUS_ADDRESS").is_some()
        || std::env::var_os("XDG_RUNTIME_DIR").is_some_and(|d| std::path::Path::new(&d).join("bus").exists());
    let systemd_run = std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default())
        .map(|d| d.join("systemd-run"))
        .find(|y| y.is_file());
    let (true, Some(systemd_run), Ok(ben)) = (veriyolu, systemd_run, std::env::current_exe()) else {
        log(m("cage: no systemd user manager — plain mode", "kafes: systemd kullanıcı yöneticisi yok — düz kip"));
        return;
    };
    let birim = birim.map_or_else(|| format!("ajan-bekci-{}", std::process::id()), str::to_owned);
    let hata = std::process::Command::new(systemd_run)
        .args(["--user", "--scope", "--quiet", "--collect", &format!("--unit={birim}"), "--"])
        .arg(ben)
        .args(std::env::args_os().skip(1))
        .env(KAFES_ISARETI, &birim)
        .exec();
    log(&format!("{} ({hata}) — {}", m("cage: systemd-run failed", "kafes: systemd-run başlatılamadı"), m("plain mode", "düz kip")));
}

/// `tara [--json]` — yalnız rapor. Çıkış 0 = K1/K2 yok, 3 = var, 2 = kullanım.
/// `tara --temizle [--kuru] [--beyaz DOSYA] [--sadece PID,…] [--bekle SN]` — önce kuru listeyi basar,
/// `--kuru` değilse yalnız o listeyi her sinyalden önce yeniden doğrulayarak uygular.
/// Çıkış 0 = her hedef kapandı (ya da plan boş) · 4 = atlanan/kalan var · 2 = kullanım.
fn tara_komutu(arg: &[OsString]) -> i32 {
    let hata = |e: String| {
        let _ = writeln!(std::io::stderr(), "ajan-bekci: tara: {e}\n{}", kullanim());
        2
    };
    let (mut json, mut temizle, mut kuru) = (false, false, false);
    let mut beyaz: Option<String> = None;
    let mut sadece: Option<Vec<i32>> = None;
    let mut bekle = Duration::from_secs(3);
    let mut i = 0;
    while i < arg.len() {
        let deger = || arg.get(i + 1).and_then(|d| d.to_str()).map(str::to_owned);
        match arg[i].to_str() {
            Some("--json") => json = true,
            Some("--temizle") => temizle = true,
            Some("--kuru") => kuru = true,
            Some("--beyaz") => {
                let Some(d) = deger() else { return hata(m("--beyaz needs a file", "--beyaz dosya ister").into()) };
                beyaz = Some(d);
                i += 1;
            }
            Some("--sadece") => {
                let p: Result<Vec<i32>, _> = deger().unwrap_or_default().split(',').map(|x| x.trim().parse::<i32>()).collect();
                let Ok(p) = p else { return hata(m("--sadece needs PID,PID…", "--sadece PID,PID… ister").into()) };
                sadece = Some(p);
                i += 1;
            }
            Some("--bekle") => {
                let Some(b) = deger().and_then(|d| d.parse::<f64>().ok()).filter(|b| *b >= 0.0) else {
                    return hata(m("--bekle needs a number", "--bekle sayı ister").into());
                };
                bekle = Duration::from_secs_f64(b);
                i += 1;
            }
            _ => return hata(format!("{}: {}", m("unknown option", "bilinmeyen seçenek"), arg[i].to_string_lossy())),
        }
        i += 1;
    }
    if !temizle && (kuru || beyaz.is_some() || sadece.is_some()) {
        return hata(m("--kuru/--beyaz/--sadece only with --temizle", "--kuru/--beyaz/--sadece yalnız --temizle ile").into());
    }
    if temizle && json {
        return hata(m("--temizle cannot be combined with --json", "--temizle --json ile birlikte kullanılmaz").into());
    }
    if json {
        dil::ayarla(Dil::En); // JSON değerleri dilden bağımsız
    }
    #[cfg(target_os = "linux")]
    {
        use ajan_bekci_cekirdek::{tara, temizle as tm};
        if !temizle {
            let bulgular = tara::tara();
            let metin = if json { tara::json(&bulgular) + "\n" } else { tara::tablo(&bulgular) };
            let _ = std::io::stdout().write_all(metin.as_bytes());
            return if bulgular.iter().any(|b| b.kanit.seviye() != "K3") { 3 } else { 0 };
        }
        // Beyaz liste: --beyaz ya da $XDG_CONFIG_HOME|~/.config/ajan-bekci/beyaz-liste (yoksa boş).
        let yol = beyaz.map(std::path::PathBuf::from).or_else(|| {
            let kok = std::env::var_os("XDG_CONFIG_HOME")
                .map(std::path::PathBuf::from)
                .or_else(|| std::env::var_os("HOME").map(|h| std::path::Path::new(&h).join(".config")))?;
            Some(kok.join("ajan-bekci/beyaz-liste"))
        });
        let (liste_metni, yol_metni) = match &yol {
            Some(y) if y.exists() => match std::fs::read_to_string(y) {
                Ok(m) => (m, y.display().to_string()),
                Err(e) => return hata(format!("{} {}: {e}", m("cannot read allow list", "beyaz liste okunamadı"), y.display())),
            },
            Some(y) => (String::new(), format!("{} {}", y.display(), m("(missing — empty)", "(yok — boş)"))),
            None => (String::new(), m("none", "yok").into()),
        };
        let beyaz = match tm::BeyazListe::ayristir(&liste_metni) {
            Ok(b) => b,
            Err(e) => return hata(e),
        };
        let (liste, bulgular, uid) = tara::anlik();
        let (plan, disarida) = tm::planla(&bulgular, &liste, &beyaz, sadece.as_deref());
        let n = plan.len();
        let mut o = match dil::simdi() {
            Dil::En => format!("DRY LIST — {n} targets (K1/K2 only, uid {uid}) · allow list {yol_metni}\n"),
            Dil::Tr => format!("KURU LİSTE — {n} hedef (yalnız K1/K2, uid {uid}) · beyaz liste {yol_metni}\n"),
        };
        let hedef = m("TARGET", "HEDEF");
        for h in &plan {
            o.push_str(&format!("{hedef} {} {} {} · {}\n", h.pid, h.seviye, h.komut, h.ayrinti));
        }
        let disarida_ad = m("OUTSIDE", "DIŞARIDA");
        for (p, neden) in &disarida {
            o.push_str(&format!("{disarida_ad} {p} {neden}\n"));
        }
        let _ = std::io::stdout().write_all(o.as_bytes());
        if kuru {
            let _ = writeln!(std::io::stdout(), "--kuru: {}", m("no process was signalled", "hiçbir sürece sinyal gönderilmedi"));
            return 0;
        }
        let mut sistem = tm::LinuxSistem { beyaz };
        let sonuc = tm::uygula(&plan, uid, &mut sistem, bekle);
        let mut o = String::new();
        let mut temiz = true;
        for (p, d) in &sonuc {
            match d {
                tm::Sonuc::Kapandi(s) => o.push_str(&format!("{} {p} {s}\n", m("STOPPED", "KAPANDI"))),
                tm::Sonuc::Atlandi(e) => {
                    temiz = false;
                    o.push_str(&format!("{} {p} {e}\n", m("SKIPPED", "ATLANDI")));
                }
                tm::Sonuc::Kaldi => {
                    temiz = false;
                    o.push_str(&format!("{} {p}\n", m("LEFT", "KALDI")));
                }
            }
        }
        let _ = std::io::stdout().write_all(o.as_bytes());
        if temiz { 0 } else { 4 }
    }
    #[cfg(windows)]
    {
        // Windows v0.1: yalnız K3 (ebeveyni ölü + ajan izli ad), yalnız rapor. Başka sürecin ortamı
        // PEB okuması ister → K2 ve --temizle v0.2.
        let _ = (kuru, beyaz, sadece, bekle);
        if temizle {
            let _ = writeln!(
                std::io::stderr(),
                "ajan-bekci: {}",
                m(
                    "tara --temizle is not available on Windows yet (v0.2); Windows reports K3 only",
                    "tara --temizle Windows'ta henüz yok (v0.2); Windows'ta yalnız K3 rapor"
                )
            );
            return 1;
        }
        let bulgular = ajan_bekci_cekirdek::tara_k3();
        let mut o = String::new();
        if json {
            let satir: Vec<String> = bulgular
                .iter()
                .map(|b| {
                    format!(
                        "{{\"pid\":{},\"kanit\":\"K3\",\"ayrinti\":\"{}\",\"ajan\":\"?\",\"yas_sn\":{},\"rss_kb\":0,\"komut\":\"{}\"}}",
                        b.pid,
                        b.neden.replace('\\', "\\\\").replace('"', "\\\""),
                        b.yas_sn,
                        b.ad.replace('\\', "\\\\").replace('"', "\\\"")
                    )
                })
                .collect();
            o = format!("[{}]\n", satir.join(",\n "));
        } else {
            o.push_str(&format!("{:>7}  {:<5} {:>7}  {}\n", "PID", m("LEVEL", "KANIT"), m("AGE(s)", "YAŞ(sn)"), m("NAME · detail", "AD · ayrıntı")));
            for b in &bulgular {
                o.push_str(&format!("{:>7}  K3    {:>7}  {} · {}\n", b.pid, b.yas_sn, b.ad, b.neden));
            }
            let n = bulgular.len();
            o.push_str(&match dil::simdi() {
                Dil::En => format!("K3 suspect {n} (Windows v0.1: K3 only, never cleaned) · no process was touched\n"),
                Dil::Tr => format!("K3 şüpheli {n} (Windows v0.1: yalnız K3, asla temizlenmez) · hiçbir sürece dokunulmadı\n"),
            });
        }
        let _ = std::io::stdout().write_all(o.as_bytes());
        0
    }
    #[cfg(not(any(target_os = "linux", windows)))]
    {
        let _ = (json, temizle, kuru, beyaz, sadece, bekle);
        let _ = writeln!(std::io::stderr(), "ajan-bekci: {}", m("tara is not available on this platform", "tara bu platformda yok"));
        1
    }
}

#[cfg(unix)]
fn main() {
    let Some(arg) = giris() else { return };
    if arg.first().is_some_and(|a| a == "tara") {
        std::process::exit(tara_komutu(&arg[1..]));
    }
    let ayar = match ayristir(&arg) {
        Ok(a) => a,
        Err(e) => {
            // Kullanım hatası terminal başlamadan önce: stderr'e yazmak güvenli.
            let _ = writeln!(std::io::stderr(), "ajan-bekci: {e}\n{}", kullanim());
            std::process::exit(2);
        }
    };
    if ayar.kafes {
        kafese_gir(ayar.birim.as_deref());
    }
    let cgroup = std::fs::read_to_string("/proc/self/cgroup").unwrap_or_default();
    log(&format!("{} · kafes {} · cgroup {}", m("started", "başladı"), std::env::var(KAFES_ISARETI).unwrap_or_else(|_| "-".into()), cgroup.trim()));
    std::process::exit(calis(&ayar));
}

fn calis(ayar: &Ayar) -> i32 {
    let mut k = match Kafes::kur(log) {
        Ok(k) => k,
        Err(e) => {
            log(&format!("{}: {e}", m("could not set up cage", "kafes kurulamadı")));
            return 1;
        }
    };
    let ben: OsString = std::process::id().to_string().into();
    if let Err(kod) = k.baslat(&ayar.komut, &[("AJAN_BEKCI_PID".into(), ben)], ayar.grup) {
        return i32::from(kod);
    }
    let mut kod = None;
    let mut sebep = m("agent exited", "ajan çıktı");
    loop {
        if let Some(c) = k.topla() {
            kod = Some(c);
            break;
        }
        if k.kapanis_istekleri() > 0 || k.ebeveyn_yok() {
            sebep = m("shutdown request", "kapanış isteği");
            break;
        }
        k.uyu(Some(Duration::from_millis(500)));
    }
    // Ağaç süpürme: ajan çıktıysa geride kalanlar (setsid torunları, MCP çocukları), istek geldiyse
    // ajan dahil hepsi. SIGTERM → --bekle → SIGKILL.
    let kalan = k.kalan();
    log(&format!("{sebep}; {} {kalan}", m("processes left in tree:", "ağaçta kalan süreç:")));
    if kalan > 0 {
        k.kapanisi_baslat();
        let son = Instant::now() + ayar.bekle;
        while Instant::now() < son {
            kod = kod.or(k.topla());
            if k.kalan() == 0 {
                break;
            }
            k.uyu(Some(Duration::from_millis(50)));
        }
        if k.kalan() > 0 {
            k.oldur();
            let son = Instant::now() + Duration::from_secs(2);
            while Instant::now() < son && k.kalan() > 0 {
                k.topla();
                std::thread::sleep(Duration::from_millis(20));
            }
        }
    }
    kod = kod.or(k.topla());
    k.on_plani_geri_al();
    log(&format!("{} {kod:?} · {} {}", m("done · agent code", "bitti · ajan kodu"), m("left", "kalan"), k.kalan()));
    kod.map_or(1, i32::from)
}

/// Windows: Job Object kafesi (KILL_ON_JOB_CLOSE; askıda başlat → işe koy → sür), konsol
/// devralınır, Ctrl-C/Ctrl-Break yok sayılır, ebeveyn tanıtıcısı izlenir. `--kafes`/`--grup`/`--birim`
/// Windows'ta etkisiz (iş her zaman kurulur; süreç grubu yok).
#[cfg(windows)]
fn main() {
    let Some(arg) = giris() else { return };
    if arg.first().is_some_and(|a| a == "tara") {
        std::process::exit(tara_komutu(&arg[1..]));
    }
    let ayar = match ayristir(&arg) {
        Ok(a) => a,
        Err(e) => {
            let _ = writeln!(std::io::stderr(), "ajan-bekci: {e}\n{}", kullanim());
            std::process::exit(2);
        }
    };
    let _ = (ayar.kafes, &ayar.birim);
    log(m("started · Job Object", "başladı · Job Object"));
    std::process::exit(calis(&ayar));
}
