//! Çıktı dili + `--help`/`--version` (Linux ve Windows CI'da). Dil seçimi: varsayılan İngilizce,
//! `LC_ALL` → `LC_MESSAGES` → `LANG` ilk boş olmayanı `tr*` ise Türkçe, `--lang` her zaman ezer.
//! README örnekleri bu testlerin denetlediği çıktıyla aynıdır.

use std::process::{Command, Output, Stdio};

const BEKCI: &str = env!("CARGO_BIN_EXE_ajan-bekci");

/// Yerel ayar değişkenleri temizlenip yalnız verilenlerle koşturur.
fn kos(ortam: &[(&str, &str)], arg: &[&str]) -> Output {
    let mut k = Command::new(BEKCI);
    k.args(arg).stdin(Stdio::null()).env_remove("LC_ALL").env_remove("LC_MESSAGES").env_remove("LANG");
    k.envs(ortam.iter().copied());
    k.output().unwrap()
}

fn metin(b: &[u8]) -> String {
    String::from_utf8(b.to_vec()).unwrap()
}

#[test]
fn yardim_varsayilan_ingilizce_stdout_0() {
    for bayrak in ["--help", "-h"] {
        let c = kos(&[], &[bayrak]);
        assert_eq!(c.status.code(), Some(0), "{bayrak}");
        let o = metin(&c.stdout);
        assert!(o.starts_with(&format!("ajan-bekci {} — ", env!("CARGO_PKG_VERSION"))), "{o}");
        assert!(o.contains("\nusage: ajan-bekci [options] -- <command> [args…]\n"), "{o}");
        assert!(o.contains("--lang en|tr") && o.contains("tara --temizle"), "{o}");
        assert!(c.stderr.is_empty());
    }
    // tara'nın ardından da yardım.
    assert!(metin(&kos(&[], &["tara", "--help"]).stdout).contains("\nusage: "));
}

#[test]
fn ortam_secimi_ve_oncelik() {
    let kullanim = |ortam: &[(&str, &str)], arg: &[&str]| {
        let o = metin(&kos(ortam, arg).stdout);
        if o.contains("\nkullanım: ") {
            "tr"
        } else if o.contains("\nusage: ") {
            "en"
        } else {
            panic!("{o}")
        }
    };
    assert_eq!(kullanim(&[("LANG", "tr_TR.UTF-8")], &["-h"]), "tr");
    assert_eq!(kullanim(&[("LANG", "en_US.UTF-8")], &["-h"]), "en");
    assert_eq!(kullanim(&[("LC_ALL", "C"), ("LANG", "tr_TR.UTF-8")], &["-h"]), "en");
    assert_eq!(kullanim(&[("LC_MESSAGES", "tr_TR.UTF-8"), ("LANG", "C")], &["-h"]), "tr");
    assert_eq!(kullanim(&[("LC_ALL", ""), ("LANG", "tr")], &["-h"]), "tr");
    // --lang ortamı ezer; iki biçim, tara'dan önce ya da sonra.
    assert_eq!(kullanim(&[("LANG", "C")], &["--lang", "tr", "-h"]), "tr");
    assert_eq!(kullanim(&[("LANG", "tr_TR.UTF-8")], &["--lang=en", "-h"]), "en");
    assert_eq!(kullanim(&[], &["tara", "--lang", "TR", "-h"]), "tr");
}

#[test]
fn surum() {
    for bayrak in ["--version", "-V"] {
        let c = kos(&[("LANG", "tr_TR.UTF-8")], &[bayrak]);
        assert_eq!(c.status.code(), Some(0));
        assert_eq!(metin(&c.stdout), format!("ajan-bekci {}\n", env!("CARGO_PKG_VERSION")));
    }
}

#[test]
fn kullanim_hatalari_iki_dilde_2() {
    let hata = |ortam: &[(&str, &str)], arg: &[&str]| {
        let c = kos(ortam, arg);
        assert_eq!(c.status.code(), Some(2), "{arg:?}");
        assert!(c.stdout.is_empty(), "{arg:?}");
        metin(&c.stderr)
    };
    let e = hata(&[], &["--bilinmeyen"]);
    assert!(e.starts_with("ajan-bekci: unknown option: --bilinmeyen\nusage: "), "{e}");
    let e = hata(&[("LANG", "tr_TR.UTF-8")], &["--bilinmeyen"]);
    assert!(e.starts_with("ajan-bekci: bilinmeyen seçenek: --bilinmeyen\nkullanım: "), "{e}");
    assert!(hata(&[], &[]).starts_with("ajan-bekci: no command (try --help)\n"));
    assert!(hata(&[], &["--lang", "tr"]).starts_with("ajan-bekci: komut yok (--help'e bak)\n"));
    assert!(hata(&[], &["--bekle"]).starts_with("ajan-bekci: --bekle needs a value\n"));
    assert!(hata(&[], &["--lang", "de", "-h"]).starts_with("ajan-bekci: --lang en|tr, got de\n"));
    assert!(hata(&[("LANG", "tr_TR.UTF-8")], &["--lang", "xx"]).starts_with("ajan-bekci: --lang en|tr, gelen xx\n"));
    assert!(hata(&[], &["--lang"]).starts_with("ajan-bekci: --lang needs a value\n"));
    let e = hata(&[], &["tara", "--kuru"]);
    assert!(e.starts_with("ajan-bekci: tara: --kuru/--beyaz/--sadece only with --temizle\n"), "{e}");
    let e = hata(&[("LANG", "tr_TR.UTF-8")], &["tara", "--kuru"]);
    assert!(e.starts_with("ajan-bekci: tara: --kuru/--beyaz/--sadece yalnız --temizle ile\n"), "{e}");
}

/// `--`'dan sonrası ajanındır: `--help`/`--lang` sarmalayıcıya değil komuta gider.
#[cfg(unix)]
#[test]
fn ciftcizgiden_sonrasi_ajanin() {
    let c = kos(&[], &["--kafes", "yok", "--", "sh", "-c", "echo \"$@\"", "x", "--help", "--lang", "tr", "-V"]);
    assert_eq!(c.status.code(), Some(0));
    assert_eq!(metin(&c.stdout), "--help --lang tr -V\n");
}
