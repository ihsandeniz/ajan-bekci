//! Çıktı dili (bağımlılıksız). Varsayılan İngilizce; Türkçe ancak `--lang tr` ya da ortamda
//! POSIX önceliğiyle ilk boş olmayan `LC_ALL` → `LC_MESSAGES` → `LANG` değeri `tr` ile başlıyorsa.
//! Seçim süreç başında bir kez yapılır (`ayarla`); metinler kullanıldıkları yerde `m(en, tr)`
//! çiftiyle yazılır — çeviri dosyası yok, iki dil yan yana durur.
//! `--json` her zaman İngilizce: sarmalayıcı JSON basmadan önce `ayarla(Dil::En)` der.

use std::sync::atomic::{AtomicBool, Ordering};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dil {
    En,
    Tr,
}

static TURKCE: AtomicBool = AtomicBool::new(false);

pub fn ayarla(d: Dil) {
    TURKCE.store(d == Dil::Tr, Ordering::Relaxed);
}

pub fn simdi() -> Dil {
    if TURKCE.load(Ordering::Relaxed) { Dil::Tr } else { Dil::En }
}

/// Seçili dildeki metin.
pub fn m(en: &'static str, tr: &'static str) -> &'static str {
    match simdi() {
        Dil::En => en,
        Dil::Tr => tr,
    }
}

/// `--lang` değeri: `en` | `tr` (büyük/küçük harf fark etmez).
pub fn ayristir(deger: &str) -> Option<Dil> {
    match deger.to_ascii_lowercase().as_str() {
        "en" => Some(Dil::En),
        "tr" => Some(Dil::Tr),
        _ => None,
    }
}

/// Ortamdan dil. `oku` değişken okuyucusu (testte sahte, gerçekte `std::env::var`).
pub fn ortamdan(oku: impl Fn(&str) -> Option<String>) -> Dil {
    for ad in ["LC_ALL", "LC_MESSAGES", "LANG"] {
        if let Some(d) = oku(ad).filter(|d| !d.is_empty()) {
            return if d.to_ascii_lowercase().starts_with("tr") { Dil::Tr } else { Dil::En };
        }
    }
    Dil::En
}

#[cfg(test)]
mod testler {
    use super::*;

    fn ortam(cift: &[(&str, &str)]) -> Dil {
        ortamdan(|ad| cift.iter().find(|(a, _)| *a == ad).map(|(_, d)| (*d).to_owned()))
    }

    #[test]
    fn ortam_onceligi() {
        assert_eq!(ortam(&[]), Dil::En);
        assert_eq!(ortam(&[("LANG", "tr_TR.UTF-8")]), Dil::Tr);
        assert_eq!(ortam(&[("LANG", "en_US.UTF-8")]), Dil::En);
        assert_eq!(ortam(&[("LANG", "C")]), Dil::En);
        // LC_ALL her şeyi ezer, LC_MESSAGES LANG'ı ezer, boş değer atlanır.
        assert_eq!(ortam(&[("LC_ALL", "C"), ("LANG", "tr_TR.UTF-8")]), Dil::En);
        assert_eq!(ortam(&[("LC_ALL", "tr_TR.UTF-8"), ("LANG", "en_US.UTF-8")]), Dil::Tr);
        assert_eq!(ortam(&[("LC_MESSAGES", "tr_TR"), ("LANG", "en_US")]), Dil::Tr);
        assert_eq!(ortam(&[("LC_ALL", ""), ("LANG", "tr")]), Dil::Tr);
    }

    #[test]
    fn bayrak_degeri() {
        assert_eq!(ayristir("tr"), Some(Dil::Tr));
        assert_eq!(ayristir("EN"), Some(Dil::En));
        assert_eq!(ayristir("de"), None);
        assert_eq!(ayristir(""), None);
    }
}
