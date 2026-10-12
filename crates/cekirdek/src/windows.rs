//! Windows kafesi: Job Object ile ağaç tutma.
//!
//! Linux'taki subreaper + pdeathsig'in karşılığı **Job Object**: çocuk bir "iş"e konur,
//! onun açtığı her süreç (torunlar dahil) otomatik olarak aynı işe girer. İş
//! `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE` ile kurulur → işin son tanıtıcısı kapanınca
//! (sarmalayıcı normal çıkınca ya da Görev Yöneticisi'nden öldürülünce) bütün ağaç ölür.
//!
//! Kaynakta (kaynak depo 34d792b) gerçek Windows'ta CI'da ölçülmüştü; bu kopya henüz yalnız
//! `cargo clippy --target x86_64-pc-windows-gnu` ile denetlendi (CI'da).
//!
//! Linux'tan farklar (bilinçli):
//! - **Kapanış = `TerminateJobObject`.** Windows'ta SIGTERM yok. Kaynaktaki HTTP ile kibar
//!   kapanış (sunucuya özel) atıldı.
//! - **Konsol devralınır.** Çocuk `CREATE_NO_WINDOW` ile AÇILMAZ: etkileşimli ajanın konsolu
//!   kendisine geçmeli.
//! - **Ctrl-C / Ctrl-Break yok sayılır**: konsol olayı aynı konsoldaki her sürece gider; ajan
//!   kendisininkini alır (claude'da "iptal"). Kapanış sayılan yalnız pencere kapatma, kullanıcı çıkışı (logoff)
//!   ve sistem kapanışı (Linux'taki SIGHUP karşılığı).
//! - **Ebeveyn ölümü:** pdeathsig yok → kafes ebeveyninin süreç tanıtıcısını bekler.
//! - **Sahiplik verisi:** exe `QueryFullProcessImageNameW`, ebeveyn Toolhelp + yaratılış anı
//!   (Windows öksüzü init'e devretmez; ebeveyn PID'i ölü ya da yeniden kullanılmış olur).
//!   Karar `sahiplik.rs`'te, Linux'ta test edilir.
//! - **İç içe iş:** ebeveyn zaten bir işin içindeyse Windows 8+ iç içe işi destekler,
//!   `CREATE_BREAKAWAY_FROM_JOB` gerekmez. Eski Windows'ta `AssignProcessToJobObject`
//!   hata verir → günlüğe yazılır, çocuk öldürülür, `baslat` 1 döner.

use std::ffi::{OsStr, OsString};
use std::os::windows::ffi::OsStringExt;
use std::path::{Path, PathBuf};
use std::os::windows::io::AsRawHandle;
use std::os::windows::process::CommandExt;
use std::process::{Child, Command};
use std::ptr::{null, null_mut};
use std::sync::atomic::{AtomicPtr, Ordering};
use std::time::{Duration, Instant};

use windows_sys::Win32::Foundation::{CloseHandle, ERROR_ACCESS_DENIED, FILETIME, HANDLE, INVALID_HANDLE_VALUE, WAIT_OBJECT_0};
use windows_sys::Win32::System::Console::SetConsoleCtrlHandler;
use windows_sys::Win32::System::Diagnostics::ToolHelp::{
    CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW, TH32CS_SNAPPROCESS, TH32CS_SNAPTHREAD,
    THREADENTRY32, Thread32First, Thread32Next,
};
use windows_sys::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE, JOBOBJECT_BASIC_ACCOUNTING_INFORMATION,
    JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectBasicAccountingInformation, JobObjectExtendedLimitInformation,
    QueryInformationJobObject, SetInformationJobObject, TerminateJobObject,
};
use windows_sys::Win32::System::Threading::{
    CREATE_SUSPENDED, CreateEventW, GetCurrentProcess, GetCurrentProcessId, GetProcessTimes, INFINITE,
    OpenProcess, OpenThread, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SYNCHRONIZE, PROCESS_TERMINATE,
    QueryFullProcessImageNameW, ResetEvent, ResumeThread, SetEvent, THREAD_SUSPEND_RESUME, TerminateProcess,
    WaitForMultipleObjects, WaitForSingleObject,
};

use crate::Gunluk;
use crate::dil::m;
use crate::sahiplik::{self, Ebeveyn};

/// İşi sonlandırırken süreçlere verilen çıkış kodu (≠0 → `--yeniden` için çökme sayılır).
const SONLANDIRMA_KODU: u32 = 1;

/// Konsol olayı (Ctrl-C, Ctrl-Break, pencere kapatma, kullanıcı çıkışı) bu olayı tetikler.
/// İşleyici ayrı bir iş parçacığında koşar; yalnız `SetEvent` yapar, iş döngüde.
static KAPAT_OLAYI: AtomicPtr<core::ffi::c_void> = AtomicPtr::new(null_mut());

unsafe extern "system" fn konsol_isleyici(tur: u32) -> windows_sys::core::BOOL {
    if crate::sahiplik::konsol_olayi_yok_sayilir(tur) {
        return 1; // Ctrl-C / Ctrl-Break: ajanın işi; işlendi say, sarmalayıcı yaşasın
    }
    let olay = KAPAT_OLAYI.load(Ordering::SeqCst);
    if !olay.is_null() {
        // SAFETY: olay, `kur()`'da yaratılan ve süreç boyunca kapatılmayan geçerli bir tanıtıcı.
        unsafe { SetEvent(olay) };
    }
    1 // işlendi; varsayılan (anında çık) davranışı çalışmasın
}

/// Kapanınca tanıtıcıyı bırakan sarmal.
struct Tanitici(HANDLE);
impl Drop for Tanitici {
    fn drop(&mut self) {
        if !self.0.is_null() && self.0 != INVALID_HANDLE_VALUE {
            // SAFETY: yalnız bizim açtığımız, başka yerde kapatılmayan tanıtıcı.
            unsafe { CloseHandle(self.0) };
        }
    }
}

pub struct Kafes {
    is: Tanitici,
    kapat_olayi: HANDLE,
    ebeveyn: Option<Tanitici>,
    ebeveyn_yok: bool,
    cocuk: Option<Child>,
    cocuk_kodu: Option<u8>,
    log: Gunluk,
}

fn son_hata() -> std::io::Error {
    std::io::Error::last_os_error()
}

impl Kafes {
    pub fn kur(log: Gunluk) -> Result<Self, String> {
        // SAFETY: Win32 çağrıları; işaretçiler yerel değişkenlere, boyutlar yapılardan.
        unsafe {
            let is = CreateJobObjectW(null(), null());
            if is.is_null() {
                return Err(format!("{}: {}", m("could not create Job Object", "Job Object yaratılamadı"), son_hata()));
            }
            let is = Tanitici(is);
            let mut sinir = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
            sinir.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            if SetInformationJobObject(
                is.0,
                JobObjectExtendedLimitInformation,
                (&raw const sinir).cast(),
                size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            ) == 0
            {
                return Err(format!("{}: {}", m("could not set KILL_ON_JOB_CLOSE", "KILL_ON_JOB_CLOSE kurulamadı"), son_hata()));
            }

            let olay = CreateEventW(null(), 1, 0, null());
            if olay.is_null() {
                return Err(format!("{}: {}", m("could not create close event", "kapanış olayı yaratılamadı"), son_hata()));
            }
            KAPAT_OLAYI.store(olay, Ordering::SeqCst);
            // Konsolu olmayan süreçte de kurulabilir; başarısızlık ölümcül değil.
            if SetConsoleCtrlHandler(Some(konsol_isleyici), 1) == 0 {
                log(&format!("konsol işleyicisi kurulamadı (önemsiz): {}", son_hata()));
            }

            let ebeveyn = ebeveyni_ac();
            let ebeveyn_yok = match &ebeveyn {
                None => true,
                Some(e) => WaitForSingleObject(e.0, 0) == WAIT_OBJECT_0,
            };
            Ok(Self { is, kapat_olayi: olay, ebeveyn, ebeveyn_yok, cocuk: None, cocuk_kodu: None, log })
        }
    }

    pub fn ebeveyn_yok(&self) -> bool {
        self.ebeveyn_yok
    }

    pub fn kapanis_istekleri(&mut self) -> usize {
        let mut n = 0;
        // SAFETY: tanıtıcılar `kur()`'dan, geçerli.
        unsafe {
            if WaitForSingleObject(self.kapat_olayi, 0) == WAIT_OBJECT_0 {
                ResetEvent(self.kapat_olayi);
                (self.log)(m("console close event received", "konsol kapanış olayı alındı"));
                n += 1;
            }
            if let Some(e) = &self.ebeveyn
                && WaitForSingleObject(e.0, 0) == WAIT_OBJECT_0
            {
                (self.log)(m("parent died", "ebeveyn öldü"));
                self.ebeveyn = None; // bir kez; tanıtıcı sinyalli kalır, döngüyü meşgul etmesin
                n += 1;
            }
        }
        n
    }

    /// Çocuğu ASKIDA başlat → işe koy → sür. Askıya almadan başlatsaydık çocuk, işe girmeden
    /// önce torun açabilirdi ve o torun işin dışında kalırdı.
    pub fn baslat(&mut self, komut: &[OsString], ortam: &[(OsString, OsString)], _grup: crate::Grup) -> Result<(), u8> {
        let ben = std::process::id();
        let mut k = Command::new(&komut[0]);
        k.args(&komut[1..]).envs(ortam.iter().map(|(a, d)| (a, d))).creation_flags(CREATE_SUSPENDED);
        let cocuk = match k.spawn() {
            Ok(c) => c,
            Err(e) => {
                (self.log)(&format!("{}: {}: {e}", m("could not start", "başlatılamadı"), komut[0].to_string_lossy()));
                return Err(127);
            }
        };
        let pid = cocuk.id();
        // SAFETY: çocuk tanıtıcısı `Child` yaşadıkça geçerli.
        let atandi = unsafe { AssignProcessToJobObject(self.is.0, cocuk.as_raw_handle()) } != 0;
        if !atandi {
            let hata = son_hata();
            let mut cocuk = cocuk;
            let _ = cocuk.kill();
            let _ = cocuk.wait();
            (self.log)(&format!(
                "{pid}: {} ({hata}) — {}",
                m("could not assign child to job", "çocuk işe konamadı"),
                m("nested jobs may be unsupported (Windows < 8); child killed", "iç içe iş desteklenmiyor olabilir (Windows < 8); çocuk öldürüldü")
            ));
            return Err(1);
        }
        if let Err(e) = ana_is_parcacigini_sur(pid) {
            let mut cocuk = cocuk;
            let _ = cocuk.kill();
            let _ = cocuk.wait();
            (self.log)(&format!("{pid}: {}: {e}", m("could not resume child", "çocuk sürdürülemedi")));
            return Err(1);
        }
        (self.log)(&format!("{} {pid} (kafes {ben}, Job Object)", m("child started", "çocuk başladı")));
        self.cocuk = Some(cocuk);
        self.cocuk_kodu = None;
        Ok(())
    }

    pub fn topla(&mut self) -> Option<u8> {
        if self.cocuk_kodu.is_none()
            && let Some(c) = &mut self.cocuk
            && let Ok(Some(durum)) = c.try_wait()
        {
            let ham = durum.code().map_or(u32::MAX, |k| k as u32);
            (self.log)(&format!("{} {ham:#x}", m("child exited, code", "çocuk çıktı, kod")));
            // Windows çıkış kodu 32 bit (ör. 0xC0000005 çöküş) → 8 bite sığmayan ≠0 kalır.
            self.cocuk_kodu = Some(u8::try_from(ham).unwrap_or(255));
        }
        self.cocuk_kodu
    }

    /// Unix ile aynı yüzey: Windows'ta ön plan grubu yok.
    pub fn on_plani_geri_al(&mut self) {}

    /// Kapanış: işi sonlandır (Windows'ta kibar sinyal yok).
    pub fn kapanisi_baslat(&mut self) {
        self.sonlandir();
    }

    fn sonlandir(&mut self) {
        // SAFETY: iş tanıtıcısı geçerli.
        if unsafe { TerminateJobObject(self.is.0, SONLANDIRMA_KODU) } == 0 {
            (self.log)(&format!("TerminateJobObject: {}", son_hata()));
        }
    }

    /// İşte kalan canlı süreç sayısı.
    pub fn kalan(&self) -> usize {
        let mut bilgi = JOBOBJECT_BASIC_ACCOUNTING_INFORMATION::default();
        // SAFETY: çıktı tamponu yapının kendisi, boyutu doğru.
        let tamam = unsafe {
            QueryInformationJobObject(
                self.is.0,
                JobObjectBasicAccountingInformation,
                (&raw mut bilgi).cast(),
                size_of::<JOBOBJECT_BASIC_ACCOUNTING_INFORMATION>() as u32,
                null_mut(),
            )
        } != 0;
        if tamam { bilgi.ActiveProcesses as usize } else { 0 }
    }

    pub fn oldur(&mut self) {
        (self.log)(&format!("{} {} — TerminateJobObject", self.kalan(), m("processes did not exit", "süreç kapanmadı")));
        self.sonlandir();
    }

    /// Kapanış olayı, ebeveyn ölümü ya da çocuk çıkışı gelene kadar, en çok `sure` uyu.
    pub fn uyu(&mut self, sure: Option<Duration>) {
        let mut bekle: Vec<HANDLE> = vec![self.kapat_olayi];
        if let Some(e) = &self.ebeveyn {
            bekle.push(e.0);
        }
        // Çıkmış çocuğun tanıtıcısı hep sinyallidir → eklenirse döngü boşa döner.
        if self.cocuk_kodu.is_none()
            && let Some(c) = &self.cocuk
        {
            bekle.push(c.as_raw_handle());
        }
        let ms = sure.map_or(INFINITE, |d| d.as_millis().clamp(1, u128::from(INFINITE - 1)) as u32);
        // SAFETY: dizi yerel, tanıtıcılar bu çağrı boyunca geçerli.
        unsafe { WaitForMultipleObjects(bekle.len() as u32, bekle.as_ptr(), 0, ms) };
    }

}

/// Askıda başlatılmış sürecin (tek) iş parçacığını bul ve sürdür.
fn ana_is_parcacigini_sur(pid: u32) -> Result<(), String> {
    // SAFETY: Toolhelp anlık görüntüsü; yapı boyutu dwSize'da bildirilir.
    unsafe {
        let anlik = Tanitici(CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0));
        if anlik.0 == INVALID_HANDLE_VALUE {
            return Err(format!("{}: {}", m("could not list threads", "iş parçacığı listesi alınamadı"), son_hata()));
        }
        let mut g: THREADENTRY32 = std::mem::zeroed();
        g.dwSize = size_of::<THREADENTRY32>() as u32;
        let mut var = Thread32First(anlik.0, &mut g) != 0;
        while var {
            if g.th32OwnerProcessID == pid {
                let is_parcacigi = Tanitici(OpenThread(THREAD_SUSPEND_RESUME, 0, g.th32ThreadID));
                if is_parcacigi.0.is_null() {
                    return Err(format!("{}: {}", m("could not open thread", "iş parçacığı açılamadı"), son_hata()));
                }
                if ResumeThread(is_parcacigi.0) == u32::MAX {
                    return Err(format!("ResumeThread: {}", son_hata()));
                }
                return Ok(());
            }
            var = Thread32Next(anlik.0, &mut g) != 0;
        }
        Err(m("child thread not found", "çocuğun iş parçacığı bulunamadı").into())
    }
}

/// Ebeveynimizi bul ve beklenebilir bir tanıtıcı aç. Ebeveyn bizden önce ölmüş ve PID'i
/// başka bir sürece verilmiş olabilir → o sürecin yaratılış anı bizden SONRAYSA ebeveyn yok.
fn ebeveyni_ac() -> Option<Tanitici> {
    // SAFETY: Toolhelp + OpenProcess; yapılar sıfırlanıp dwSize bildirilir.
    unsafe {
        let ben = GetCurrentProcessId();
        let anlik = Tanitici(CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0));
        if anlik.0 == INVALID_HANDLE_VALUE {
            return None;
        }
        let mut g: PROCESSENTRY32W = std::mem::zeroed();
        g.dwSize = size_of::<PROCESSENTRY32W>() as u32;
        let mut ppid = None;
        let mut var = Process32FirstW(anlik.0, &mut g) != 0;
        while var {
            if g.th32ProcessID == ben {
                ppid = Some(g.th32ParentProcessID);
                break;
            }
            var = Process32NextW(anlik.0, &mut g) != 0;
        }
        let e = Tanitici(OpenProcess(PROCESS_SYNCHRONIZE | PROCESS_QUERY_LIMITED_INFORMATION, 0, ppid?));
        if e.0.is_null() {
            return None;
        }
        let dogum = |h: HANDLE| {
            let (mut y, mut c, mut k, mut u): (FILETIME, FILETIME, FILETIME, FILETIME) = std::mem::zeroed();
            (GetProcessTimes(h, &mut y, &mut c, &mut k, &mut u) != 0)
                .then(|| (u64::from(y.dwHighDateTime) << 32) | u64::from(y.dwLowDateTime))
        };
        match (dogum(e.0), dogum(GetCurrentProcess())) {
            (Some(o), Some(b)) if o > b => None, // PID yeniden kullanılmış: gerçek ebeveyn ölü
            _ => Some(e),
        }
    }
}

/// Sürecin yaratılış anı (FILETIME, 100 ns). Tanıtıcı QUERY_LIMITED hakkıyla açılmış olmalı.
fn dogum_ani(h: HANDLE) -> Option<u64> {
    // SAFETY: h geçerli bir süreç tanıtıcısı; çıktılar yerel FILETIME'lara.
    unsafe {
        let (mut y, mut c, mut k, mut u): (FILETIME, FILETIME, FILETIME, FILETIME) = std::mem::zeroed();
        (GetProcessTimes(h, &mut y, &mut c, &mut k, &mut u) != 0).then(|| (u64::from(y.dwHighDateTime) << 32) | u64::from(y.dwLowDateTime))
    }
}

/// Komutun gerçek dosyası (sahiplik kararındaki "bizim exe"). Tam yol verilirse çözülür;
/// çıplak ad gelirse PATH'te (PATHEXT yerine yalnız `.exe` eklenerek) aranır.
pub fn komut_yolu(komut: &OsStr) -> Option<PathBuf> {
    let k = Path::new(komut);
    if k.components().count() > 1 {
        return std::fs::canonicalize(k).ok();
    }
    let adlar = [k.to_path_buf(), k.with_extension("exe")];
    std::env::split_paths(&std::env::var_os("PATH")?)
        .flat_map(|d| adlar.iter().map(move |a| d.join(a)))
        .find(|y| y.is_file())
        .and_then(|y| std::fs::canonicalize(y).ok())
}

/// Sürecin tam exe yolu (`C:\…\python.exe`, `\\?\` öneksiz). Okunamazsa `None`.
pub fn surec_exe(pid: u32) -> Option<PathBuf> {
    // SAFETY: OpenProcess + QueryFullProcessImageNameW; tampon boyutu karakter cinsinden.
    unsafe {
        let h = Tanitici(OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid));
        if h.0.is_null() {
            return None;
        }
        let mut tampon = vec![0u16; 32768];
        let mut n = tampon.len() as u32;
        (QueryFullProcessImageNameW(h.0, PROCESS_NAME_WIN32, tampon.as_mut_ptr(), &mut n) != 0)
            .then(|| PathBuf::from(OsString::from_wide(&tampon[..n as usize])))
    }
}

/// `tara` Windows v0.1 bulgusu: yalnız K3 (şüpheli), asla temizlenmez.
#[derive(Debug, Clone)]
pub struct K3Bulgu {
    pub pid: u32,
    pub ad: String,
    pub neden: String,
    pub yas_sn: u64,
}

/// `tara` Windows v0.1: Toolhelp adı ajan izi taşıyan (node, mcp, claude, codex) ve ebeveyni ölü
/// (ya da PID'i sonradan doğana geçmiş) süreçler. Başka sürecin ortamı/komut satırı PEB okuması
/// ister → v0.2. Hiçbir sürece dokunmaz.
pub fn tara_k3() -> Vec<K3Bulgu> {
    let ben = std::process::id();
    let simdi = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs());
    let mut sonuc = Vec::new();
    for (pid, _, ad) in surec_adlari() {
        if pid == ben || pid == 0 || !sahiplik::ajan_adi_ipucu(&ad) {
            continue;
        }
        let Some(neden) = sahiplik::k3_ipucu(&ad, ebeveyn_durumu(pid)) else { continue };
        // FILETIME (1601'den 100 ns) → Unix saniye.
        let dogum = match canli_ac(pid) {
            Ok(Some(h)) => dogum_ani(h.0),
            _ => None,
        };
        let yas_sn = dogum.map_or(0, |d| simdi.saturating_sub((d / 10_000_000).saturating_sub(11_644_473_600)));
        sonuc.push(K3Bulgu { pid, ad, neden, yas_sn });
    }
    sonuc.sort_by_key(|b| b.pid);
    sonuc
}

/// (pid, ebeveyn pid, exe adı) — Toolhelp anlık görüntüsü.
fn surec_adlari() -> Vec<(u32, u32, String)> {
    let mut l = Vec::new();
    // SAFETY: Toolhelp; yapı sıfırlanıp dwSize bildirilir.
    unsafe {
        let anlik = Tanitici(CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0));
        if anlik.0 == INVALID_HANDLE_VALUE {
            return l;
        }
        let mut g: PROCESSENTRY32W = std::mem::zeroed();
        g.dwSize = size_of::<PROCESSENTRY32W>() as u32;
        let mut var = Process32FirstW(anlik.0, &mut g) != 0;
        while var {
            let n = g.szExeFile.iter().position(|c| *c == 0).unwrap_or(g.szExeFile.len());
            l.push((g.th32ProcessID, g.th32ParentProcessID, String::from_utf16_lossy(&g.szExeFile[..n])));
            var = Process32NextW(anlik.0, &mut g) != 0;
        }
    }
    l
}

/// (pid, ebeveyn pid) — Toolhelp anlık görüntüsü.
fn surec_listesi() -> Vec<(u32, u32)> {
    let mut l = Vec::new();
    // SAFETY: Toolhelp; yapı sıfırlanıp dwSize bildirilir.
    unsafe {
        let anlik = Tanitici(CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0));
        if anlik.0 == INVALID_HANDLE_VALUE {
            return l;
        }
        let mut g: PROCESSENTRY32W = std::mem::zeroed();
        g.dwSize = size_of::<PROCESSENTRY32W>() as u32;
        let mut var = Process32FirstW(anlik.0, &mut g) != 0;
        while var {
            l.push((g.th32ProcessID, g.th32ParentProcessID));
            var = Process32NextW(anlik.0, &mut g) != 0;
        }
    }
    l
}

/// Süreci QUERY_LIMITED + SYNCHRONIZE ile aç: Ok(Some) açık ve yaşıyor · Ok(None) yok/çıkmış · Err erişim reddi.
fn canli_ac(pid: u32) -> Result<Option<Tanitici>, ()> {
    // SAFETY: OpenProcess + WaitForSingleObject(0) yalnız durum sorar.
    unsafe {
        let h = Tanitici(OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SYNCHRONIZE, 0, pid));
        if h.0.is_null() {
            return if son_hata().raw_os_error() == Some(ERROR_ACCESS_DENIED as i32) { Err(()) } else { Ok(None) };
        }
        Ok((WaitForSingleObject(h.0, 0) != WAIT_OBJECT_0).then_some(h))
    }
}

/// Sürecin ebeveyni yaşıyor mu? (karar: `sahiplik::ebeveyn_karari`)
pub fn ebeveyn_durumu(pid: u32) -> Ebeveyn {
    let Some(&(_, ppid)) = surec_listesi().iter().find(|(p, _)| *p == pid) else {
        return Ebeveyn::Okunamadi; // süreç listede yok: yarış — şüphede dokunma
    };
    let cocuk_dogum = match canli_ac(pid) {
        Ok(Some(h)) => dogum_ani(h.0),
        _ => None,
    };
    match canli_ac(ppid) {
        Err(()) => sahiplik::ebeveyn_karari(ppid, Err(()), None, cocuk_dogum),
        Ok(None) => sahiplik::ebeveyn_karari(ppid, Ok(false), None, cocuk_dogum),
        Ok(Some(h)) => sahiplik::ebeveyn_karari(ppid, Ok(true), dogum_ani(h.0), cocuk_dogum),
    }
}

/// Bizim çocuğumuz olmayan öksüz süreci ve torunlarını durdur (`taskkill /T /F` kadar sert). Torunlar Toolhelp'ten ebeveyn zinciriyle bulunur; ebeveynden ÖNCE doğmuş
/// "çocuk" PID yeniden kullanımıdır, alınmaz. Öldürmeden hemen önce her PID'in exe'si yeniden
/// doğrulanır (bu arada PID başkasına geçmiş olabilir). Kayboluş `bekle` boyunca izlenir.
pub fn oksuzu_durdur(pid: u32, bekle: Duration, log: Gunluk) {
    let liste = surec_listesi();
    let dogum = |p: u32| canli_ac(p).ok().flatten().and_then(|h| dogum_ani(h.0));
    let mut hedefler = vec![pid];
    let mut i = 0;
    while i < hedefler.len() {
        let e = hedefler[i];
        let e_dogum = dogum(e);
        let cocuklar: Vec<u32> = liste
            .iter()
            .filter(|(c, pp)| *pp == e && *c != e && !hedefler.contains(c))
            .map(|&(c, _)| c)
            .filter(|&c| matches!((e_dogum, dogum(c)), (Some(ed), Some(cd)) if cd >= ed))
            .collect();
        hedefler.extend(cocuklar);
        i += 1;
    }
    let kimlik: Vec<(u32, Option<PathBuf>)> = hedefler.iter().map(|&p| (p, surec_exe(p))).collect();
    for (p, exe) in &kimlik {
        if surec_exe(*p) != *exe {
            continue;
        }
        // SAFETY: yalnız TERMINATE hakkıyla açılan, az önce kimliği doğrulanan süreç.
        unsafe {
            let h = Tanitici(OpenProcess(PROCESS_TERMINATE, 0, *p));
            if !h.0.is_null() && TerminateProcess(h.0, SONLANDIRMA_KODU) == 0 {
                log(&format!("pid {p} sonlandırılamadı: {}", son_hata()));
            }
        }
    }
    let son = Instant::now() + bekle;
    while Instant::now() < son && kimlik.iter().any(|(p, _)| matches!(canli_ac(*p), Ok(Some(_)))) {
        std::thread::sleep(Duration::from_millis(100));
    }
}
