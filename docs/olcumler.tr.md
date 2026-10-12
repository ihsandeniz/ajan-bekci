# Ölçümler

README'deki her sayının dayanağı burada. Bölümler: [O1](#o1) bekçisiz taban · [O2](#o2) bekçiyle Linux ·
[O3](#o3) `tara` · [O4](#o4) `tara --temizle` · [O5](#o5) Windows (CI) · [O6](#o6) testler · [O7](#o7) ölçülmeyenler.
English: [olcumler.md](olcumler.md).

Tarih: 2026-10-11. Ölçüm makinesi: tek kullanıcılı Linux masaüstü (systemd kullanıcı oturumu). Bu özet ham ölçüm
kayıtlarından süzüldü; süreç kimlikleri, yollar ve makineye özgü ayrıntılar çıkarıldı, yalnız sayılar kaldı.

## Ölçüm düzeneği (O1 ve O2 için ortak)

- **Sahte MCP sunucusu:** en küçük stdio MCP sunucusu (`initialize` / `tools/list`). Başlarken iki çocuk açar:
  normal bir "araç" süreci ve `setsid` ile gruptan ve oturumdan kopan bir "geliştirme sunucusu" (torun).
- **Ajan bu sunucuyla açılır**, hiçbir istem gönderilmez, sonra aşağıdaki yollardan biriyle kapatılır.
- **Her deneme kendi kafesinde:** ayrı bir `systemd-run --user --scope` birimi, ayrı ev dizini, ayrı tmux sunucusu.
  Sayım ve temizlik yalnız o birimin süreç listesi üzerinden yapılır; isimle süreç eşleme yok.
- **Kaçak** = kapanıştan 5 sn sonra denemenin kafesinde (ve varsa bekçinin kafesinde) hâlâ yaşayan süreç.
- **Kapanış yolları:**

| Kod | Yol | Nasıl |
|---|---|---|
| S1 | normal çıkış | `/exit` (claude) · `/quit` (codex); ajanın kendi çıkması beklenir |
| S2 | terminal kapandı | tmux sunucusu öldürülür (SIGHUP) |
| S3 | ajan çöktü | ajanın kendisine `kill -9`. Bekçisiz ölçümde ardından tmux sunucusu da kapatılır (S2'nin SIGHUP'ı eklenir); bekçili ölçümde pane bekçiyle birlikte kendiliğinden kapanır |
| S4 | bekçi öldürüldü | bekçinin kendisine `kill -9` (yalnız O2) |

- Her hücre 5 deneme. Sürümler: Claude Code 2.1.294, Codex CLI 0.161.0.
- Gerçek ajanlar, yalnız ölçüm kafesine verilen giriş bilgisiyle açıldı. Claude için bekçili ölçüm, yalnız bu
  ölçüm için üretilmiş ayrı bir jetonla yapıldı.

<a id="o1"></a>
## O1 — Bekçisiz taban

| Yol | Claude Code | Codex CLI |
|---|---|---|
| S1 normal çıkış | 5/5 denemede kaçak · 1 süreç (setsid torunu) | 5/5 kaçak · 1 süreç (setsid torunu) |
| S2 terminal kapandı | 5/5 kaçak · 1 süreç (setsid torunu) | 5/5 kaçak · **2 süreç** (setsid torunu + MCP'nin araç çocuğu) |
| S3 ajan çöktü | 5/5 kaçak · 1 süreç (setsid torunu) | 5/5 kaçak · **2 süreç** (aynı ikisi) |

- **30/30 deneme başladı ve sayıldı.** Geride kalanların hepsi PID 1'e bağlandı.
- **MCP sunucusunun kendisi** 30/30 denemede öldü (stdin kapanınca çıkıyor).
- **Codex'in fazladan kaçağı:** MCP'nin araç çocuğu. Alanlarında süreç grubu = MCP sunucusu, oturum = ajanın oturumu.
  Okuma (çıkarım): codex MCP'yi ayrı bir süreç grubunda başlatıyor. Normal çıkışta o grubu temizliyor, terminal
  kapanınca (S2) ve çöküş + terminal kapanışında (S3) temizlemiyor. Çöküşün terminal kapanışı olmadan tek başına
  etkisi ayrı ölçülmedi.
- **Tekrar:** Claude bekçisiz S2, ayrı jetonla 5 kez daha koşuldu. Sonuç yine 5/5 kaçak · 1 süreç (setsid torunu),
  yani ilk ölçümle tutarlı.
- **Çekince:** `setsid` torunu düzeneğin bilerek ürettiği bir desen. Gerçek MCP sunucularının bunu ne sıklıkla
  yaptığı ölçülmedi.

<a id="o2"></a>
## O2 — Bekçiyle (Linux, `--grup b`, `--kafes cgroup`)

| Yol | `ajan-bekci -- claude` | `ajan-bekci -- codex` | `ajan-bekci -- <sahte ajan>` |
|---|---|---|---|
| S1 normal çıkış | **0/5** · bekçi 5/5 kendiliğinden çıktı | **0/5** · bekçi 5/5 kendiliğinden çıktı | 0/5 |
| S2 terminal kapandı | **0/5** | **0/5** | 0/5 |
| S3 ajan çöktü | **0/5** · bekçi 5/5 süpürüp çıktı | **0/5** · bekçi 5/5 süpürüp çıktı | 0/5 |
| S4 bekçiye `kill -9` | 5/5'te 1 süreç kaldı (setsid torunu), **1/1 bekçinin scope'unda** | 5/5'te 2 süreç kaldı, **2/2 bekçinin scope'unda** | 5/5'te 1 süreç, 1/1 scope'ta |

- **S1-S3 toplam:** claude 0/15 · codex 0/15 · sahte 0/15. Bekçisiz aynı yollarda claude 15/15, codex 15/15 denemede
  kaçak vardı ([O1](#o1)).
- **S4:** bekçi SIGKILL yiyince süpürecek kimse kalmıyor; ebeveyn ölüm sinyali torunlara geçmiyor. Kalanlar adı belli
  scope'ta duruyor. `tara` bu durumu sahte ajanla (gerçek bekçiye `kill -9`) K1 olarak buldu ([O3](#o3)); gerçek
  claude/codex kalıntısı üzerinde `tara` koşulmadı.
- **Kapanış ve kafes (testlerle sınandı):** ağaç SIGTERM → `--bekle` sn (varsayılan 3) → SIGKILL ile süpürülür.
  `--kafes cgroup`'ta bekçi kendini `systemd-run --user --scope` ile yeniden başlatır (exec), PID değişmez.
  systemd veriyolu yoksa düz kipe geçmesi yalnız tasarım: testi yok, ölçülmedi ([O7](#o7)).
- **Kafes denetimi:** codex + sahte 40/40 denemede bekçi, ajan ve MCP bekçinin scope'undaydı. Claude koşusunda 25/25 deneme
  başladı (ajan + MCP + araç çocuğu + setsid torunu her denemede görüldü).
- **Etkileşim duman testi (claude, 5 madde × 2 grup kipi):** 5/5. Maddeler: açılış, yazı ekranda, Ctrl-C ajanı da
  bekçiyi de kapatmadı, pencere 160→100 sütun ve ajan yeniden çizdi, `/exit` → bekçi kodu 0 ve kaçak 0.
- **Grup kipi seçimi:**
  - `--grup a`: Ctrl-Z yalnız ajanı durdurdu, bekçi beklemede kaldı, kabuk geri gelmedi. `fg` de Ctrl-C de işe yaramadı.
  - `--grup b`: Ctrl-Z ikisini durdurdu, `fg` ikisini sürdürdü, Ctrl-C ikisini temiz kapattı.
  - Bu yüzden varsayılan `b`. Ölçüm gerçek `bash -i` içinde, düz `sleep` ajanla yapıldı.

<a id="o3"></a>
## O3 — `tara` (yalnız rapor)

- **Gerçek makinede kuru çalışma:**
  - PID 1 ve kullanıcı systemd'si altındaki 53 meşru süreçten (terminal, tmux sunucusu, ses sunucusu, kabuklar …)
    **0'ı** K1/K2'de çıktı.
  - Yaşayan 7 Claude oturumunun torunlarından 0'ı çıktı.
- **Sahte kalıntı senaryosu:** 10 bulgunun 10'u bulundu (4 K1, 6 K2), 10 ayrı koşuda 10/10. 4 kontrol çıkmadı:
  canlı `CLAUDE_PID` taşıyan, canlı bekçinin scope'undaki iki süreç, işaretsiz süreç.
- **Çıkış ve sinyal:** çıkış 0 = K1/K2 yok, 3 = var. `tara` hiçbir sürece sinyal göndermez (modülde `kill` çağrısı yok).
- **Mutasyon:** doğrulama satırları tek tek silindi (canlı bekçili scope süzgeci, başlangıç anı karşılaştırması,
  canlı hedef, uid süzgeci). 4/4'ünü testler yakaladı.
- **Ortam okuma:** yalnız `CLAUDE_PID`, `AJAN_BEKCI_PID`, `AJAN_BEKCI_KAFES` değeri tutulur. Bunu bir birim testi
  doğruluyor.
- **Bekçisiz ajanların kalıntısı:**
  - Bekçisiz codex'in kalıntısı K1/K2'de görünmez, çünkü codex PID taşıyan bir ortam işareti bırakmıyor.
  - Araç çocuğu deseni ("grup lideri ve oturum lideri ölü") K3 olarak listelenir; birim testi O1'deki şekille yapıldı.
  - Ölçümdeki setsid torunu (`sleep`, kendi grup ve oturum lideri) K3'ün ad ve grup ölçütlerine uymaz. K3'e
    ancak ortamında codex izi (`CODEX_*`) taşırsa girer; bunu taşıyıp taşımadığı ölçülmedi.
  - Bekçisiz claude'un MCP'sinin `CLAUDE_PID` taşıyıp taşımadığı ölçülmedi.
- **Bekçiyle açılanların kalıntısı:** sahte senaryoda gerçek bekçiye `kill -9` ile kalanlar 10 tekrarda K1 bulundu.
  Gerçek claude/codex S4 kalıntısı (O2) üzerinde `tara` koşulmadı.

<a id="o4"></a>
## O4 — `tara --temizle`

- **Akış:** kuru liste her zaman önce basılır; `--kuru` değilse yalnız o plan uygulanır, arada değişen atlanır.
- **Sahte senaryo:** 6 hedef (3 K2, SIGTERM'i yok sayan 1 K2 SIGKILL ile kapandı, 2 K1). Kuru liste = gerçek temizlik
  6/6, 10 tekrarda 10/10.
- **Yanlış öldürme 0.** 4 kontrol her tekrarda yaşadı: beyaz listedeki, canlı bekçinin ağacındaki, `--sadece` dışındaki
  ve işaretsiz süreç.
- **PID yeniden kullanımı** sahte sistemle iki noktada benzetildi:
  - listelendikten sonra başlangıç anı değişmiş → sinyal yok;
  - SIGTERM'den sonra PID başkasına geçmiş → SIGKILL yok.
- **Mutasyon 7/7:** başlangıç anı, exe, uid, SIGTERM öncesi kanıt, canlı ajan ağacı, beyaz liste, K3 dışlama.
  SIGKILL öncesindeki ikinci kanıt denetimi ayrıca kırılmadı; SIGTERM öncesindekinin aynısı.
- **Gerçek makinede yalnız `--kuru` koşuldu: 0 hedef.**
- **Bilinen:** ölmüş bir oturumun bilerek arka plana attığı sunucu K2 çıkar; tek koruma beyaz liste. Canlı ajan tanıma
  süreç ya da exe adıyla yapılıyor.

<a id="o5"></a>
## O5 — Windows (yalnız CI)

**Gerçek bir Windows makinesi yok.** Bütün sonuçlar GitHub Actions `windows-latest` (Windows Server) imajında.

| Deneme | Linux | Windows | Windows testleri | Duman (4 senaryo × 3 tur) |
|---|---|---|---|---|
| 1 | ✅ | ✅ | 5/5 | 12/12 |
| 2 | ✅ | ✅ | 5/5 | 12/12 |
| 3 | ✅ | ✅ | 5/5 | 12/12 |

- **Toplam:** testler 15/15, duman 36/36.
- **Sahte ajan:** `node` ile yazıldı. Bir MCP benzeri torun ve onun çocuğu, bir `start /b` torunu, bir `detached`
  torun açıyor (5 süreç).
- **Bekçiye `TerminateProcess` → kalan 0.** İç içe iş (dış bekçi kapanınca) → kalan 0.
- **Çıkış kodu aktarımı:** `cmd /c exit 7` → 7, `node process.exit(3)` → 3, olmayan komut → 127, iç içe kod 9 iki
  kattan geçti.
- **`tara` v0.1:** ebeveyni ölü bir `node` K3 çıktı ve `tara`'dan sonra hâlâ yaşıyordu. `--temizle` → çıkış 1.
- **CI senaryosu olmayanlar:** Ctrl-C/Ctrl-Break'in yok sayılması yalnız kod + birim testi (konsol olayı ayrımı);
  konsolun devralınması ve ebeveyn tanıtıcısının izlenmesi yalnız kodda (test yok).
- **Sınanmadı:** gerçek Ctrl-C, etkileşimli konsol, gerçek claude/codex, evdeki bir Windows 11 masaüstü (Defender,
  farklı konsol barındırıcı).

<a id="o6"></a>
## O6 — Testler ve statik denetim

- Linux'ta 66 test (`cargo test --workspace`; iki dilli çıktı testleri dahil).
- `cargo clippy --workspace --all-targets -- -D warnings` Linux'ta ve `x86_64-pc-windows-gnu` hedefinde temiz.
- Windows testleri (`tests/windows.rs`, `olcum/windows/duman.ps1`) yalnız CI'da koşar.
- Süreç öldüren her test yalnız kendi açtığı ağaca ya da kendi scope'una sinyal yollar. Testler işaretli `sleep <sayı>`
  süreçleri kullanır; isimle eşleme yok.

<a id="o7"></a>
## O7 — Ölçülmeyenler

- Gerçek MCP sunucularının oturumdan kopan çocuk açma sıklığı.
- Ajanın kendi araç kabuklarının (istemle çalışan Bash vb.) kalıntısı; ölçümde istem gönderilmedi.
- Gündelik terminal zinciri (terminal → kabuk → `ajan-bekci -- claude`) ve orada gerçek Ctrl-Z/`fg`.
- Systemd'siz makinede düz kip. Systemd veriyolu var ama yanıt vermiyorsa ajan başlamadan çıkılır; bu da ölçülmedi.
- Çok kullanıcılı makine.
- Gerçek Windows; asgari Rust sürümü (yalnız rustc 1.96 ile derlendi).
