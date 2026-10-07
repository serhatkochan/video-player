# Performans ölçümleri

Video Player 0.1.0 için 7 Ekim 2026’da alınan yerel ölçümler. Her senaryoda bir ısınma denemesi ayrıldı, ardından yedi denemenin medyanı hesaplandı. İşletim sistemi, dosya ve GPU önbellekleri temizlenmedi.

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
| --- | --- |
| Süreç başlatma → pencere tanıtıcısının oluşması, boş açılış | 29,9 ms | 28,6 / 35,8 ms |
| Yerel yüzey ve libmpv başlatma | 13,8 ms | 13,4 / 15,8 ms |
| Dosya yükleme → ilk oynatma ilerlemesi bildirimi | 324,0 ms | 318,4 / 340,5 ms |
| Duraklatılmış videoda 30. saniyeye hassas sarma konumunun onayı | 13,1 ms | 12,2 / 19,8 ms |
| Boşta CPU, makinenin toplam kapasitesi | %0,097 | %0,080 / %0,146 |
| 1080p oynatmada CPU, makinenin toplam kapasitesi | %0,195 | %0,113 / %0,260 |
| Boşta RAM, çalışma kümesi | 160,5 MiB | 160,4 / 160,7 MiB |
| 1080p oynatmada RAM, çalışma kümesi | 311,3 MiB | 251,0 / 317,7 MiB |
| Boşta özel bellek, taahhüt edilen alan | 392,1 MiB | 391,2 / 392,7 MiB |
| 1080p oynatmada özel bellek, taahhüt edilen alan | 591,5 MiB | 530,9 / 598,0 MiB |

## Neyi ölçüyor?

GUI ölçümü, gerçek dağıtım EXE’sini uygulamanın `--smoke-report` modu ile çalıştırır. Pencere metriği `MainWindowHandle` değerinin sıfırdan farklı hale gelmesini ölçer. İlk çizilen arayüz karesini, tıklamaya hazır hale gelmeyi veya videonun ekrana gelmesini ölçmez. Bu nedenle 29,9 ms bir tam açılış süresi olarak sunulmaz.

CPU ve bellek, süreç başlatıldıktan sonraki 2–5. saniyeler arasında örneklenir. CPU hesabı `sürecin CPU zamanı / geçen süre / 32 mantıksal işlemci × 100` şeklindedir; Windows Görev Yöneticisi’ndeki toplam işlemci kapasitesi yaklaşımını kullanır. Örneğin %0,195 toplam kullanım, tek mantıksal işlemci kapasitesine göre yaklaşık %6,23’e karşılık gelir. Bellek sayıları GPU VRAM’ini içermez; çalışma kümesi ile taahhüt edilen özel bellek ayrı raporlanır.

Motor ölçümü `performance_probe` geliştirici örneğinde gerçek `VideoHost` ve `Mpv` modüllerini kullanır. Her denemede yeni bir motor ve gizli 960×540 Windows video yüzeyi oluşturulur. İlk ilerleme, doğru dosyaya ait duraklatılmamış ve boşta olmayan bir durum bildiriminde süre ve oynatma konumunun sıfırdan büyük olmasıdır. Hassas sarma, duraklatılmış oynatıcıda konum bildiriminin istenen 30. saniyeye 0,05 saniye yakınlığa ulaşmasıdır.

İlerleme ve sarma ölçümleri, ekrana çizilen karenin zamanını doğrulamaz. Asenkron libmpv bildirimleri, 3 ms örnekleme aralığı ve Windows zamanlayıcısı sonuçları etkiler. Bu ölçümler tek cihaz ve tek 1080p örnek içindir; farklı donanımlar, soğuk açılış, HDR, 4K/8K veya diğer oynatıcılarla karşılaştırma için sonuç oluşturmaz.

## Tekrar ölçme

Rust, Windows SDK ve MSVC derleme araçlarını hazırladıktan sonra:

```powershell
./scripts/Get-Runtime.ps1
cargo build --workspace --release --locked
./scripts/Measure-Performance.ps1 -Runs 7
```

Betik, örnek yoksa 60 saniyelik H.264/AAC video üretir. Sonuçlar `test-media/performance/results/performance.json` altında tutulur; bu klasör Git’e eklenmez. Kurulu ya da başka bir derlenmiş uygulamayı ölçmek için `-Executable`, `-RuntimeDirectory` ve `-MediaPath` parametreleri kullanılabilir.

Yalnızca motoru ölçmek için:

```powershell
cargo run -p video-player --example performance_probe --release --locked -- ./test-media/performance/h264-1080p30.mp4 ./runtime ./test-media/performance/native.json 7
```

Ölçüm, test için açtığı süreçleri kendiliğinden kapatır. Test dosyasının devam bilgisi uygulamanın yerel geçmişine eklenebilir. Diğer açık Video Player süreçleri kapatılmaz.
