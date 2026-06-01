<div align="center">

<img src="docs/hero.webp" alt="VibeTerm" width="820">

# VibeTerm

**Her yerde kıvılcımlar, bir bakışta net.**

Vibe coding için modern bir terminal yöneticisi. Önce yerel, CJK doğal. Ajanları bırak koşsunlar — hangisi yanıyor, hangisi söndü, hangisi seni çağırıyor, hepsi göz önünde. Müdahale yok, giriş yok, bulut yok.

[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)
[![Platform](https://img.shields.io/badge/platform-macOS%20%7C%20Windows-lightgrey.svg)](https://github.com/fjlmcm/VibeTerm/releases)
[![Release](https://img.shields.io/github/v/release/fjlmcm/VibeTerm?color=success)](https://github.com/fjlmcm/VibeTerm/releases)
[![Stack](https://img.shields.io/badge/Tauri%202-Rust%20%C2%B7%20SolidJS-ffc131.svg)](https://github.com/fjlmcm/VibeTerm)

[**www.vibeterm.org**](https://www.vibeterm.org) · [**İndir**](https://github.com/fjlmcm/VibeTerm/releases) · [**GitHub**](https://github.com/fjlmcm/VibeTerm)

[English](README.md) · [简体中文](README.zh.md) · [繁體中文](README.zh-hant.md) · [日本語](README.ja.md) · [한국어](README.ko.md) · [Tiếng Việt](README.vi.md) · [Bahasa Indonesia](README.id.md) · [Español](README.es.md) · [Português](README.pt-br.md) · [Deutsch](README.de.md) · [Français](README.fr.md) · [Italiano](README.it.md) · [Русский](README.ru.md) · **Türkçe**

</div>

---

## Ajanlarını senin yerine çalıştırmaz. Sadece onlara göz kulak olur.

- **Yapılandırmana asla dokunmaz** — Durumu izleyerek anlar: çıktıyı okur, dosyaları salt okunur izler. ~/.claude veya ~/.codex'e asla yazmaz, hook kurmaz, arka plan servisi başlatmaz. Ajan yapılandırmanın tek baytına bile dokunulmaz.
- **Bir sürü ajanı dizginler** — Ajan sayısı artınca işler karışır. Takılanları ve seni bekleyenleri yukarı çıkarır, böylece kime bakman gerektiğini görmek için hepsini tek tek açmana gerek kalmaz.
- **Terminal terminal kalır** — Terminalin temel işlerini iyi yapar. Özellik şişkinliği yok, ajan tezgâhına dönüşme hevesi yok.
- **Çalışan CJK** — Geniş karakterler, IME girişi, içinde emoji olan kopyalama — İngilizce terminallerin sürekli yanlış yaptığı şeyler, burada doğru çözülmüş.
- **Her şey senin makinende kalır** — Giriş yok, veri toplama yok, varsayılan çevrimdışı. Yalnızca sen güncelleme kontrol ettiğinde çevrimiçi olur, o zaman da sadece okur.
- **MIT, açık kaynak** — Tüm kod açık. Oku, değiştir, ne istersen.

## Beş durum, bir bakışta net.

- 🔵 **Çalışıyor** — Parıltılı sabit mavi nokta. Ajan çalışıyor.
- 🟡 **Bekliyor** — Nefes alıp veren amber nokta. Seni bekliyor, bir bakmaya değer.
- 🔴 **Takıldı** — Kırmızı-turuncu halka. 5 dakikadan uzun sessiz, muhtemelen takıldı.
- ⚪ **Boşta** — Hareketsiz gri nokta. Bir şey olmuyor.
- 🟢 **Bitti** — Üstü çizili, dış hatlı halka. Bu iş gerçekten bitti.

## Bir terminalin yapması gereken her şey, artı ajan kısmı.

_Her zamanki terminal özellikleri, artı ekranı dolduran yapay zekâ ajanları için durum farkındalığı ve düzenleme._

### Ajanlar

- **Bir ajanın ne yaptığını görür** — Çalışıyor, bekliyor, takıldı ya da bitti — yapılandırmana dokunmadan anlaşılır.
- **Takılma algılama + aciliyet sıralaması** — Ekran ajanlarla mı dolu? Takılanlar ve seni bekleyenler en üste çıkar.
- **Canlı kullanım** — Kalan bağlam, 5h/7d kota, yakma hızı, önbellek, maliyet — hepsi tek barda.
- **Kullanım istatistikleri** — Claude / Codex için token ve maliyet rakamları. Çevrimdışı hesaplanır, dışa aktarılabilir.

### Terminal

- **Bölmeler + worktree** — Bir git worktree bağla, görev başına bir terminal ağacı.
- **Canvas panosu** — Görevleri kart olarak diz, çerçeveyle seç, tek komutu birden çok terminale gönder.
- **Yüzen pencereler** — Herhangi bir görevi kendi penceresine çıkar ve izlemeye devam et.
- **GPU ile çizim** — WebGL hızlandırmalı, yine de CJK gliflerini düşürmez ve takılmaz.

### Akış

- **Komut paleti** — Özel kısayollar ve eylemler. Hepsini klavyeden yönet.
- **Prompt şablonları** — claude / codex / shell için kullanışlı hazır şablonlar, bir tuş ötede.
- **Yapılandırılabilir durum çubuğu** — Widget'ları sürükleyerek diz; her ajan türünün kendi düzeni var.
- **Masaüstü bildirimleri** — 24 yerleşik ses + sessiz saatler, yalnızca bir ajanın durumu değiştiğinde.
- **Anında tema değişimi** — 10 yerleşik tema, istediğin zaman değiştir, macOS ve Windows.

## Yapılandırmana dokunmadan bir ajanın ne yaptığını nasıl bilir?

Üç izleme yolu, artı salt okunur dosya izleme. Hook yok, giriş yok, hiçbir şey yazılmaz.

1. **OSC 133 / 633 dizileri** — Kabuk entegrasyonundan gelen komut sınırı işaretleri. En güvenilir katman: bir komutun ne zaman başladığını, bittiğini ya da giriş beklediğini tam olarak bilir.
2. **Ajan çıktısını okuma** — 11 yaygın ajanın onay istemlerini eşleştirerek birinin seni ne zaman beklediğini anlar.
3. **Başlıktaki o döner ikon** — Pencere başlığındaki braille döner ikon dönüyorsa, ajan çalışıyor.

> **Kural: senin şeylerine dokunmak yok** — ~/.claude veya ~/.codex'e asla yazmaz, hook kurmaz, arka plan servisi başlatmaz. Her durum gözlemlenir, asla içeri sokulmaz.

## Büyük İngilizce yapay zekâ terminallerinin hiçbiri CJK'yi ciddiye almıyor.

Neredeyse her büyük yapay zekâ terminali deposunda, İngilizce kullanıcıların acil işlerinin altına gömülmüş hâlâ açık CJK hataları var. Bu işi kimse gerçekten yapmadı. VibeTerm bunu gerçek bir iş olarak ele alıyor.

- IME birleştirme baştan sona yakalanır (isComposing / keyCode 229). Yanlış gönderim yok, gecikme yok.
- Geniş ve belirsiz genişlikler doğru ölçülür, tablolar kaymaz.
- Satır sarma kesintisiz; akış asla bir glifi bölmez.
- Kopyalama Intl.Segmenter ile korunur, surrogate çiftlerini ya da ZWJ'yi parçalamaz.
- GPU çiziminde CJK düşmez ve kaymaz.

## Bir denesene?

macOS 11+ ve Windows, aynı indirme sayfasından.

**[İndir →](https://github.com/fjlmcm/VibeTerm/releases)** — macOS `.dmg` · Windows `.exe` / `.msi`.

Ya da kaynaktan derle:

```bash
pnpm install
pnpm build      # = tauri build → src-tauri/target/release/bundle/
pnpm dev        # dev (Vite HMR + tauri dev)
```

Built with **Tauri 2 · Rust · SolidJS · xterm.js** (pnpm monorepo).

## Bunların omuzlarında.

ryoppippi'nin ccusage'ına (MIT) özel teşekkürler. Kullanım istatistikleri, model fiyatları ve 5 saatlik bloklar oradan geldi; fiyat verileri LiteLLM ve Anthropic'in resmi rakamlarından.

Also building on [Tauri](https://tauri.app) · [SolidJS](https://solidjs.com) · [xterm.js](https://xtermjs.org) · [WezTerm](https://github.com/wezterm/wezterm) · [Tabby](https://github.com/Eugeny/tabby). Full list in [THIRD-PARTY-NOTICES.md](THIRD-PARTY-NOTICES.md).

## MIT License

[MIT](LICENSE) · © 2026 VibeTerm contributors
