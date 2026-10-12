//! Linux kafesi: subreaper, pdeathsig, süreç grubu, sinyal, /proc.
//!
//! Sarmalayıcının durum makinesi platformdan bağımsızdır; bu modül ona aynı adlı
//! yöntemleri sunar (`windows.rs` ile birebir aynı yüzey).
//!
//! Sinyal işleyicide iş yapılmaz: signal-hook işleyicide yalnız borulara yazar; asıl iş
//! döngüde. Döngü bir sinyal ya da sıradaki zamanlayıcı gelene kadar uyur (boşta CPU yok).
//!
//! Terminal ön planı `Grup` ile seçilir: etkileşimli ajan terminalin ön plan
//! grubunda değilse terminalden okurken SIGTTIN ile durur. SIGINT kapanış SAYILMAZ (Ctrl-C ajanın
//! işi, claude'da "iptal"); işleyici yine kurulur ki varsayılan davranış sarmalayıcıyı öldürmesin.

use std::ffi::OsString;
use std::io::Read;
use std::os::unix::net::UnixStream;
use std::os::unix::process::CommandExt;
use std::path::PathBuf;
use std::process::Command;
use std::time::{Duration, Instant};

use nix::errno::Errno;
use nix::sys::prctl;
use nix::sys::signal::{Signal, kill, killpg};
use nix::sys::wait::{WaitPidFlag, WaitStatus, waitpid};
use nix::unistd::{Pid, getppid, setpgid};
use signal_hook::consts::{SIGCHLD, SIGHUP, SIGINT, SIGTERM};
use signal_hook::iterator::Signals;

use nix::libc;

use crate::{Grup, Gunluk};
use crate::dil::m;
use crate::agac::{self, canli_torunlar};

/// Uyandırıcı kurulamazsa döngünün düştüğü en kısa uyku.
const ADIM: Duration = Duration::from_millis(50);

/// Self-pipe: her sinyal bu boruya bir bayt yazar; döngü okurken zaman aşımıyla uyur.
/// (`Signals::wait()` zaman aşımı almıyor; zamanlayıcılı bekleme için gerekli.)
struct Uyandirici(UnixStream);

impl Uyandirici {
    fn kur(sinyaller: &[i32]) -> std::io::Result<Self> {
        let (oku, yaz) = UnixStream::pair()?;
        for s in sinyaller {
            signal_hook::low_level::pipe::register(*s, yaz.try_clone()?)?;
        }
        Ok(Self(oku))
    }

    fn uyu(&mut self, sure: Option<Duration>) {
        let sure = sure.map(|d| d.max(Duration::from_millis(1)));
        if self.0.set_read_timeout(sure).is_err() {
            std::thread::sleep(ADIM);
            return;
        }
        let mut t = [0u8; 64];
        let _ = self.0.read(&mut t);
    }
}

pub struct Kafes {
    sinyaller: Signals,
    uyandirici: Uyandirici,
    ben: i32,
    cocuk: Option<Pid>,
    cocuk_kodu: Option<u8>,
    ebeveyn_yok: bool,
    grup: Grup,
    log: Gunluk,
}

impl Kafes {
    /// Sinyal işleyicileri çocuğu başlatmadan ÖNCE kurulur: erken ölen çocuğun SIGCHLD'i kaçmasın.
    pub fn kur(log: Gunluk) -> Result<Self, String> {
        let ilk_ebeveyn = getppid();
        let izlenen = [SIGTERM, SIGINT, SIGHUP, SIGCHLD];
        let sinyaller = Signals::new(izlenen).map_err(|e| format!("{}: {e}", m("could not install signal handler", "sinyal işleyicisi kurulamadı")))?;
        let uyandirici = Uyandirici::kur(&izlenen).map_err(|e| format!("{}: {e}", m("could not create wake pipe", "uyandırma borusu kurulamadı")))?;
        prctl::set_child_subreaper(true).map_err(|e| format!("{}: {e}", m("could not become subreaper", "subreaper olunamadı")))?;
        prctl::set_pdeathsig(Signal::SIGTERM).map_err(|e| format!("{}: {e}", m("could not set pdeathsig", "pdeathsig kurulamadı")))?;
        Ok(Self {
            sinyaller,
            uyandirici,
            ben: std::process::id() as i32,
            cocuk: None,
            cocuk_kodu: None,
            // prctl'dan önce ebeveyn öldüyse sinyal hiç gelmez → kendimiz bakarız.
            ebeveyn_yok: getppid() != ilk_ebeveyn,
            grup: Grup::Kendi,
            log,
        })
    }

    pub fn ebeveyn_yok(&self) -> bool {
        self.ebeveyn_yok
    }

    /// Bu turda gelen kapanış istekleri (SIGTERM/SIGHUP); her biri günlüğe. SIGINT yok sayılır.
    pub fn kapanis_istekleri(&mut self) -> usize {
        let mut n = 0;
        for s in self.sinyaller.pending().filter(|s| *s != SIGCHLD) {
            if s == SIGINT {
                (self.log)(m("SIGINT ignored (Ctrl-C belongs to the agent)", "SIGINT yok sayıldı (Ctrl-C ajanın)"));
            } else {
                (self.log)(&format!("{} {s}", m("got signal", "sinyal alındı:")));
                n += 1;
            }
        }
        n
    }

    /// Çocuğu `grup` kipinde, `ortam` değişkenleriyle başlat. Hata → çıkış kodu 127.
    pub fn baslat(&mut self, komut: &[OsString], ortam: &[(OsString, OsString)], grup: Grup) -> Result<(), u8> {
        let ben = self.ben;
        let on_plan = grup == Grup::KendiOnPlan && terminal_mi();
        if on_plan {
            // Arka plandaki sarmalayıcı ön planı geri alırken SIGTTOU ile durmasın.
            // SAFETY: tek iş parçacıklı kurulum anında işleyici değişimi.
            unsafe { libc::signal(libc::SIGTTOU, libc::SIG_IGN) };
        }
        let mut k = Command::new(&komut[0]);
        k.args(&komut[1..]).envs(ortam.iter().map(|(a, d)| (a, d)));
        // SAFETY: fork ile exec arasında yalnız async-signal-safe sistem çağrıları
        // (setpgid, tcsetpgrp, signal, prctl); bellek ayırma yok.
        unsafe {
            k.pre_exec(move || {
                if grup != Grup::Ortak {
                    setpgid(Pid::from_raw(0), Pid::from_raw(0))?;
                }
                if on_plan {
                    // Ebeveyn de aynı çağrıyı yapar: hangisi önce koşarsa koşsun ajan okumadan ön planda.
                    libc::signal(libc::SIGTTOU, libc::SIG_IGN);
                    libc::tcsetpgrp(0, libc::getpid());
                    libc::signal(libc::SIGTTOU, libc::SIG_DFL);
                }
                // Kafes SIGKILL yerse doğrudan çocuk yine de kibar bir kapanış sinyali alsın.
                prctl::set_pdeathsig(Signal::SIGTERM)?;
                Ok(())
            });
        }
        match k.spawn() {
            Ok(c) => {
                let pid = Pid::from_raw(c.id() as i32);
                if on_plan {
                    // SAFETY: 0 = stdin; çocuk kendi grubunun lideri.
                    unsafe { libc::tcsetpgrp(0, pid.as_raw()) };
                }
                self.grup = if on_plan { Grup::KendiOnPlan } else if grup == Grup::Ortak { Grup::Ortak } else { Grup::Kendi };
                (self.log)(&format!("{} {pid} (kafes {ben}, grup {:?})", m("child started", "çocuk başladı"), self.grup));
                self.cocuk = Some(pid);
                self.cocuk_kodu = None;
                Ok(())
            }
            Err(e) => {
                (self.log)(&format!("{}: {}: {e}", m("could not start", "başlatılamadı"), komut[0].to_string_lossy()));
                Err(127)
            }
        }
    }

    /// Bütün ölü çocukları (bize bağlanmış öksüzler dahil) topla — zombi birikmesin.
    /// Dönüş: şimdiki çocuğun çıkış kodu (sinyalse 128+n), henüz bitmediyse `None`.
    pub fn topla(&mut self) -> Option<u8> {
        loop {
            match waitpid(Pid::from_raw(-1), Some(WaitPidFlag::WNOHANG)) {
                Ok(WaitStatus::Exited(pid, kod)) => {
                    if Some(pid) == self.cocuk {
                        (self.log)(&format!("{} {kod}", m("child exited, code", "çocuk çıktı, kod")));
                        self.cocuk_kodu = Some(kod as u8);
                    }
                }
                Ok(WaitStatus::Signaled(pid, sinyal, _)) => {
                    if Some(pid) == self.cocuk {
                        (self.log)(&format!("{} {sinyal}", m("child killed by signal", "çocuk sinyalle öldü:")));
                        self.cocuk_kodu = Some(128u8.saturating_add(sinyal as u8));
                    }
                }
                Ok(WaitStatus::StillAlive) | Err(Errno::ECHILD) => break,
                Ok(_) | Err(Errno::EINTR) => continue,
                Err(_) => break,
            }
        }
        self.cocuk_kodu
    }

    /// (a) kipinde terminal ön planını sarmalayıcının grubuna geri ver (çocuk çıktıktan sonra).
    pub fn on_plani_geri_al(&mut self) {
        if self.grup == Grup::KendiOnPlan {
            // SAFETY: 0 = stdin; SIGTTOU `baslat`ta yok sayıldı.
            unsafe { libc::tcsetpgrp(0, libc::getpgrp()) };
        }
    }

    /// Çocuğun PID'i (başlatıldıysa).
    pub fn cocuk(&self) -> Option<i32> {
        self.cocuk.map(|p| p.as_raw())
    }

    /// Çocuğun grubuna (kendi grubundaysa) ve bütün canlı torunlara SIGTERM (kibar kapanış).
    pub fn kapanisi_baslat(&mut self) {
        if let Some(c) = self.cocuk
            && self.grup != Grup::Ortak
        {
            let _ = killpg(c, Signal::SIGTERM); // grup: ajan + aynı gruptaki çocukları
        }
        for p in canli_torunlar(self.ben) {
            let _ = kill(Pid::from_raw(p), Signal::SIGTERM); // setsid'le gruptan kaçanlar
        }
    }

    /// Ağaçta kalan canlı süreç sayısı (subreaper sayesinde setsid'liler dahil).
    pub fn kalan(&self) -> usize {
        canli_torunlar(self.ben).len()
    }

    pub fn oldur(&mut self) {
        let kalan = canli_torunlar(self.ben);
        (self.log)(&format!("{} {} — SIGKILL", kalan.len(), m("processes did not exit gracefully", "süreç zarif kapanmadı")));
        for p in &kalan {
            let _ = kill(Pid::from_raw(*p), Signal::SIGKILL);
        }
    }

    /// Bir sinyal gelene ya da `sure` dolana kadar uyu (`None` = süresiz). SIGCHLD de uyandırır.
    pub fn uyu(&mut self, sure: Option<Duration>) {
        self.uyandirici.uyu(sure);
    }
}

fn terminal_mi() -> bool {
    // SAFETY: yalnız sorgu.
    unsafe { libc::isatty(0) == 1 }
}

/// Bizim çocuğumuz olmayan bir süreci ve ağacını durdur: SIGTERM → `bekle` → SIGKILL.
/// `waitpid` yapamayız (çocuğumuz değil), kayboluşu /proc'tan izlenir. Liste baştan
/// alınır: kök ölünce torunları başka yere devredilir, ağaçtan bulunamazlar. SIGKILL'den
/// önce her PID'in exe'si yeniden doğrulanır (bu arada PID başkasına geçmiş olabilir).
/// `--temizle` bunun üstüne başlangıç anı (starttime) doğrulamasını ekler.
pub fn oksuzu_durdur(pid: i32, bekle: Duration, log: Gunluk) {
    let mut hedefler = vec![pid];
    hedefler.extend(agac::torunlar(pid, &agac::tum_surecler()).into_iter().filter(|s| !s.zombi()).map(|s| s.pid));
    let exe = |p: i32| std::fs::read_link(format!("/proc/{p}/exe")).ok();
    let kimlik: Vec<(i32, Option<PathBuf>)> = hedefler.iter().map(|&p| (p, exe(p))).collect();
    let yasiyor =
        |p: i32, e: &Option<PathBuf>| agac::tum_surecler().iter().any(|s| s.pid == p && !s.zombi()) && exe(p) == *e;
    let _ = killpg(Pid::from_raw(pid), Signal::SIGTERM); // kök kendi grubunun lideriyse grup da
    for (p, _) in &kimlik {
        let _ = kill(Pid::from_raw(*p), Signal::SIGTERM);
    }
    let son = Instant::now() + bekle;
    while Instant::now() < son {
        if !kimlik.iter().any(|(p, e)| yasiyor(*p, e)) {
            log(&format!("{} süreç SIGTERM ile kapandı", kimlik.len()));
            return;
        }
        std::thread::sleep(ADIM);
    }
    let kalan: Vec<i32> = kimlik.iter().filter(|(p, e)| yasiyor(*p, e)).map(|(p, _)| *p).collect();
    log(&format!("{} süreç {} sn'de kapanmadı — SIGKILL", kalan.len(), bekle.as_secs_f64()));
    for p in &kalan {
        let _ = kill(Pid::from_raw(*p), Signal::SIGKILL);
    }
}
