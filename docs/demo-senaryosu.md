# Demo kaydı senaryosu (kayıt yapılmadı)

Amaç: 60-90 saniyelik bir terminal kaydında üç şeyi göstermek:
1. Bekçisiz kapanan bir ajan arkasında süreç bırakır.
2. Aynı ajan `ajan-bekci --` ile kapanınca hiçbir şey kalmaz.
3. Bekçinin kendisi `kill -9` yese bile `tara` kalanları **K1 kesin** kanıtla bulur, `--temizle` önce kuru liste gösterir.

**Kayıt temiz bir sanal makinede (VM) çekilir.** Geliştirme makinesinde çekilmez: orada açık ajan oturumları, gerçek
PID'ler, ev yolu ve başka süreçlerin komut satırları ekrana düşer, ayrıca sahne 3'teki `kill -9` ve `--temizle`
gerçek süreçlere dokunur; bunlar yalnız tek kullanımlık bir makinede yapılmalı. Kayıt aracı: `asciinema rec` (metin tabanlı, görsel kayıttan az sızdırır).

## Güvenlik kuralları (kayıt sırasında)

- **VM:** systemd kullanıcı oturumu olan güncel bir Linux (scope kafesi için), Rust araç zinciri, `asciinema`. Ağ
  yalnız derleme için; içinde başka ajan, giriş bilgisi ya da kişisel dosya yok. Kullanıcı ve makine adı nötr
  (ör. `demo@demo`), `PS1='$ '`.
- Sinyal yalnız **kendi açtığın, PID'ini ekranda gördüğün** sürece gider. `pkill`/`killall`/isimle eşleme yok.
- `--temizle` yalnız `--kuru` ya da `--sadece <ekrandaki PID'ler>` ile.
- Gerçek claude/codex yerine **sahte ajan** kullanılır (kimlik gerekmez, istem gönderilmez). Gerçek ajanla kayıt istenirse ayrı jeton şart; asıl girişin kopyası kullanılmaz.

## Hazırlık (kayıttan önce, kayda girmez)

```bash
cargo build --release && export PATH="$PWD/target/release:$PATH"
mkdir -p /tmp/demo && cat > /tmp/demo/sahte-ajan.sh <<'SH'
#!/bin/bash
# "ajan": bir MCP benzeri çocuk ve oturumdan kopan bir "geliştirme sunucusu" açar
sleep 9101 &                 # MCP'nin normal çocuğu gibi
setsid -f sleep 9102         # gruptan ve oturumdan kopan torun
echo "ajan hazır (PID $$) — kapatmak için Enter"
read
SH
chmod +x /tmp/demo/sahte-ajan.sh
```

İşaretli sayılar (`9101`, `9102`) yalnız ekranda tanımak için. Listeleme `ps` ile yapılır, sinyal PID'le.

## Sahne 1 — bekçisiz (≈20 sn)

```bash
$ /tmp/demo/sahte-ajan.sh
ajan hazır (PID …) — kapatmak için Enter
⏎
$ ps -o pid,ppid,cmd -C sleep | grep 910
  …  1 sleep 9101          # ← ajan çıktı, iki süreç kaldı, ikisinin de ebeveyni PID 1
  …  1 sleep 9102
```

Anlatım satırı: "Ajan kapandı ama açtığı geliştirme sunucusu yaşıyor; kimse kapatmayacak."
Temizlik (ekrandaki PID'lerle): `kill <pid> <pid>`.

> Not: bu sahne gerçek ölçümün küçültülmüş hâli. Gerçek ölçümde Claude Code ve Codex CLI'da aynı torun
> 5/5 denemede kaldı ([olcumler.tr.md § O1](olcumler.tr.md#o1)). Sahte betik bunu yeniden üretmek için var; ekranda
> "sahte ajan" olduğu açıkça yazılsın.

## Sahne 2 — bekçiyle (≈15 sn)

```bash
$ ajan-bekci -- /tmp/demo/sahte-ajan.sh
ajan hazır (PID …) — kapatmak için Enter
⏎
$ ps -o pid,ppid,cmd -C sleep | grep 910
$                          # ← boş: bekçi ağacı süpürdü
```

Anlatım: "Aynı ajan, başına `ajan-bekci --` yazınca geride hiçbir şey bırakmıyor."
İsteğe bağlı: aynı sahneyi terminal penceresini kapatarak tekrarla (SIGHUP yolu).

## Sahne 3 — bekçiye kill -9 + tara (≈30 sn)

```bash
# pane A:
$ ajan-bekci -- /tmp/demo/sahte-ajan.sh
# pane B:
$ pgrep -x ajan-bekci                          # bekçinin PID'i ekranda
$ kill -9 <bekçi PID>                          # yalnız bu PID
$ ajan-bekci tara
K1 … sleep 9101 · ajan-bekci-<PID>.scope, canlı bekçi yok
K1 … sleep 9102 · …
$ echo $?
3
$ ajan-bekci tara --temizle --kuru
KURU LİSTE — 2 hedef (yalnız K1/K2, uid …)
HEDEF … K1 …
--kuru: hiçbir sürece sinyal gönderilmedi
$ ajan-bekci tara --temizle --sadece <iki PID>
KAPANDI … SIGTERM
KAPANDI … SIGTERM
```

Anlatım: "Bekçi öldürülse bile kalanlar adı belli bir kafeste bekliyor. `tara` onları kesin kanıtla buluyor;
temizlik önce kuru liste, sonra yalnız senin onayladığın PID'ler."

⚠️ Sahne 3'ün çıktı satırları **temsilîdir**; biçim programın kullanım özetinden alındı,
gerçek çıktı kayıt provasında görülüp senaryo ona göre düzeltilmeli. `pgrep -x ajan-bekci` başka bir bekçi
oturumunu da gösterebilir: kayıt makinesinde başka bekçi açık olmamalı.

## Kayıtta söylenmeyecekler (iddia sınırı)

- "Gerçek MCP sunucuları hep böyle kaçırır" denmez: setsid torunu düzeneğin deseni, sıklığı ölçülmedi ([O1](olcumler.tr.md#o1)).
- "Windows'ta denendi" denmez: yalnız CI'da ölçüldü ([O5](olcumler.tr.md#o5)).
- "Bekçisiz açılan codex'in kalıntısını da temizler" denmez: yalnız K3 görünür, temizlenmez ([O3](olcumler.tr.md#o3)).

## Prova listesi (kayıt öncesi)

- [ ] Kayıt VM'de; sahne 1-3 çıktıları gerçek koşuda alındı, senaryodaki temsilî satırlar düzeltildi
- [ ] Kayıtta ev yolu, kullanıcı adı, makine adı, başka süreç komut satırı görünmüyor
- [ ] Kayıt sonunda `ps -C sleep` ile `910x` işaretlilerden kalan yok
