# ajan-bekci

> Son sürüm: [v0.1.0](https://github.com/ihsandeniz/ajan-bekci/releases/latest). English: [README.md](README.md).
> Köşeli parantezli işaretler ([O2](docs/olcumler.tr.md#o2) gibi) ölçüm özetinin ilgili bölümüne gider.

`ajan-bekci`, terminal kodlama ajanını (Claude Code, Codex CLI …) **sarmalayan** küçük bir Rust programı.
Ajan kapanınca — kendi çıkışı, terminalin kapanması, ajanın çökmesi — açtığı süreçleri (MCP sunucuları,
onların çocukları, `setsid` ile kopan geliştirme sunucuları) süpürür. Bu, Claude Code, Codex CLI ve sahte bir
ajanla üç kapanış yolunda ölçüldü. [O2](docs/olcumler.tr.md#o2)
Bir de `tara` komutu var: ölmüş ajanlardan geride kalmış süreçleri **kanıt seviyesiyle** listeler.

```bash
ajan-bekci -- claude
ajan-bekci -- codex
ajan-bekci tara
```

## Neden?

Bekçisiz ölçüm (Linux, sahte bir MCP sunucusu ile, her hücre 5 deneme): [O1](docs/olcumler.tr.md#o1)

- **Claude Code 2.1.294:** üç kapanış yolunun üçünde de 5/5 denemede MCP sunucusunun `setsid` ile kopardığı
  torun geride kaldı.
- **Codex CLI 0.161.0:** normal çıkışta aynı torun kaldı. Terminal kapanınca ya da ajan çökünce **bir süreç daha**
  kaldı: MCP sunucusunun normal çocuğu (10/10 deneme).
- Geride kalanların hepsi (30/30) PID 1'e bağlandı.

> Çekince: `setsid` torunu ölçüm düzeneğinin bilerek ürettiği bir desen. Gerçek MCP sunucularının bunu ne
> sıklıkla yaptığı ölçülmedi. [O1](docs/olcumler.tr.md#o1)

## Önce / sonra (Linux)

Kaçak = kapanıştan 5 sn sonra denemenin kafesinde (scope) hâlâ yaşayan süreç. Her hücre 5 deneme.
[O1](docs/olcumler.tr.md#o1) · [O2](docs/olcumler.tr.md#o2)

| Kapanış yolu | claude, bekçisiz | `ajan-bekci -- claude` | codex, bekçisiz | `ajan-bekci -- codex` |
|---|---|---|---|---|
| Normal çıkış | 5/5 kaçak (1 süreç) | **0/5** | 5/5 kaçak (1 süreç) | **0/5** |
| Terminal kapandı (SIGHUP) | 5/5 kaçak (1 süreç) | **0/5** | 5/5 kaçak (2 süreç) | **0/5** |
| Ajan çöktü (`kill -9`)¹ | 5/5 kaçak (1 süreç) | **0/5** | 5/5 kaçak (2 süreç) | **0/5** |
| Bekçinin kendisine `kill -9` | — | 5/5'te 1 süreç kaldı, **bekçinin scope'unda** | — | 5/5'te 2 süreç kaldı, **ikisi de bekçinin scope'unda** |

¹ Bekçisiz ölçümde ajana `kill -9`'dan sonra terminal de kapatıldı; çöküşün tek başına etkisi ayrı ölçülmedi.

Sahte ajanla da aynı: S1-S3 0/15, bekçiye `kill -9`'da kalan 1/1 scope'ta. Scope'ta kalanları `tara` sahte ajanla
K1 olarak buldu; gerçek claude/codex kalıntısı üzerinde `tara` koşulmadı. [O2](docs/olcumler.tr.md#o2) ·
[O3](docs/olcumler.tr.md#o3)

## Kurulum

- **Hazır ikili:** [GitHub Releases](https://github.com/ihsandeniz/ajan-bekci/releases/latest) — Linux x86_64
  (`ajan-bekci-<sürüm>-x86_64-linux.tar.gz`) ve Windows x86_64 (`…-x86_64-windows.zip`), `SHA256SUMS` ile.
- **Kaynaktan:** Rust 2024 sürümü gerekir; yalnız rustc 1.96 ile derlendi, asgari sürüm ölçülmedi.
  [O7](docs/olcumler.tr.md#o7)
- **AUR paketi:** sonra.

```bash
git clone https://github.com/ihsandeniz/ajan-bekci && cd ajan-bekci
cargo build --release
install -m 755 target/release/ajan-bekci ~/.local/bin/
```

Linux'ta `systemd --user` oturumu varsa bekçi kendini bir scope'a koyar; veriyolu yoksa düz kipte çalışır.
Veriyolu soketi var ama systemd yanıt vermiyorsa ajan başlamadan çıkılır (ölçülmedi). [O7](docs/olcumler.tr.md#o7)

## Kullanım

Çıktı dili varsayılan olarak İngilizcedir. `LC_ALL` → `LC_MESSAGES` → `LANG` sırasında ilk boş olmayan değer `tr` ile
başlıyorsa Türkçe olur;
`--lang tr|en` ortamı her zaman ezer. Seçenek adları (`--bekle`, `--kafes`, `tara`, `--temizle` …), çıkış kodları
ve `K1/K2/K3` dilden bağımsızdır. `--json` her zaman İngilizcedir. `ajan-bekci --help` tam yardımı basar.

### Sarmalayıcı

```bash
ajan-bekci [seçenekler] -- <komut> [arg…]
  --bekle SN            SIGKILL öncesi bekleme (varsayılan 3)
  --kafes cgroup|yok    cgroup: varsa systemd kullanıcı scope'u içinde çalış (varsayılan)
  --grup a|b            b: ajan sarmalayıcının süreç grubunda kalır (varsayılan)
  --birim AD            scope birim adı (varsayılan ajan-bekci-<pid>)
  --log DOSYA           tanı günlüğünü DOSYA'ya ekle (terminale hiçbir şey basılmaz)
  --lang en|tr          çıktı dili
```

- Ajan çocuk olarak başlar. Bekçi **alt-toplayıcı**dır (`setsid` ile kopan torunlar da ona bağlanır) ve ebeveyni
  ölünce sinyal alır.
- **Kapanış:** ajan çıkınca ya da terminal kapanınca (SIGHUP) / SIGTERM gelince ağaç süpürülür:
  SIGTERM → `--bekle` sn → SIGKILL. [O2](docs/olcumler.tr.md#o2)
- **Ctrl-C bekçiyi kapatmaz;** ajana gider. [O2](docs/olcumler.tr.md#o2)
- **`--kafes cgroup`:** bekçi kendini `systemd-run --user --scope` ile `ajan-bekci-<PID>.scope` içinde yeniden
  başlatır; PID değişmez. Bekçi `kill -9` yese bile kalanlar adı belli bir scope'ta durur. [O2](docs/olcumler.tr.md#o2)
- **`--grup b`:** ajan bekçinin süreç grubunda kalır, Ctrl-Z / `fg` ikisini birlikte durdurur ve sürdürür.
  `--grup a` ölçüldü ve iş denetimini kilitledi: Ctrl-Z sonrası kabuk geri gelmedi. [O2](docs/olcumler.tr.md#o2)
- Bekçi ajanın terminaline hiçbir şey yazmaz.
- Ajana `AJAN_BEKCI_PID` ortam değişkeni verilir; `tara` bunu K2 kanıtı olarak kullanır.
- **Çıkış kodu:** ajanınkidir (sinyalle öldüyse 128+n) · 2 kullanım hatası · 127 başlatılamadı · 1 kafes yok
  (ajanın kodu alınamadığında da 1).

### `tara` — geride kalanları listele

```bash
ajan-bekci tara [--json]
```

**Hiçbir sürece sinyal göndermez.** Çıkış 0 = K1/K2 yok, 3 = var. [O3](docs/olcumler.tr.md#o3)

```
$ ajan-bekci --lang tr tara
    PID  KANIT AJAN     YAŞ      RSS  KOMUT · ayrıntı
   4242  K1    codex    3dk     12MB  node mcp-server.js · ajan-bekci-4100.scope içinde, bekçisi yok
   4250  K2    claude   1sa      8MB  sleep 600 · CLAUDE_PID=4100 ölü
   4300  K3    ?         2g     40MB  node x · öksüz · claude izi, adında mcp
K1 kesin 1 · K2 güçlü 1 · K3 şüpheli 1 (K3 asla temizlenmez) · hiçbir sürece sinyal gönderilmedi
```

(örnek çıktı; PID'ler temsilî)

| Seviye | Anlamı |
|---|---|
| **K1 kesin** | Sürecin cgroup'u `ajan-bekci-*.scope` ve o scope'ta canlı bekçi yok. Ortam silinse bile yakalanır |
| **K2 güçlü** | Ortamında `CLAUDE_PID` ya da `AJAN_BEKCI_PID` var ve o PID ölü, zombi ya da başkasına geçmiş |
| **K3 ipucu** | Öksüz (ebeveyni PID 1 ya da `systemd --user`) + ajan izi. **Asla temizlenmez** |

- Ortamdan yalnız `CLAUDE_PID`, `AJAN_BEKCI_PID`, `AJAN_BEKCI_KAFES` değeri okunur; başka değişken saklanmaz.
  [O3](docs/olcumler.tr.md#o3)
- Gerçek bir makinede (7 canlı Claude oturumu, 53 meşru öksüz süreç) K1/K2'de **0 yanlış alarm**.
  [O3](docs/olcumler.tr.md#o3)
- Sahte kalıntı senaryosu: 10/10 bulundu, 10 tekrarda. [O3](docs/olcumler.tr.md#o3)

### `tara --temizle` — güvenlik kemeri

```bash
ajan-bekci tara --temizle --kuru                 # önce yalnız listele (sinyal yok)
ajan-bekci tara --temizle --sadece 4242,4250     # yalnız onayladığın PID'ler
```

```
KURU LİSTE — 2 hedef (yalnız K1/K2, uid 1000) · beyaz liste /home/u/.config/ajan-bekci/beyaz-liste (yok — boş)
HEDEF 4242 K1 node mcp-server.js · ajan-bekci-4100.scope içinde, bekçisi yok
HEDEF 4250 K2 sleep 600 · CLAUDE_PID=4100 ölü
DIŞARIDA 4260 canlı ajan ağacında
--kuru: hiçbir sürece sinyal gönderilmedi
```

Gerçek koşuda her hedef için `KAPANDI <pid> SIGTERM|SIGKILL`, `ATLANDI <pid> <neden>` ya da `KALDI <pid>` satırı
basılır.

- **Önce kuru liste her zaman basılır;** `--kuru` değilse yalnız o liste uygulanır. [O4](docs/olcumler.tr.md#o4)
- **Yalnız K1/K2 ve yalnız kendi kullanıcın.** K3 asla. [O4](docs/olcumler.tr.md#o4)
- **Her sinyalden hemen önce yeniden doğrulama:** kanıt hâlâ geçerli mi; uid + başlangıç anı + exe listelendiği
  anla aynı mı. Tutmayan atlanır. SIGKILL'den önce bir kez daha. [O4](docs/olcumler.tr.md#o4)
- **PID başkasına geçerse dokunulmaz** (iki noktada benzetimle sınandı). [O4](docs/olcumler.tr.md#o4)
- **Canlı `claude` / `codex` / `ajan-bekci` ağacı plana giremez.** [O4](docs/olcumler.tr.md#o4)
- **Beyaz liste:** `$XDG_CONFIG_HOME/ajan-bekci/beyaz-liste` (yoksa `~/.config/…`). Satır biçimi
  `exe <tam yol>` ya da `komut <alt dizgi>`, `#` yorum. Bozuk satır varsa hiçbir şey yapılmaz.
- Sahte senaryoda kuru liste = gerçek temizlik 6/6, yanlış öldürme 0, 10 tekrarda. Koruma satırları tek tek
  kırılınca (7 mutasyon) testler 7/7 yakaladı; SIGKILL öncesi ikinci kanıt denetimi ayrıca kırılmadı.
  [O4](docs/olcumler.tr.md#o4)
- **Çıkış:** 0 = hepsi kapandı ya da plan boş · 4 = atlanan ya da kalan var · 2 = kullanım.

> ⚠️ Ölmüş bir ajan oturumunun **bilerek** arka plana attığı bir sunucu (`nohup … &`) K2 görünür.
> Temizlemeden önce kuru listeye bak; böyle süreçleri beyaz listeye yaz. [O4](docs/olcumler.tr.md#o4)

## Windows (CI'da ölçüldü)

**Gerçek bir Windows makinesinde denenmedi.** Ölçümler GitHub `windows-latest` (Windows Server) CI'ında yapıldı,
fazlası değil. Bazı maddeler yalnız kodda ve birim testinde var; yanlarında yazıyor. [O5](docs/olcumler.tr.md#o5)

- **Kafes:** sarmalayıcı **Job Object** ile kurulur (KILL_ON_JOB_CLOSE; çocuk askıda başlatılıp işe konur).
- **Davranış (CI'da senaryo yok):**
  - Ctrl-C / Ctrl-Break yok sayılır; pencere, oturum ya da sistem kapanışı kapanış sayılır (kod + birim testi).
  - Konsol ajana devredilir; ebeveyn izlenir (yalnız kod).
- **Bekçiye `TerminateProcess` → torun 0.** İç içe iş → 0. Çıkış kodu aktarılır. Linux'taki "bekçiye `kill -9`'da
  kalan" sınırı Windows'ta yok, çünkü işi çekirdek kapatır.
- **CI:** 3 denemede yeşil; Windows testleri 15/15, duman 36/36.
- **`tara`:** Windows v0.1 **yalnız K3** listeler (ebeveyni ölü + ajan adlı süreç). K2 ve `--temizle` Windows'ta yok.
- `--kafes`, `--grup`, `--birim` Windows'ta etkisiz.
- **Sınanmadı:** gerçek Ctrl-C, etkileşimli konsol, gerçek claude/codex.

## Sınırlar

- **Bekçisiz açılmış codex'in kalıntısını `tara` K1/K2 ile görmez**; codex PID taşıyan bir ortam işareti bırakmıyor.
  MCP'nin araç çocuğunu yalnız K3 şüpheli olarak listeler. Ölçümdeki setsid torunu K3 ölçütlerine de uymuyor
  (ortamında codex izi taşıyıp taşımadığı ölçülmedi). Bu yüzden `--temizle` ona dokunmaz. Bekçiyle açılanların kalıntısı K1 kesin.
  [O3](docs/olcumler.tr.md#o3)
- **Bekçisiz claude'un MCP'sinin `CLAUDE_PID` taşıyıp taşımadığı ölçülmedi;** yani bekçisiz claude kalıntısının K2
  çıkıp çıkmadığı bilinmiyor. [O3](docs/olcumler.tr.md#o3)
- **Linux'ta bekçinin kendisi `kill -9` yerse** süpürecek kimse kalmaz. Kalanlar scope'ta bekler; `tara` bunu sahte
  ajanla K1 olarak buldu.
  [O2](docs/olcumler.tr.md#o2)
- "Canlı ajan" tanıma süreç ya da exe adıyla (`claude`, `codex`, `ajan-bekci`) yapılıyor. [O4](docs/olcumler.tr.md#o4)
- **Ölçülmeyenler:**
  - systemd'siz makinede düz kip
  - gündelik terminal zinciri ve orada Ctrl-Z
  - gerçek MCP sunucularının kopan çocuk açma sıklığı
  - ajanın kendi araç kabuklarının kalıntısı
  - çok kullanıcılı makine

  [O7](docs/olcumler.tr.md#o7)

## Geliştirme

```bash
cargo test --workspace && cargo clippy --workspace --all-targets -- -D warnings
```

Linux'ta 66 test; clippy Linux ve `x86_64-pc-windows-gnu` hedefinde `-D warnings` ile temiz. Windows testleri yalnız
CI'da koşar. Süreç öldüren her test yalnız kendi açtığı ağaca ya da kendi scope'una sinyal yollar; isimle eşleme
(`pkill`/`killall`) yok. [O6](docs/olcumler.tr.md#o6)

Çekirdek (`crates/cekirdek`) aynı yazarın süreç bekçisi projesinden kopyalandı; ayrıntı `KAYNAK.md`'de.

## Lisans

MIT ya da Apache-2.0, kullanıcının seçimine bağlı: [LICENSE-MIT](LICENSE-MIT) · [LICENSE-APACHE](LICENSE-APACHE).
