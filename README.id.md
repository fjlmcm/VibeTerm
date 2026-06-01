<div align="center">

<img src="docs/hero.webp" alt="VibeTerm" width="820">

# VibeTerm

**Percikan di mana-mana, jelas dalam sekali lihat.**

Pengelola terminal modern untuk vibe coding. Lokal dulu, CJK native. Biarkan agent berjalan — mana yang berkobar, mana yang padam, mana yang memanggilmu, semua terlihat. Tanpa intrusi, tanpa login, tanpa cloud.

[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)
[![Platform](https://img.shields.io/badge/platform-macOS%20%7C%20Windows-lightgrey.svg)](https://github.com/fjlmcm/VibeTerm/releases)
[![Release](https://img.shields.io/github/v/release/fjlmcm/VibeTerm?color=success)](https://github.com/fjlmcm/VibeTerm/releases)
[![Stack](https://img.shields.io/badge/Tauri%202-Rust%20%C2%B7%20SolidJS-ffc131.svg)](https://github.com/fjlmcm/VibeTerm)

[**www.vibeterm.org**](https://www.vibeterm.org) · [**Unduh**](https://github.com/fjlmcm/VibeTerm/releases) · [**GitHub**](https://github.com/fjlmcm/VibeTerm)

[English](README.md) · [简体中文](README.zh.md) · [繁體中文](README.zh-hant.md) · [日本語](README.ja.md) · [한국어](README.ko.md) · [Tiếng Việt](README.vi.md) · **Bahasa Indonesia** · [Español](README.es.md) · [Português](README.pt-br.md) · [Deutsch](README.de.md) · [Français](README.fr.md) · [Italiano](README.it.md) · [Русский](README.ru.md) · [Türkçe](README.tr.md)

</div>

---

## Tidak menjalankan agent-mu untukmu. Hanya mengawasinya.

- **Tak pernah menyentuh konfigmu** — Status diketahui dengan mengamati: membaca output, memantau berkas secara hanya-baca (tanpa menulis). Tak pernah menulis ke ~/.claude atau ~/.codex, tak memasang hook, tak menjalankan layanan latar. Konfigurasi agent-mu sama sekali tak tersentuh.
- **Menjaga belasan agent tetap terkendali** — Begitu agent-nya banyak, langsung berantakan. Yang macet dan yang menunggumu dinaikkan ke atas, jadi kamu tak perlu membuka satu per satu untuk lihat siapa yang butuh kamu.
- **Terminal tetap terminal** — Mengerjakan dasar-dasar terminal dengan baik. Tanpa fitur berlebihan, tanpa ambisi jadi bengkel agent.
- **CJK yang benar-benar jalan** — Karakter lebar, input IME, menyalin teks ber-emoji — hal yang terus salah di terminal berbahasa Inggris, di sini beres.
- **Semua tetap di mesinmu** — Tanpa login, tanpa pengumpulan data, offline secara bawaan. Hanya online saat kamu sendiri mengecek pembaruan, itu pun cuma membaca, tanpa mengirim apa pun.
- **MIT, open source** — Seluruh kode terbuka. Baca, ubah, terserah kamu.

## Lima status, jelas sekali lihat.

- 🔵 **Berjalan** — Titik biru menyala tetap. Agent sedang bekerja.
- 🟡 **Menunggu** — Titik kuning bernapas. Sedang menunggumu, perlu kamu cek.
- 🔴 **Macet** — Cincin merah-oranye. Sunyi lebih dari 5 menit, mungkin macet.
- ⚪ **Diam** — Titik abu-abu diam. Tak ada yang terjadi.
- 🟢 **Selesai** — Cincin bergaris, dicoret. Yang ini benar-benar selesai.

## Semua yang harus dilakukan terminal, plus urusan agent-nya.

_Semua fitur terminal biasa, plus kesadaran status dan orkestrasi untuk layar penuh agent AI._

### Agent

- **Melihat agent sedang apa** — Berjalan, menunggu, macet, atau selesai — diketahui tanpa menyentuh konfigmu.
- **Deteksi macet + urut berdasarkan urgensi** — Layar penuh agent? Yang macet dan yang menunggumu naik ke atas.
- **Pemakaian langsung** — Sisa konteks, kuota 5h/7d, laju pemakaian, cache, biaya — semua dalam satu bar.
- **Statistik pemakaian** — Angka token dan biaya untuk Claude / Codex. Dihitung offline, bisa diekspor.

### Terminal

- **Split panel + worktree** — Pasang git worktree, satu pohon terminal per tugas.
- **Papan Canvas** — Tata tugas sebagai kartu, pilih dengan kotak, kirim satu perintah ke beberapa terminal.
- **Jendela mengambang** — Lepas tugas mana pun ke jendelanya sendiri dan terus awasi.
- **Render GPU** — Dipercepat WebGL, dan CJK tetap tak menjatuhkan glyph atau tersendat.

### Alur

- **Palet perintah** — Pintasan dan aksi yang bisa diatur. Semua bisa dikendalikan dari keyboard.
- **Templat prompt** — Preset praktis untuk claude / codex / shell, sekali tekan.
- **Bar status yang bisa diatur** — Seret widget untuk menatanya; tiap jenis agent punya pengaturan sendiri.
- **Notifikasi desktop** — 24 suara bawaan + jam tenang, hanya saat status agent berubah.
- **Ganti tema seketika** — 10 tema bawaan, ganti kapan saja, macOS dan Windows.

## Bagaimana cara tahu agent sedang apa tanpa menyentuh konfigmu?

Tiga cara mengamati, plus pemantauan berkas hanya-baca. Tanpa hook, tanpa login, tanpa menulis apa pun.

1. **Urutan OSC 133 / 633** — Penanda batas perintah dari integrasi shell. Lapisan paling andal: tahu persis kapan perintah mulai, selesai, atau menunggu input.
2. **Membaca output agent** — Mencocokkan prompt izin dari 11 agent umum untuk tahu kapan salah satunya menunggumu.
3. **Spinner di judul itu** — Kalau spinner braille di judul jendela berputar, agent sedang bekerja.

> **Garis merahnya: tidak menyentuh apa pun milikmu.** — Tak pernah menulis ke ~/.claude atau ~/.codex, tak memasang hook, tak menjalankan layanan latar. Setiap status diamati, tak pernah disuntikkan.

## Tak satu pun terminal AI besar berbahasa Inggris yang sungguh-sungguh menggarap CJK.

Hampir tiap repo terminal AI besar punya bug CJK yang masih terbuka, terkubur di bawah hal-hal mendesak pengguna berbahasa Inggris. Tak ada yang sungguh-sungguh mengerjakan bagian ini. VibeTerm menganggapnya pekerjaan nyata.

- Komposisi IME dicegat sampai akhir (isComposing / keyCode 229). Tanpa salah kirim, tanpa lag.
- Lebar penuh dan ambigu diukur benar, tabel tetap rapi.
- Pembungkusan baris tak terpotong; streaming tak pernah memotong glyph.
- Penyalinan dijaga Intl.Segmenter, tak merobek pasangan surrogate atau ZWJ.
- CJK tak hilang atau bergeser saat render GPU.

## Cobain, yuk?

macOS 11+ dan Windows, dari halaman yang sama.

**[Unduh →](https://github.com/fjlmcm/VibeTerm/releases)** — macOS `.dmg` · Windows `.exe` / `.msi`.

Atau build dari kode sumber:

```bash
pnpm install
pnpm build      # = tauri build → src-tauri/target/release/bundle/
pnpm dev        # dev (Vite HMR + tauri dev)
```

Built with **Tauri 2 · Rust · SolidJS · xterm.js** (pnpm monorepo).

## Di atas bahu proyek-proyek ini.

Terima kasih khusus untuk ccusage dari ryoppippi (MIT). Statistik pemakaian, harga model, dan blok 5 jam berasal dari sana; data harga dari LiteLLM dan angka resmi Anthropic.

Also building on [Tauri](https://tauri.app) · [SolidJS](https://solidjs.com) · [xterm.js](https://xtermjs.org) · [WezTerm](https://github.com/wezterm/wezterm) · [Tabby](https://github.com/Eugeny/tabby). Full list in [THIRD-PARTY-NOTICES.md](THIRD-PARTY-NOTICES.md).

## MIT License

[MIT](LICENSE) · © 2026 VibeTerm contributors
