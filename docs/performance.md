# Performans ölçümleri

Video Player 0.1.0 için 7 Ekim 2026’da (UTC) alınan yerel ölçümler. Türkiye saatine göre son tek-oynatıcı ölçümü 8 Ekim 2026’da tamamlandı. Her senaryoda bir ısınma denemesi ayrıldı, ardından yedi denemenin medyanı hesaplandı. İşletim sistemi, dosya ve GPU önbellekleri temizlenmedi.

## Test koşulları

| Bileşen | Değer |
| --- | --- |
| Windows | Windows 11 Pro, derleme 26200 |
| İşlemci | Intel Core i9-13900K, 24 çekirdek / 32 mantıksal işlemci |
| Ekran kartı | NVIDIA GeForce RTX 4080 |
| GPU sürücüsü | 32.0.15.9186 |
| Bellek | 32 GB kurulu; sistemin bildirdiği 31,71 GiB |
| Örnek video | 1920×1080, 30 kare/s, 60 saniye, H.264 + AAC, MP4 |
| Donanım çözme | `d3d11va` |
| Derleme | Rust `release` profili |

Örnek video FFmpeg’in `testsrc2` ve `sine` kaynaklarından üretildi. Kişisel video veya ses kullanılmadı. Video dosyasının ve çalıştırılan EXE’nin SHA-256 değerleri [ham sonuçlarda](benchmarks/0.1.0-windows-x64.json) bulunur.

## Sonuçlar

| Ölçüm | Medyan | En düşük / en yüksek |
| --- | ---: | ---: |
| Süreç başlatma → pencere tanıtıcısının oluşması, boş açılış | 30,2 ms | 29,7 / 31,1 ms |
| Yerel yüzey ve libmpv başlatma | 11,1 ms | 10,2 / 12,1 ms |
| Dosya yükleme → ilk oynatma ilerlemesi bildirimi | 325,2 ms | 315,6 / 353,4 ms |
| Duraklatılmış videoda 30. saniyeye hassas sarma konumunun onayı | 20,5 ms | 17,1 / 21,5 ms |
| Boşta CPU, makinenin toplam kapasitesi | %0,099 | %0,081 / %0,115 |
| 1080p oynatmada CPU, makinenin toplam kapasitesi | %0,246 | %0,130 / %0,328 |
| Boşta RAM, çalışma kümesi | 154,6 MiB | 154,4 / 154,9 MiB |
| 1080p oynatmada RAM, çalışma kümesi | 243,8 MiB | 243,6 / 245,4 MiB |
| Boşta özel bellek, taahhüt edilen alan | 385,8 MiB | 384,6 / 386,8 MiB |
| 1080p oynatmada özel bellek, taahhüt edilen alan | 524,2 MiB | 523,3 / 526,1 MiB |

## Neyi ölçüyor?

GUI ölçümü, gerçek dağıtım EXE’sini uygulamanın `--smoke-report` modu ile normal, görünür pencerede çalıştırır. Pencere metriği `MainWindowHandle` değerinin sıfırdan farklı hale gelmesini ölçer. İlk çizilen arayüz karesini, tıklamaya hazır hale gelmeyi veya videonun ekrana gelmesini ölçmez. Bu nedenle 30,2 ms bir tam açılış süresi olarak sunulmaz.

CPU ve bellek, süreç başlatıldıktan sonraki 2–5. saniyeler arasında örneklenir. CPU hesabı `sürecin CPU zamanı / geçen süre / 32 mantıksal işlemci × 100` şeklindedir; Windows Görev Yöneticisi’ndeki toplam işlemci kapasitesi yaklaşımını kullanır. Örneğin %0,246 toplam kullanım, tek mantıksal işlemci kapasitesine göre yaklaşık %7,87’ye karşılık gelir. Bellek sayıları GPU VRAM’ini içermez; çalışma kümesi ile taahhüt edilen özel bellek ayrı raporlanır.

Motor ölçümü `performance_probe` geliştirici örneğinde gerçek `VideoHost` ve `Mpv` modüllerini kullanır. Her denemede yeni bir motor ve gizli 960×540 Windows video yüzeyi oluşturulur. İlk ilerleme, doğru dosyaya ait duraklatılmamış ve boşta olmayan bir durum bildiriminde süre ve oynatma konumunun sıfırdan büyük olmasıdır. Hassas sarma, duraklatılmış oynatıcıda konum bildiriminin istenen 30. saniyeye 0,05 saniye yakınlığa ulaşmasıdır.

İlerleme ve sarma ölçümleri, ekrana çizilen karenin zamanını doğrulamaz. Asenkron libmpv bildirimleri, 3 ms örnekleme aralığı ve Windows zamanlayıcısı sonuçları etkiler. Bu motor süreleri tek cihaz ve tek 1080p örnek içindir; farklı donanımlar, soğuk açılış, HDR veya 4K/8K hakkında sonuç oluşturmaz. Diğer oynatıcılarla kaynak kullanımı karşılaştırması aşağıda ayrı raporlanır.

## Oynatıcı karşılaştırması

[Karşılaştırmanın ham raporu](benchmarks/0.1.0-player-comparison.json), 7 Ekim 2026’da (UTC) aynı Windows cihazında ve aynı sentetik dosyayla alınan yedi denemeyi içerir. Her oynatıcı ve senaryo için bir ısınma denemesi ölçüm dışında bırakıldı. İşletim sistemi, dosya ve GPU önbellekleri temizlenmedi; oynatıcıların çalışma sırası dönüşümlüydü.

| Oynatıcı | Sürüm |
| --- | --- |
| Video Player | 0.1.0, yayımlanacak uygulama ve libmpv DLL’i |
| VLC | 3.0.23 |
| Bağımsız mpv | `v0.41.0-1104-geb0ee1031` |

Video Player ve bağımsız mpv aynı motor ailesini kullanır; arayüzleri, medya derlemeleri ve ayarları farklıdır. Bu kıyas süreç kaynaklarını ölçer, motorları tek başına sıralamaz. Tek-oynatıcı ve karşılaştırma betikleri ayrı oturumlarda çalıştı; bu yüzden Video Player’ın kaynak kullanımı medyanları iki tabloda farklıdır.

### Video açılarak başlatılan süreçler

| Oynatıcı | Pencere tanıtıcısı, ms | RAM, çalışma kümesi | CPU, toplam makine kapasitesi |
| --- | ---: | ---: | ---: |
| Video Player | 28,7 | 243,9 MiB | %0,196 |
| VLC | 131,1 | 170,1 MiB | %0,082 |
| mpv | 198,2 | 158,9 MiB | %0,146 |

Tablo medyanları gösterir. Video Player’ın pencere tanıtıcısı bu koşullarda daha erken oluştu; VLC ve mpv daha az RAM ve CPU kullandı. `MainWindowHandle` ölçümü ilk çizilen arayüz veya video karesini, tıklamaya hazır olmayı ya da toplam açılış süresini göstermez.

### Dosya açılmadan boşta çalışan süreçler

| Oynatıcı | Pencere tanıtıcısı, ms | RAM, çalışma kümesi | CPU, toplam makine kapasitesi |
| --- | ---: | ---: | ---: |
| Video Player | 39,3 | 154,5 MiB | %0,081 |
| VLC | 132,6 | 35,4 MiB | %0,000 |
| mpv | 202,6 | 100,1 MiB | %0,000 |

Sıfıra yuvarlanan CPU değerleri yalnızca bu kısa örnekleme aralığına aittir. Arayüzlerin boyutları ve çizdiği içerik farklıdır; mpv için `autofit=1100x720` kullanıldı.

### Ayarlar ve oynatma doğrulaması

- CPU ve bellek her süreç için başlatmadan sonraki 2–5. saniyeler arasında örneklendi. CPU 32 mantıksal işlemcinin toplam kapasitesine göre hesaplandı. RAM GPU VRAM’ini içermez; özel bellek ayrıca ham raporda bulunur.
- Her denemede aynı videonun yeni, baytları değişmeyen bir dosya adı kullanıldı. Üç oynatıcıya aynı dosya verildi; Video Player’ın devam geçmişi videoyu farklı bir konumdan başlatmadı.
- VLC, çalışma dizinindeki taşınabilir kopyayla ve çekirdek ayarları yüklenmeden çalıştı. Kurulu VLC’nin kişisel ayarları kullanılmadı. Döngü adresindeki RC denetimi oynatma durumunu ve ilerleyen süreyi doğruladı.
- mpv kişisel ayarlar ve harici betikler yüklenmeden, `gpu-next`, D3D11 ve `hwdec=auto-safe` ile çalıştı. Bunlar bu test için seçilen ayarlardır. Adlandırılmış kanal üzerinden doğru dosya, H.264, ilerleyen süre ve `d3d11va` doğrulandı.
- Video Player’ın `--smoke-report` modu yerel video yüzeyini, H.264, oynatma ilerlemesini ve `d3d11va` durumunu her denemede doğruladı.
- VLC’nin D3D11VA kullanımı yalnızca ayrılan ısınma denemesinin günlüğünde doğrulandı. Ölçülen denemeler aynı çözme ayarlarını kullandı; bu denemelerde dosyaya günlük yazma kapalıydı.
- Video Player’ın ölçüm modu, VLC’nin RC denetimi ve mpv’nin IPC kanalı süreçlerde etkin kaldı. Denetim sorguları kaynak örneklemesinden sonra gönderildi. Bu ek araçlar ve farklı arayüzler sonuçları etkileyebilir.

Bu kıyas bir bilgisayar ve bir dosyayla sınırlıdır. Görüntü kalitesi, ilk video karesi, sarma gecikmesi, HDR veya 4K/8K kıyası yapılmadı. EXE ve kullanılan medya DLL’i hashleri, ham denemeler, örnek sayıları ve en düşük/en yüksek değerler raporda korunur.

## Tekrar ölçme

Rust, Windows SDK ve MSVC derleme araçlarını hazırladıktan sonra:

```powershell
./scripts/Get-Runtime.ps1
cargo build --workspace --release --locked
./scripts/Measure-Performance.ps1 -Runs 7
```

Betik, örnek dosya yoksa [yayımlanan sentetik H.264/AAC dosyasını](https://github.com/serhatkochan/video-player/releases/download/v0.1.0/benchmark-h264-1080p30.mp4) indirip sabit SHA-256 değerini doğrular. Sonuçlar `test-media/performance/results/performance.json` altında tutulur; bu klasör Git’e eklenmez. Kurulu ya da başka bir derlenmiş uygulamayı ölçmek için `-Executable`, `-RuntimeDirectory` ve `-MediaPath` parametreleri kullanılabilir.

Karşılaştırmayı çalıştırmak için VLC’yi hazırla, bağımsız mpv EXE’sinin yolunu belirt ve şunları çalıştır:

```powershell
./scripts/Build-Windows.ps1
./scripts/Measure-Performance.ps1 -Runs 7
./scripts/Measure-PlayerComparison.ps1 -Runs 7 -MpvExecutable "C:\mpv\mpv.exe"
```

`-MpvExecutable` verilmezse yalnızca Video Player ve VLC ölçülür. VLC farklı bir dizindeyse `-VlcDirectory` kullan. Karşılaştırma varsayılan olarak `dist/stage/video-player.exe` dosyasını çalıştırır ve raporu `test-media/performance/comparison/comparison.json` altında oluşturur.

Yalnızca motoru ölçmek için:

```powershell
cargo run -p video-player --example performance_probe --release --locked -- ./test-media/performance/h264-1080p30.mp4 ./runtime ./test-media/performance/native.json 7
```

Ölçüm, test için açtığı süreçleri kendiliğinden kapatır. Test dosyasının devam bilgisi uygulamanın yerel geçmişine eklenebilir. Diğer açık Video Player süreçleri kapatılmaz.
