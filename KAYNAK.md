# KAYNAK — çekirdeğin geldiği yer

Çekirdek, aynı yazarın süreç bekçisi projesinin **34d792b** sürümünden kopyalanarak çıkarıldı ve
ajan sarmalayıcıya göre arındırıldı. Git geçmişi taşınmadı.

| Bu depo | Kaynak (34d792b) | Ne değişti |
|---|---|---|
| `crates/cekirdek/src/agac.rs` | `src/agac.rs` | değişmedi (5 test) |
| `crates/cekirdek/src/sahiplik.rs` | `src/sahiplik.rs` | port ayağı atıldı: `yerel_port`, `Dinleyen` → `Aday`, `siniflandir`'dan `tablo_okundu`/`port_dolu` çıktı. `yol_anahtari`, `ebeveyn_karari`, sınıflandırma kuralı aynı. 13 testten 11'i kaldı (port testleri `yerel_port_ag_bayt_sirasi`, `tablo_okunamadi…` düştü; `bos_port` → `aday_yoksa_bos`), örnek yollar genelleşti |
| `crates/cekirdek/src/unix.rs` | `src/sistem_unix.rs` | `Sistem` → `Kafes`. Çıktı: `portu_hazirla`, `--kapanis` notu, sağlık/port bağımlılığı, çocuğa sabit ortam değişkeni (artık `baslat(komut, ortam)` dışarıdan alır). Günlük `crate::log` yerine `Gunluk` geri çağrısı. `oksuzu_durdur` `pub` + günlük parametresi. Süreç grubu + pdeathsig + subreaper + SIGTERM→SIGKILL aynen |
| `crates/cekirdek/src/windows.rs` | `src/sistem_windows.rs` | `Sistem` → `Kafes`. Çıktı: HTTP kibar kapanış, TCP tablosu (`GetExtendedTcpTable`), `portu_hazirla`, sabit ortam değişkenleri, **`CREATE_NO_WINDOW`** (etkileşimli ajanın konsolu devralınmalı). `komut_yolu`, `surec_exe`, `ebeveyn_durumu`, `oksuzu_durdur` `pub` (`tara` için). Job Object + askıda başlat → işe koy → sür aynen. `IpHelper`/`WinSock` özellikleri Cargo'dan düştü |
| `crates/cekirdek/tests/kafes.rs` | `tests/agac.rs` | Kaynakta testler bekçi **ikilisi** üzerinden; ikili sonradan yazıldığı için 9 davranış doğrudan `Kafes` üzerinden: çıkış kodu, 128+n, 127, ortam + kendi grup, setsid torunu kapanışı, çocuk çıkınca kaçak torunun kafeste kalması, TERM'i yok sayana SIGKILL, zombi toplama, `oksuzu_durdur` yalnız verilen ağaç. Kaynağa özgü olanlar (çocuğa verilen sabit ebeveyn PID değişkeni, stderr borusu, ebeveyn SIGKILL) ikilinin testlerinde (`tests/sarmalayici.rs`) |
| `Cargo.lock` | `Cargo.lock` | çözümleme aynı sürümlerle (nix 0.31.3, signal-hook 0.4.5, windows-sys 0.61.2) |

Alınmayanlar: `main.rs` (sunucu bekçisi durum makinesi), `ayar.rs`, `politika.rs`, `saglik.rs`, `gunluk.rs`,
`port.rs` (Linux port sahibi; `ebeveyn_canli` ölçütü ajan için yanlış alarm üretir), `tests/eski.rs`, `tests/yeniden.rs`.

**Fark taşıma:** kaynakta 34d792b sonrası `agac.rs` / `sahiplik.rs` / platform dosyalarına giren düzeltmeler buraya elle taşınır.
