<div align="center">

<img src="docs/hero.webp" alt="VibeTerm" width="820">

# VibeTerm

**Tia lửa khắp nơi, rõ trong một cái nhìn.**

Trình quản lý terminal hiện đại cho vibe coding. Cục bộ trước, CJK gốc. Cứ để agent chạy — cái nào đang rực, cái nào đã lụi, cái nào đang gọi bạn — tất cả thu vào một cái nhìn. Không xâm nhập, không đăng nhập, không đám mây.

[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)
[![Platform](https://img.shields.io/badge/platform-macOS%20%7C%20Windows-lightgrey.svg)](https://github.com/fjlmcm/VibeTerm/releases)
[![Release](https://img.shields.io/github/v/release/fjlmcm/VibeTerm?color=success)](https://github.com/fjlmcm/VibeTerm/releases)
[![Stack](https://img.shields.io/badge/Tauri%202-Rust%20%C2%B7%20SolidJS-ffc131.svg)](https://github.com/fjlmcm/VibeTerm)

[**www.vibeterm.org**](https://www.vibeterm.org) · [**Tải về**](https://github.com/fjlmcm/VibeTerm/releases) · [**GitHub**](https://github.com/fjlmcm/VibeTerm)

[English](README.md) · [简体中文](README.zh.md) · [繁體中文](README.zh-hant.md) · [日本語](README.ja.md) · [한국어](README.ko.md) · **Tiếng Việt** · [Bahasa Indonesia](README.id.md) · [Español](README.es.md) · [Português](README.pt-br.md) · [Deutsch](README.de.md) · [Français](README.fr.md) · [Italiano](README.it.md) · [Русский](README.ru.md) · [Türkçe](README.tr.md)

</div>

---

## Nó không chạy agent thay bạn. Chỉ trông chừng chúng thôi.

- **Không bao giờ đụng vào cấu hình của bạn** — Nó hiểu trạng thái bằng cách quan sát: đọc output, theo dõi tệp ở chế độ chỉ đọc. Không bao giờ ghi vào ~/.claude hay ~/.codex, không cài hook, không chạy dịch vụ nền. Không một byte cấu hình agent nào bị đụng tới.
- **Quản được cả đống agent** — Chỉ cần vài agent là đã rối. Nó đẩy lên trên những cái bị kẹt và những cái đang đợi bạn, để bạn khỏi phải mở từng cái xem ai cần mình.
- **Terminal vẫn là terminal** — Làm tốt những thứ cơ bản của một terminal. Không nhồi tính năng, không tham vọng thành bàn làm việc cho agent.
- **CJK chạy ngon lành** — Ký tự rộng, nhập IME, sao chép có emoji bên trong — những thứ terminal phương Tây hay làm hỏng, ở đây xử lý đàng hoàng.
- **Mọi thứ ở lại trên máy bạn** — Không đăng nhập, không thu thập dữ liệu, mặc định ngoại tuyến. Chỉ lên mạng khi bạn tự kiểm tra cập nhật, mà cũng chỉ đọc.
- **MIT, mã nguồn mở** — Toàn bộ mã là công khai. Đọc, sửa, tùy bạn.

## Năm trạng thái, rõ trong một cái nhìn.

- 🔵 **Đang chạy** — Chấm xanh sáng đều. Agent đang làm việc.
- 🟡 **Đang chờ** — Chấm hổ phách thở nhẹ. Đang đợi bạn, đáng để liếc qua.
- 🔴 **Kẹt** — Vòng đỏ-cam. Lặng hơn 5 phút, chắc là kẹt rồi.
- ⚪ **Rảnh** — Chấm xám đứng yên. Không có gì xảy ra.
- 🟢 **Xong** — Vòng viền, gạch ngang. Cái này xong thật rồi.

## Mọi thứ một terminal nên làm, cộng thêm phần agent.

_Đủ các tính năng terminal thường thấy, cộng thêm nhận biết trạng thái và điều phối cho một màn hình đầy AI agent._

### Agent

- **Thấy agent đang làm gì** — Đang chạy, đang chờ, kẹt hay xong — nhận ra mà không đụng cấu hình của bạn.
- **Phát hiện kẹt + sắp xếp theo độ gấp** — Màn hình đầy agent? Cái kẹt và cái đang đợi bạn được đẩy lên đầu.
- **Mức dùng thời gian thực** — Ngữ cảnh còn lại, hạn mức 5h/7d, tốc độ tiêu, cache, chi phí — tất cả trên một thanh.
- **Thống kê mức dùng** — Số token và chi phí cho Claude / Codex. Tính ngoại tuyến, xuất được.

### Terminal

- **Chia ô + worktree** — Gắn một git worktree, mỗi tác vụ một cây terminal riêng.
- **Bảng Canvas** — Xếp tác vụ thành thẻ, chọn bằng khung, gửi một lệnh tới nhiều terminal.
- **Cửa sổ nổi** — Tách bất kỳ tác vụ nào ra cửa sổ riêng và tiếp tục trông chừng.
- **Dựng hình bằng GPU** — Tăng tốc WebGL, mà CJK vẫn không rớt glyph hay giật.

### Hiệu suất

- **Bảng lệnh** — Phím tắt và hành động tùy chỉnh. Làm hết bằng bàn phím.
- **Mẫu prompt** — Mẫu sẵn tiện lợi cho claude / codex / shell, chỉ một phím.
- **Thanh trạng thái tùy chỉnh** — Kéo widget để sắp xếp; mỗi loại agent có cấu hình riêng.
- **Thông báo trên màn hình** — 24 âm thanh tích hợp + giờ yên tĩnh, chỉ khi trạng thái agent đổi.
- **Đổi giao diện tức thì** — 10 giao diện tích hợp, đổi bất cứ lúc nào, macOS và Windows.

## Làm sao nó biết agent đang làm gì mà không đụng cấu hình của bạn?

Ba cách quan sát, cộng theo dõi tệp chỉ đọc. Không hook, không đăng nhập, không ghi gì cả.

1. **Chuỗi OSC 133 / 633** — Dấu ranh giới lệnh từ tích hợp shell. Lớp đáng tin nhất: biết chính xác khi nào một lệnh bắt đầu, kết thúc hay đang chờ nhập.
2. **Đọc output của agent** — Đối chiếu lời nhắc cấp quyền của 11 agent phổ biến để biết khi nào một cái đang đợi bạn.
3. **Spinner trên thanh tiêu đề** — Nếu biểu tượng quay kiểu braille trên thanh tiêu đề cửa sổ đang chạy, tức là agent đang làm việc.

> **Lằn ranh: không đụng vào đồ của bạn** — Không bao giờ ghi vào ~/.claude hay ~/.codex, không cài hook, không chạy dịch vụ nền. Mọi trạng thái đều được quan sát, không bao giờ chèn vào.

## Không một terminal AI lớn nào bằng tiếng Anh coi trọng CJK.

Gần như mọi repo terminal AI lớn đều có lỗi CJK còn mở, bị chôn dưới những vấn đề gấp hơn của người dùng tiếng Anh. Chẳng ai thực sự làm tới nơi tới chốn phần này. VibeTerm coi đó là việc thật sự.

- Chặn tổ hợp IME suốt từ đầu đến cuối (isComposing / keyCode 229). Không gửi nhầm, không lag.
- Đo đúng độ rộng ký tự full-width và loại mơ hồ (ambiguous), nên bảng không lệch.
- Ngắt dòng CJK không bị cắt cụt; khi truyền theo luồng (streaming) cũng không xé đôi một glyph.
- Sao chép được Intl.Segmenter bảo vệ, không làm vỡ cặp surrogate hay cụm emoji ghép (ZWJ).
- CJK không rớt hay lệch khi dựng hình bằng GPU.

## Thử nhé?

macOS 11+ và Windows, chung một trang tải.

**[Tải về →](https://github.com/fjlmcm/VibeTerm/releases)** — macOS `.dmg` · Windows `.exe` / `.msi`.

Hoặc tự build từ mã nguồn:

```bash
pnpm install
pnpm build      # = tauri build → src-tauri/target/release/bundle/
pnpm dev        # dev (Vite HMR + tauri dev)
```

Built with **Tauri 2 · Rust · SolidJS · xterm.js** (pnpm monorepo).

## Đứng trên vai những dự án này.

Cảm ơn đặc biệt ccusage của ryoppippi (MIT). Thống kê mức dùng, giá mô hình và khối 5 giờ đều từ đó mà ra; dữ liệu giá đến từ LiteLLM và các con số chính thức của Anthropic.

Also building on [Tauri](https://tauri.app) · [SolidJS](https://solidjs.com) · [xterm.js](https://xtermjs.org) · [WezTerm](https://github.com/wezterm/wezterm) · [Tabby](https://github.com/Eugeny/tabby). Full list in [THIRD-PARTY-NOTICES.md](THIRD-PARTY-NOTICES.md).

## MIT License

[MIT](LICENSE) · © 2026 VibeTerm contributors
