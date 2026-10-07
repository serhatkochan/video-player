# Video Player landing page

Design Read: Windows 11 kullanıcıları için, gerçek uygulamayı ve doğrudan indirmeyi öne çıkaran bir ürün sayfası; uygulamanın sıcak, koyu sinema kimliği. ENERGY 2 / RHYTHM 3 / MOTION 1.

- Palet: mevcut uygulama ikonundaki #D8B08C kum tonu; #101112 ve #191A1C nötrleri. Marka devamlılığı ve karanlıkta video izleme bağlamı sabit koyu temayı gerekçelendirir.
- Düzen: indirme ve gerçek ekran görüntüsü yan yana; ardından format satırları, açık özellik listesi, aydınlık performans alanı ve indirme bölümü. İçerik türleri ritmi değiştirir.
- Tipografi: Segoe UI sistem ailesi Windows ürününü tanıdık kılar; yalnız başlıklardaki Georgia italik sıcak marka sesini oluşturur. Harici font isteği yok.
- Boşluk: mobilde 24/48, geniş ekranda 32/96 ölçeği; ürün görüntüsü ve ölçüm tablosu kendi genişliklerini kullanır.
- Kartlar: özellikler kartlara bölünmez; yalnız gerçek uygulama görüntüsü ve ön sürüm kapsamı sınırlandırılır.
- Varlıklar: mevcut uygulama SVG simgesi ve 1100×720 gerçek açılış görüntüsü. Yeni logo, sahte video karesi, Explorer görüntüsü veya referans üretilmez.
- Simgeler: yalnız indirme eylemi ve görüntü büyütme eylemi için açık anlamlı SVG; diğer gezinme metinle yapılır.
- Hareket: kısa hover/press geçişleri; kaydırma gösterileri veya otomatik medya yok. Reduced motion geçişleri kapatır.
- Stack: mevcut Rust deposuna bağımsız HTML/CSS/JS; tek sayfanın sunucu veya framework bağımlılığı ihtiyacı yok.

ui-ux-pro-max araştırması: `native video player cinema premium --design-system`. Hero-Centric / Dark Mode (OLED) eşleşmesi ürün bağlamına uygundur. Önerilen mor/kırmızı palet, Inter ve parıltı yerine mevcut marka rengi, sistem fontu ve sade yüzeyler kullanılır. Bunlar öneri olup marka ve antislop gereksinimleri önceliklidir.

İçerik kanıtları: README, docs/benchmarks/0.1.0-windows-x64.json, docs/benchmarks/0.1.0-player-comparison.json ve v0.1.0 GitHub Releases. 0.1.0 ön sürümdür. İlk kare, evrensel hız üstünlüğü, HDR/Explorer matrisi doğrulanmış gibi sunulmaz.
