//! Bu süreç kimin? — sahiplik KARARI (platformdan bağımsız).
//!
//! Veri platforma göre başka yerden gelir (Linux `/proc`; Windows `QueryFullProcessImageNameW` ·
//! Toolhelp + yaratılış anı, bkz. `windows.rs`), ama ölçüt AYNI: **sahiplik**.
//!
//! - Bizim programımız (aynı exe) ve ebeveyni ölü → öksüz, durdurulabilir
//! - Bizim programımız ama ebeveyni canlı → başka bir örneğe ait, DOKUNMA
//! - Başka bir program ya da okunamayan → DOKUNMA
//!
//! Windows'ta öksüz süreç init'e devredilmez: ebeveyn PID'i ölü bir süreci (ya da PID
//! yeniden kullanıldıysa bambaşka, çocuktan SONRA doğmuş bir süreci) gösterir. Ebeveyn
//! okunamıyorsa (erişim reddi) şüphe = canlı sayılır, dokunulmaz.
//!
//! Bu dosya Win32 çağırmaz → Linux'ta da derlenir ve test edilir. Kaynakta (kaynak depo 34d792b)
//! karar bir portu dinleyen süreçler için kurulmuştu ve gerçek Windows'ta CI'da ölçülmüştü;
//! burada port ayağı atıldı, karar "aday süreç" listesine genelleştirildi (`KAYNAK.md`).

use std::path::{Path, PathBuf};
use crate::dil::m;

/// Aday sürecin ebeveyni hakkında bildiklerimiz.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Ebeveyn {
    /// Ebeveyn yaşıyor (PID'i ve yaratılışı tutarlı).
    Canli(u32),
    /// Ebeveyn yok: süreç bulunamadı, çıkmış ya da PID'i çocuktan sonra doğan birine geçmiş.
    Olu,
    /// Okunamadı (erişim reddi vb.) — şüphede dokunulmaz.
    Okunamadi,
}

/// Sahipliği sorulan süreç.
#[derive(Debug, Clone, PartialEq)]
pub struct Aday {
    pub pid: u32,
    /// Sürecin tam exe yolu; okunamadıysa `None`.
    pub exe: Option<PathBuf>,
    pub ebeveyn: Ebeveyn,
}

#[derive(Debug, PartialEq)]
pub enum Sahip {
    /// Aday yok.
    Bos,
    Oksuz(u32),
    CanliKardes { pid: u32, ebeveyn: u32 },
    /// Başka program, exe'si okunamıyor ya da ebeveyni okunamayan bizim program.
    Yabanci { pid: Option<u32>, exe: Option<PathBuf> },
}

/// Windows yollarını karşılaştırılabilir yap: `\\?\` öneki (`fs::canonicalize` ekler,
/// `QueryFullProcessImageNameW` eklemez) atılır, `/` → `\`, büyük/küçük harf yok sayılır
/// (NTFS varsayılanı). `\\?\UNC\sunucu\…` → `\\sunucu\…`.
pub fn yol_anahtari(yol: &Path) -> String {
    let s = yol.to_string_lossy().replace('/', "\\");
    let s = if let Some(unc) = s.strip_prefix(r"\\?\UNC\") {
        format!(r"\\{unc}")
    } else {
        s.strip_prefix(r"\\?\").map(str::to_owned).unwrap_or(s)
    };
    s.to_lowercase()
}

/// `adaylar` aynı kaynağı paylaşan süreçler (ör. bir fork'lu sunucunun kopyaları; tekrarsız).
/// Hepsi bizim exe ve hepsinin ebeveyni ölüyse öksüz; aksi her durumda dokunulmaz.
pub fn siniflandir(adaylar: &[Aday], bizim_exe: Option<&Path>) -> Sahip {
    let Some(ilk) = adaylar.first() else {
        return Sahip::Bos;
    };
    let bizim = bizim_exe.map(yol_anahtari);
    let hepsi_bizim = bizim.is_some() && adaylar.iter().all(|d| d.exe.as_deref().map(yol_anahtari) == bizim);
    if !hepsi_bizim {
        // Temsilci: bizim olmayan ilk süreç (kullanıcıya gösterilecek olan o).
        let y = adaylar.iter().find(|d| d.exe.as_deref().map(yol_anahtari) != bizim).unwrap_or(ilk);
        return Sahip::Yabanci { pid: Some(y.pid), exe: y.exe.clone() };
    }
    // Hepsi bizim exe. Biri bile canlı/okunamayan ebeveynliyse dokunma.
    for d in adaylar {
        match d.ebeveyn {
            Ebeveyn::Canli(e) => return Sahip::CanliKardes { pid: d.pid, ebeveyn: e },
            Ebeveyn::Okunamadi => return Sahip::Yabanci { pid: Some(d.pid), exe: d.exe.clone() },
            Ebeveyn::Olu => {}
        }
    }
    Sahip::Oksuz(ilk.pid)
}

/// Ebeveyn kararı: `ebeveyn_dogum`/`cocuk_dogum` aynı birimde yaratılış anı (Windows FILETIME,
/// 100 ns). Ebeveyn çocuktan SONRA doğmuşsa PID yeniden kullanılmıştır → gerçek ebeveyn ölü.
pub fn ebeveyn_karari(ppid: u32, acilabildi: Result<bool, ()>, ebeveyn_dogum: Option<u64>, cocuk_dogum: Option<u64>) -> Ebeveyn {
    match acilabildi {
        Err(()) => Ebeveyn::Okunamadi,
        Ok(false) => Ebeveyn::Olu, // süreç yok ya da çıkmış
        Ok(true) => match (ebeveyn_dogum, cocuk_dogum) {
            (Some(e), Some(c)) if e > c => Ebeveyn::Olu,
            (Some(_), Some(_)) => Ebeveyn::Canli(ppid),
            _ => Ebeveyn::Okunamadi,
        },
    }
}

/// Windows konsol olayı: Ctrl-C (0) ve Ctrl-Break (1) ajanındır → sarmalayıcı yok sayar.
/// Pencere kapatma (2), kullanıcı çıkışı (logoff, 5), sistem kapanışı (6) kapanış isteğidir.
pub fn konsol_olayi_yok_sayilir(tur: u32) -> bool {
    matches!(tur, 0 | 1)
}

/// Windows `tara` v0.1 ön süzgeci: exe adı bir ajan izi taşıyor mu (büyük/küçük harf yok sayılır).
pub fn ajan_adi_ipucu(ad: &str) -> bool {
    let a = ad.to_ascii_lowercase();
    matches!(a.as_str(), "node.exe" | "claude.exe" | "codex.exe") || a.contains("mcp")
}

/// Windows `tara` v0.1 kararı (yalnız K3): ajan izli ad + ebeveyn ÖLÜ. Ebeveyn canlı ya da
/// okunamıyorsa bulgu yok (şüphede listeleme bile yok — yanlış alarm, kullanıcıyı yorar).
pub fn k3_ipucu(ad: &str, ebeveyn: Ebeveyn) -> Option<String> {
    (ajan_adi_ipucu(ad) && ebeveyn == Ebeveyn::Olu).then(|| format!("{} · {} {ad}", m("orphan (parent dead)", "öksüz (ebeveyn ölü)"), m("name", "ad")))
}

#[cfg(test)]
mod testler {
    use super::*;

    const AJAN: &str = r"C:\Users\m\AppData\Roaming\npm\node_modules\ajan\bin\node.exe";

    fn d(pid: u32, exe: &str, ebeveyn: Ebeveyn) -> Aday {
        Aday { pid, exe: Some(PathBuf::from(exe)), ebeveyn }
    }

    #[test]
    fn yol_anahtari_onek_ayrac_ve_harf() {
        let a = yol_anahtari(Path::new(&format!(r"\\?\{AJAN}")));
        assert_eq!(a, yol_anahtari(Path::new(AJAN)));
        assert_eq!(a, yol_anahtari(Path::new(&AJAN.to_uppercase().replace('\\', "/"))));
        assert_eq!(yol_anahtari(Path::new(r"\\?\UNC\nas\pay\py.exe")), r"\\nas\pay\py.exe");
        assert_ne!(yol_anahtari(Path::new(r"C:\a\python.exe")), yol_anahtari(Path::new(r"C:\b\python.exe")));
    }

    #[test]
    fn aday_yoksa_bos() {
        assert_eq!(siniflandir(&[], Some(Path::new(AJAN))), Sahip::Bos);
        assert_eq!(siniflandir(&[], None), Sahip::Bos);
    }

    #[test]
    fn oksuz_bizim_program() {
        let l = [d(4242, &format!(r"\\?\{AJAN}"), Ebeveyn::Olu)];
        assert_eq!(siniflandir(&l, Some(Path::new(AJAN))), Sahip::Oksuz(4242));
    }

    #[test]
    fn canli_ebeveynli_bizim_program_dokunulmaz() {
        let l = [d(4242, AJAN, Ebeveyn::Canli(77))];
        assert_eq!(siniflandir(&l, Some(Path::new(AJAN))), Sahip::CanliKardes { pid: 4242, ebeveyn: 77 });
    }

    #[test]
    fn ebeveyni_okunamayan_bizim_program_dokunulmaz() {
        let l = [d(4242, AJAN, Ebeveyn::Okunamadi)];
        assert!(matches!(siniflandir(&l, Some(Path::new(AJAN))), Sahip::Yabanci { pid: Some(4242), .. }));
    }

    #[test]
    fn baska_program_yabanci() {
        let l = [d(900, r"C:\Program Files\nginx\nginx.exe", Ebeveyn::Olu)];
        assert_eq!(
            siniflandir(&l, Some(Path::new(AJAN))),
            Sahip::Yabanci { pid: Some(900), exe: Some(PathBuf::from(r"C:\Program Files\nginx\nginx.exe")) }
        );
    }

    #[test]
    fn baska_kurulumun_ayni_adli_programi_yabanci() {
        // Aynı ad, başka dizin (kullanıcının kendi node'u) — bizim değil.
        let l = [d(900, r"C:\Program Files\nodejs\node.exe", Ebeveyn::Olu)];
        assert!(matches!(siniflandir(&l, Some(Path::new(AJAN))), Sahip::Yabanci { pid: Some(900), .. }));
    }

    #[test]
    fn exe_okunamayan_aday_yabanci() {
        let l = [Aday { pid: 4, exe: None, ebeveyn: Ebeveyn::Olu }]; // ör. System (pid 4)
        assert!(matches!(siniflandir(&l, Some(Path::new(AJAN))), Sahip::Yabanci { pid: Some(4), exe: None }));
    }

    #[test]
    fn karisik_adaylar_yabanci_ve_temsilci_yabanci_olan() {
        let l = [d(1, AJAN, Ebeveyn::Olu), d(2, r"C:\x\other.exe", Ebeveyn::Olu)];
        assert!(matches!(siniflandir(&l, Some(Path::new(AJAN))), Sahip::Yabanci { pid: Some(2), .. }));
    }

    #[test]
    fn bizim_exe_bilinmiyorsa_hep_yabanci() {
        let l = [d(4242, AJAN, Ebeveyn::Olu)];
        assert!(matches!(siniflandir(&l, None), Sahip::Yabanci { .. }));
    }

    #[test]
    fn ebeveyn_karari_pid_yeniden_kullanimi() {
        assert_eq!(ebeveyn_karari(77, Ok(true), Some(200), Some(100)), Ebeveyn::Olu); // ebeveyn sonra doğmuş
        assert_eq!(ebeveyn_karari(77, Ok(true), Some(50), Some(100)), Ebeveyn::Canli(77));
        assert_eq!(ebeveyn_karari(77, Ok(false), None, Some(100)), Ebeveyn::Olu);
        assert_eq!(ebeveyn_karari(77, Err(()), None, None), Ebeveyn::Okunamadi);
        assert_eq!(ebeveyn_karari(77, Ok(true), None, Some(100)), Ebeveyn::Okunamadi);
    }

    #[test]
    fn konsol_ctrl_c_ve_break_yok_sayilir_kapatma_sayilmaz() {
        assert!(konsol_olayi_yok_sayilir(0) && konsol_olayi_yok_sayilir(1));
        for t in [2, 5, 6] {
            assert!(!konsol_olayi_yok_sayilir(t), "{t}");
        }
    }

    #[test]
    fn windows_k3_yalniz_ajan_izi_ve_olu_ebeveyn() {
        assert!(k3_ipucu("node.exe", Ebeveyn::Olu).is_some());
        assert!(k3_ipucu("Server-MCP.exe", Ebeveyn::Olu).is_some());
        assert!(k3_ipucu("CODEX.EXE", Ebeveyn::Olu).is_some());
        assert!(k3_ipucu("node.exe", Ebeveyn::Canli(4)).is_none());
        assert!(k3_ipucu("node.exe", Ebeveyn::Okunamadi).is_none());
        assert!(k3_ipucu("explorer.exe", Ebeveyn::Olu).is_none(), "ebeveyni ölü meşru Windows süreçleri");
    }
}
