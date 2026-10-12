//! ajan-bekci çekirdeği — bir ajanın (claude, codex …) bütün süreç ağacını tutan ve
//! kapanışta süpüren platform kafesi. Karar mantığı platformdan bağımsız ve test edilebilir;
//! sarmalayıcının döngüsü kök paketin `main.rs`'inde.
//!
//! Kaynak: kaynak depo 34d792b (ayrıntı `KAYNAK.md`).

#[cfg(unix)]
pub mod agac;
/// Çıktı dili: varsayılan İngilizce, `tr*` yerel ayarında ya da `--lang tr` ile Türkçe.
pub mod dil;
pub mod sahiplik;
/// Kaçak süreçleri kanıt seviyesiyle listeler (yalnız rapor, sinyal yok). Linux /proc + cgroup v2.
#[cfg(target_os = "linux")]
pub mod tara;
/// `tara --temizle`: K1/K2 kalıntılarını yeniden doğrulayarak kapatır (güvenlik kemeri: sinyalden hemen önce yeniden doğrulama).
#[cfg(target_os = "linux")]
pub mod temizle;

#[cfg(unix)]
mod unix;
#[cfg(unix)]
pub use unix::{Kafes, oksuzu_durdur};

#[cfg(windows)]
mod windows;
#[cfg(windows)]
pub use windows::{K3Bulgu, Kafes, ebeveyn_durumu, komut_yolu, oksuzu_durdur, surec_exe, tara_k3};

/// Günlük satırı yazan işlev. Kafes stderr'e kendisi YAZMAZ: sarmalayıcının stderr'i
/// kapanmış bir boru olabilir (EPIPE → `eprintln!` panikler, kapanışın ortasında ölüm).
pub type Gunluk = fn(&str);

/// Çocuğun süreç grubu (Unix). Windows'ta yok sayılır: çocuk konsolu zaten devralır.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Grup {
    /// Kendi grubu, terminale dokunulmaz (terminalsiz iş; testler).
    Kendi,
    /// (a) Kendi grubu + terminal ön planı çocuğa verilir, çıkışta geri alınır.
    KendiOnPlan,
    /// (b) Sarmalayıcının grubunda kalır; temizlik yalnız ağaç yürüyerek (grup sinyali YOK —
    /// sarmalayıcının kendisini de vururdu).
    Ortak,
}

