# Landing sayfası doğrulaması

2026-10-08 · antislop: geliştirme boyunca · Chrome headless, gerçek tarayıcı motoru.

## Canlı yayın

- PASS: https://videoplayer.serhatkochan.com/ oturumsuz gerçek Chrome'da HTTP 200 verdi; masaüstü ve mobil görüntüsü kontrol edildi.
- PASS: HTTP → HTTPS 308 yönlendirmesi; güvenilir TLS sertifikası ve otomatik yenileme etkin.
- PASS: Hostinger'da yalnız videoplayer CNAME kaydı eklendi, TTL 300. Diğer dokuz kayıt kümesinin aynı kaldığı bellekte karşılaştırıldı.
- PASS: Vercel video-player projesi serhatkochans-projects takımındadır; GitHub serhatkochan/video-player, rootDirectory website, outputDirectory public.
- PASS: canlı HTML, CSS, JS ve bütün görsellerin SHA-256 hashleri yerel kaynaklarla aynıdır; production-verification.json'da kayıtlıdır.
- PASS: canlı dialog/karşılaştırma, mobil taşma, CSP başlığı ve konsol kontrolü; hata sayısı 0.
- PASS: tasarım/test notları, Cargo.toml ve .env yayın dizininde sunulmuyor; HTTP 404 doğrulandı.
- PASS: gerçek kurulum EXE'si HTTP 200, Content-Length 165745450. İndirme bağlantısı hesap veya kaynak derlemesi gerektirmez.
- PASS: Hostinger API anahtarı yalnız bellekte kullanıldı; uygulama/site kaynaklarına veya dosyalara kaydedilmedi.

## Tarayıcı kanıtı

- PASS: 320×700, 375×812, 390×844, 600×900, 768×1024, 812×375, 900×900, 1024×768, 1440×960 ve 1920×1080. Sayfa yatay taşmıyor; görünür link ve düğmeler en az 44×44 CSS piksel.
- PASS: 720×450 ekran ve yüzde 200 kök metin ölçeği; metin taşması yok. Ekran görüntüsü ayrıca gözle incelendi.
- PASS: açılış, mobil, tablet, masaüstü ve büyütülmüş metin ekran görüntüleri root tarafından incelendi.
- PASS: konsol hata/uyarı sayısı 0; başarısız yerel ağ isteği 0.
- PASS: reduced-motion altında düğme transition süresi 0s. Otomatik video veya kayan içerik yok.
- PASS: JavaScript sözdizimi kontrolü (`node --check`). Statik dosyalar sunucuda çalıştırıldı; uygulama veya bundler derlemesi gerekmiyor.

## Öğelerin tek tek tıklanması

Harici kontroller izole test profilinde hedef URL'ye yönlendirildi. Test, büyük EXE'yi tekrar indirmek yerine yönlenmeyi yakalar; gerçek EXE erişimi ayrıca HTTP 200 ile doğrulandı.

- PASS: İçeriğe geç → #icerik; Tab ile görünür oluyor, Enter ile çalışıyor.
- PASS: marka → #baslangic.
- PASS: Özellikler → #ozellikler.
- PASS: Performans → #performans.
- PASS: üst İndir → #indir.
- PASS: üst GitHub → https://github.com/serhatkochan/video-player.
- PASS: Windows için indir → yayımlanmış v0.1.0 kurulum EXE'si.
- PASS: Yakından gör → native modal dialog; Kapat, Escape ve dış alan tıklaması kapatıyor; odak açan düğmeye dönüyor.
- PASS: Klavye kısayollarını gör → README'nin gerçek klavye kısayolları bölümü.
- PASS: Ölçüm yöntemi ve ham sonuçlar → v0.1.0/docs/performance.md.
- PASS: VLC/mpv karşılaştırması → native details açılıp kapanıyor; Enter ve Space çalışıyor; dar ekranda tablo ArrowRight ile kaydırılıyor.
- PASS: Karşılaştırmanın ham verileri → v0.1.0/docs/benchmarks/0.1.0-player-comparison.json.
- PASS: Kurulum EXE'sini indir → aynı çevrimdışı kurulum EXE'si.
- PASS: Sürüm notları ve tüm dosyalar → GitHub v0.1.0 release.
- PASS: Test kapsamını incele → v0.1.0/docs/acceptance/0.1.0.json.
- PASS: Kod: MIT lisansı → main/LICENSE.
- PASS: Sorun bildir → repo/issues.
- PASS: alt GitHub → repo.

## Antislop Delivery Gate

Root tarafından uygulanmış zorunlu kapı. Tasarım kararlarının gerekçesi DESIGN.md'de kayıtlıdır.

### Hard Gate

- R-02 PASS: kullanıcıya gösterilen metinde em dash yok.
- R-03 PASS: 10 ekran boyutu, yatay telefon ve yüzde 200 metin ölçeği kontrol edildi; sıfır sayfa taşması.
- R-17 PASS: 0.1.0 sürüm, 165,7 MB ve dört performans metriği gerçek release/JSON verilerinden alınmıştır.
- R-18 PASS: referans veya müşteri yorumu yok.
- R-23 PASS: kullanıcının yetkilendirdiği landing kapsamı; yalnız mevcut simge/ekran görüntüsü ve gerçek kaynaklardan gelen veriler kullanıldı.
- R-24 PASS: beş iç link hedefi tarayıcıda tıklandı ve ID'leri doğrulandı.
- R-25 PASS: antislop-human contrast-check.py ölçümleri; normal metin eşleşmeleri 16,34:1, 8,24:1, 9,46:1, 5,98:1 ve 11,86:1. Kontrol kenarı 4,01:1.
- R-26 PASS: iki dialog düğmesi, details kontrolü ve bütün linklerin gerçek davranışları kayıtlıdır.
- R-27 PASS: statik içerik; API, form veya dinamik veri yüklemesi yok. JS kapalıysa görüntü görünür, desteklenmeyen dialog açma düğmesi gizli; native details ve indirme linkleri JS gerektirmez.
- R-28 PASS: gereksiz FAQ bölümü yok.
- R-32 PASS: skip-link, odak halkaları, native modal focus, Enter/Space/Escape ve klavyeyle tablo kaydırma tarayıcıda doğrulandı.
- R-33 PASS: kaynak değişiklikleri apply_patch ile yapılmıştır; CSS/HTML string değiştiren harici düzenleme scripti kullanılmadı.
- R-34 PASS: tema anahtarı yok; koyu marka teması DESIGN.md'de gerekçeli, açık performans bölümünün kontrastı ayrıca ölçüldü.
- R-35 PASS: sayfa gerçek Chrome'da çalıştırıldı; yukarıdaki öğeler tek tek tıklandı; konsol hatası yok.
- R-36 PASS: ilk kare süresi veya dünya çapında üstünlük iddiası yok. Ön sürüm ve doğrulanmamış Windows/Explorer/HDR kapsamı açık.
- R-37 PASS: üretim öncesi Design Read ve üç tasarım dial'ı DESIGN.md'de belirtildi, kullanıcıya görsel yön açıklandı.
- R-38 PASS: gerçek arayüz, gerçek release ve ölçüm kaynakları; sahte video, takım, fiyat veya hayalet sayfa yok.

### Purpose Gate

- R-01 PASS: gradient veya glow yok.
- R-04 PASS: yalnız mevcut uygulama simgesi, indirme sembolü ve büyütme sembolü; her birinin işlevi DESIGN.md'de gerekçeli.
- R-06 PASS: Windows ile tutarlı Segoe UI; sıcak başlık sesi için Georgia italik; geniş monospace veya büyük harfli etiket yok.
- R-07 PASS: desen/grid/blueprint arka planı yok.
- R-08 PASS: süs amaçlı oklar yok.
- R-09 PASS: kapsül rozet yok; ön sürüm durumu doğrudan indirme metninde.
- R-10 PASS: glassmorphism yok.
- R-12 PASS: yalnız gerçek uygulama görüntüsünde tek gölge, koyu zeminden ayırmak için.
- R-13 PASS: glow yok.
- R-14 PASS: dört özellik açık liste; tek tip özellik kartı dizisi yok.
- R-19 PASS: MOTION 1; yalnız hover/press geçişi, reduced-motion kapatır.
- R-22 PASS: yeni veya stok illüstrasyon yok.

### Liveliness

- Dials PASS: ENERGY 2 / RHYTHM 3 / MOTION 1 açıkça belirtilmiştir.
- Tutarlılık PASS: büyük ürün başlığı, açık satır listesi, ters renkli ölçüm alanı ve son indirme bölümü farklı kompozisyonlardadır.
- Odak PASS: hero'da indirme ve gerçek ürün, özelliklerde kontroller, ölçümde 11,1 ms ve kapsam, sonda EXE eylemi.
- Boşluk PASS: mobil 24/48, geniş ekran 32/96 ritmi; içerik grupları çizgi ve boşlukla ayrılır.
- Aksan PASS: uygulamanın tek kum aksanı eylem/başlık vurgularında kullanılır.
- Kimlik motifi PASS: sıcak italik ikinci başlık satırı ve yatay editoryal ayrımlar tutarlı biçimde tekrarlanır.
- Design Read PASS: Windows yerel medya kullanıcıları, koyu sinema kimliği ve gerçek ürün kanıtı üretim öncesinde kayıtlıdır.

### Craftsmanship

- C-1 PASS: büyük görsel kararların tamamının DESIGN.md'de ürün temelli gerekçesi var.
- C-2 PASS: bütün kontroller davranış kontrolünden geçti.
- C-3 PASS: bölümler indirme, format desteği, oynatma özellikleri, küçük resim amacı, gerçek performans ve ön sürüm kapsamını karşılar.
- C-4 PASS: mobil/tablet/masaüstü, yatay ekran, metin büyütme, keyboard ve reduced-motion kontrol edildi.
- C-5 PASS: uydurulmuş yorum, istatistik veya karşılaştırma yok.
- R-05 PASS: hero + kart şablonu yerine ürün görüntüsü, format satırları, özellik listesi ve ölçüm tablosu; değişen bölüm ritmi.
- R-11 PASS: düğmeler 5px, görüntü 10px, dialog 8px; kapsül kalıbı yok.
- R-15 PASS: Windows için indir, EXE'yi indir, test kapsamını incele gibi gerçek eylem metinleri.
- R-16 PASS: AI pazarlama sıfatları veya evrensel hız üstünlüğü iddiası yok.
- R-20 PASS: gerçek Video Player ekranı, marka aksanı, Windows thumbnail amacı ve kaynaklı ölçümler ürüne özgüdür.
- R-21 PASS: uygulamanın mevcut koyu kimliği ve video izleme bağlamı sabit temayı gerekçelendirir.
- R-29 PASS: nötrlerin yanında yalnız mevcut #D8B08C aksanı; açık ölçüm alanı aynı sıcak renk ailesi.
- R-30 PASS: başka ürünün arayüzü taklit edilmedi; tasarım mevcut Video Player simgesinden türetildi.
- R-31 PASS: palet, düzen, tipografi, boşluk, kart kullanımı, varlıklar, simgeler, hareket ve stack için birer satır gerekçe DESIGN.md'de bulunur.
