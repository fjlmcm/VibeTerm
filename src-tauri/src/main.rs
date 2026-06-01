//! VibeTerm — Tauri app 入口
//!
//! IPC handler 全部在本文件 + 各 use 上引用 core/config/tasks。

#![cfg_attr(
    all(not(debug_assertions), target_os = "windows"),
    windows_subsystem = "windows"
)]

use std::sync::Arc;
use std::time::Duration;

use tauri::ipc::Channel;
#[cfg(target_os = "macos")]
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem, Submenu};
#[cfg(target_os = "macos")]
use tauri::RunEvent;
use tauri::{AppHandle, Emitter, Manager, WebviewUrl, WebviewWindowBuilder};
use tauri_plugin_clipboard_manager::ClipboardExt;
use tracing_subscriber::EnvFilter;

use vibeterm_config::actions::{ActionMode, ActionsFile};
use vibeterm_config::{Config, EnvFile, KeybindingsFile, NotifyFile, PromptsFile, Theme};
use vibeterm_core::{TaskRegistry, TerminalRegistry};
use vibeterm_ipc::{
    CreateTaskOpts, IpcError, IpcResult, SpawnPtyOpts, SpawnPtyResult, TaskDto, TaskLocation,
    TerminalId,
};
use vibeterm_pty::{ChunkSink, ExitInfo, SpawnOpts};
use vibeterm_status::StatusDetector;

mod clipboard_files;

/// macOS:把 NSVisualEffectView underWindowBackground material 装到窗口下层。
/// WebView 设了 transparent → resize 时新扩展区域露出毛玻璃模糊层,
/// 看着像故意的视觉设计,而不是 lag 的死黑/死白。
/// 借鉴 Tabby `references/tabby/app/lib/window.ts:118` setVibrancy(macOSVibrancyType)。
#[cfg(target_os = "macos")]
fn apply_macos_vibrancy(window: &tauri::WebviewWindow) {
    use window_vibrancy::{apply_vibrancy, NSVisualEffectMaterial, NSVisualEffectState};
    if let Err(e) = apply_vibrancy(
        window,
        NSVisualEffectMaterial::UnderWindowBackground,
        Some(NSVisualEffectState::Active),
        None,
    ) {
        tracing::warn!(err = %e, "apply_vibrancy failed");
    }
}

// ---- App state ----
struct AppState {
    terminals: Arc<TerminalRegistry>,
    tasks: Arc<TaskRegistry>,
    // 顶栏菜单语言 — 前端 setLang() 时通过 set_menu_lang IPC 同步
    menu_lang: std::sync::Mutex<MenuLang>,
    /// 每个 terminal 的 StatusDetector handle. agent 嗅探层用它在识别到
    /// agent_kind 后开启 stall 检测; close_terminal 时清理.
    status_detectors: std::sync::Mutex<
        std::collections::HashMap<TerminalId, Arc<std::sync::Mutex<StatusDetector>>>,
    >,
    /// 通知点击聚焦:tauri-plugin-notification 2.x 桌面无 click callback,
    /// 用"最近通知 + 时间戳"近似 — notify 后写入此字段,window focused 事件触发时
    /// 若在 NOTIFY_FOCUS_GRACE 窗口内 → 视为点击,emit 给前端切 task; 否则忽略.
    last_notify: std::sync::Mutex<Option<(vibeterm_ipc::TaskId, std::time::Instant)>>,
    /// agent 完成通知 throttle (per-task, 30s). 防来回对话每个 turn 都响.
    /// 现由嗅探(标题 spinner→静态 / OSC D)触发完成通知, key 用 "task-<id>".
    last_agent_completed: std::sync::Mutex<std::collections::HashMap<String, std::time::Instant>>,
    /// 间歇持续提醒(persistent_unseen_sound)节流时刻。None = 当前无"未看完成"或主窗口在
    /// 前台(已 reset);Some(t) = 上次响铃时刻,下次需隔 PERSISTENT_REMIND_INTERVAL。全局单路。
    last_persistent_remind: std::sync::Mutex<Option<std::time::Instant>>,
}

/// 窗口聚焦事件 → 视为"点击通知"的最大允许 gap. 超过则当作用户从 dock/Cmd-Tab
/// 主动激活,不强制切 task. 取 10s 是经验:用户看到通知到点击通常 < 5s.
const NOTIFY_FOCUS_GRACE: std::time::Duration = std::time::Duration::from_secs(10);

/// agent_completed 通知冷却时间(per-task). transcript 完成检测已是轮级精确(claude end_turn /
/// codex task_complete,一轮一次 + set_agent_turn_done 去重),只需挡同一 task 几秒内的极快重复;
/// 5s 让正常多轮对话每轮都提示,又不至于同轮边界的瞬时重复连响两声.
const AGENT_COMPLETED_COOLDOWN: std::time::Duration = std::time::Duration::from_secs(5);

/// 间歇持续提醒(persistent_unseen_sound)的最小响铃间隔。全局单路(不 per-task),
/// 60s 一次:不致烦扰,又不漏。仅在"有未看完成 + 主窗口失焦"时计时。
const PERSISTENT_REMIND_INTERVAL: std::time::Duration = std::time::Duration::from_secs(60);

#[derive(Clone, Copy, Debug)]
enum MenuLang {
    ZhCN,
    ZhHant,
    En,
    Ja,
    Ko,
    Vi,
    Id,
    Es,
    PtBr,
    De,
    Fr,
    It,
    Ru,
    Tr,
}

impl MenuLang {
    fn from_tag(s: &str) -> Self {
        let l = s.to_lowercase();
        // 繁体优先(zh-tw/zh-hk/zh-mo/zh-hant),其余 zh → 简体
        if l.starts_with("zh-hant")
            || l.starts_with("zh-tw")
            || l.starts_with("zh-hk")
            || l.starts_with("zh-mo")
        {
            MenuLang::ZhHant
        } else if l.starts_with("zh") {
            MenuLang::ZhCN
        } else if l.starts_with("ja") {
            MenuLang::Ja
        } else if l.starts_with("ko") {
            MenuLang::Ko
        } else if l.starts_with("vi") {
            MenuLang::Vi
        } else if l.starts_with("id") || l.starts_with("in") {
            MenuLang::Id
        } else if l.starts_with("es") {
            MenuLang::Es
        } else if l.starts_with("pt") {
            MenuLang::PtBr
        } else if l.starts_with("de") {
            MenuLang::De
        } else if l.starts_with("fr") {
            MenuLang::Fr
        } else if l.starts_with("it") {
            MenuLang::It
        } else if l.starts_with("ru") {
            MenuLang::Ru
        } else if l.starts_with("tr") {
            MenuLang::Tr
        } else {
            MenuLang::En
        }
    }

    fn from_env() -> Self {
        Self::from_tag(&std::env::var("LANG").unwrap_or_default())
    }
}

struct MenuLabels {
    about_app: &'static str, // About item label
    check_update: &'static str,
    settings: &'static str,
    // 系统 Predefined 项 label override
    services: &'static str,
    hide: &'static str,
    hide_others: &'static str,
    show_all: &'static str,
    quit: &'static str,
    undo: &'static str,
    redo: &'static str,
    cut: &'static str,
    copy: &'static str,
    paste: &'static str,
    select_all: &'static str,
    minimize: &'static str,
    maximize: &'static str,
    // ----
    file: &'static str,
    new_task: &'static str,
    new_terminal: &'static str,
    open_claude_md: &'static str,
    open_config_dir: &'static str,
    close_terminal: &'static str,
    edit: &'static str,
    find_in_terminal: &'static str,
    view: &'static str,
    command_palette: &'static str,
    next_task: &'static str,
    prev_task: &'static str,
    split_horizontal: &'static str,
    split_vertical: &'static str,
    switch_theme: &'static str,
    window: &'static str,
    focus_main: &'static str,
    floating_prefix: &'static str, // 用作 `{prefix} — {label}`
    help: &'static str,
    open_shortcuts: &'static str,
    open_github: &'static str,
    open_issues: &'static str,
    open_privacy: &'static str,
}

const LBL_ZH: MenuLabels = MenuLabels {
    about_app: "关于 VibeTerm",
    check_update: "检查更新…",
    settings: "偏好设置…",
    services: "服务",
    hide: "隐藏 VibeTerm",
    hide_others: "隐藏其他",
    show_all: "全部显示",
    quit: "退出 VibeTerm",
    undo: "撤销",
    redo: "重做",
    cut: "剪切",
    copy: "复制",
    paste: "粘贴",
    select_all: "全选",
    minimize: "最小化",
    maximize: "缩放",
    file: "文件",
    new_task: "新建任务",
    new_terminal: "新建终端",
    open_claude_md: "打开 CLAUDE.md…",
    open_config_dir: "打开配置目录",
    close_terminal: "关闭终端",
    edit: "编辑",
    find_in_terminal: "查找",
    view: "视图",
    command_palette: "命令面板",
    next_task: "切到下一任务",
    prev_task: "切到上一任务",
    split_horizontal: "水平分屏",
    split_vertical: "垂直分屏",
    switch_theme: "切换主题…",
    window: "窗口",
    focus_main: "VibeTerm(主窗口)",
    floating_prefix: "浮窗",
    help: "帮助",
    open_shortcuts: "全部快捷键…",
    open_github: "GitHub 仓库",
    open_issues: "报告问题",
    open_privacy: "隐私政策",
};

const LBL_EN: MenuLabels = MenuLabels {
    about_app: "About VibeTerm",
    check_update: "Check for Updates…",
    settings: "Preferences…",
    services: "Services",
    hide: "Hide VibeTerm",
    hide_others: "Hide Others",
    show_all: "Show All",
    quit: "Quit VibeTerm",
    undo: "Undo",
    redo: "Redo",
    cut: "Cut",
    copy: "Copy",
    paste: "Paste",
    select_all: "Select All",
    minimize: "Minimize",
    maximize: "Zoom",
    file: "File",
    new_task: "New Task",
    new_terminal: "New Terminal",
    open_claude_md: "Open CLAUDE.md…",
    open_config_dir: "Open Config Folder",
    close_terminal: "Close Terminal",
    edit: "Edit",
    find_in_terminal: "Find",
    view: "View",
    command_palette: "Command Palette",
    next_task: "Next Task",
    prev_task: "Previous Task",
    split_horizontal: "Split Horizontally",
    split_vertical: "Split Vertically",
    switch_theme: "Switch Theme…",
    window: "Window",
    focus_main: "VibeTerm (Main)",
    floating_prefix: "Floating",
    help: "Help",
    open_shortcuts: "All Shortcuts…",
    open_github: "GitHub Repository",
    open_issues: "Report Issue",
    open_privacy: "Privacy Policy",
};

const LBL_JA: MenuLabels = MenuLabels {
    about_app: "VibeTerm について",
    check_update: "アップデートを確認…",
    settings: "環境設定…",
    services: "サービス",
    hide: "VibeTerm を隠す",
    hide_others: "ほかを隠す",
    show_all: "すべてを表示",
    quit: "VibeTerm を終了",
    undo: "取り消す",
    redo: "やり直す",
    cut: "カット",
    copy: "コピー",
    paste: "ペースト",
    select_all: "すべてを選択",
    minimize: "しまう",
    maximize: "拡大/縮小",
    file: "ファイル",
    new_task: "新規タスク",
    new_terminal: "新規ターミナル",
    open_claude_md: "CLAUDE.md を開く…",
    open_config_dir: "設定フォルダを開く",
    close_terminal: "ターミナルを閉じる",
    edit: "編集",
    find_in_terminal: "検索",
    view: "表示",
    command_palette: "コマンドパレット",
    next_task: "次のタスクへ",
    prev_task: "前のタスクへ",
    split_horizontal: "水平分割",
    split_vertical: "垂直分割",
    switch_theme: "テーマ切替…",
    window: "ウインドウ",
    focus_main: "VibeTerm(メイン)",
    floating_prefix: "フロート",
    help: "ヘルプ",
    open_shortcuts: "すべてのショートカット…",
    open_github: "GitHub リポジトリ",
    open_issues: "問題を報告",
    open_privacy: "プライバシーポリシー",
};

const LBL_ZH_HANT: MenuLabels = MenuLabels {
    about_app: "關於 VibeTerm",
    check_update: "檢查更新……",
    settings: "偏好設定……",
    services: "服務",
    hide: "隱藏 VibeTerm",
    hide_others: "隱藏其他",
    show_all: "全部顯示",
    quit: "結束 VibeTerm",
    undo: "還原",
    redo: "重做",
    cut: "剪下",
    copy: "拷貝",
    paste: "貼上",
    select_all: "全選",
    minimize: "縮到最小",
    maximize: "縮放",
    file: "檔案",
    new_task: "新增任務",
    new_terminal: "新增終端機",
    open_claude_md: "開啟 CLAUDE.md……",
    open_config_dir: "開啟設定檔資料夾",
    close_terminal: "關閉終端機",
    edit: "編輯",
    find_in_terminal: "尋找",
    view: "顯示方式",
    command_palette: "命令面板",
    next_task: "下一個任務",
    prev_task: "上一個任務",
    split_horizontal: "水平分割",
    split_vertical: "垂直分割",
    switch_theme: "切換主題……",
    window: "視窗",
    focus_main: "VibeTerm（主視窗）",
    floating_prefix: "浮動視窗",
    help: "輔助說明",
    open_shortcuts: "所有快速鍵……",
    open_github: "GitHub 儲存庫",
    open_issues: "回報問題",
    open_privacy: "隱私權政策",
};

const LBL_KO: MenuLabels = MenuLabels {
    about_app: "VibeTerm에 관하여",
    check_update: "업데이트 확인…",
    settings: "환경설정…",
    services: "서비스",
    hide: "VibeTerm 가리기",
    hide_others: "기타 가리기",
    show_all: "모두 보기",
    quit: "VibeTerm 종료",
    undo: "실행 취소",
    redo: "다시 실행",
    cut: "오려두기",
    copy: "복사하기",
    paste: "붙여넣기",
    select_all: "전체 선택",
    minimize: "최소화",
    maximize: "확대/축소",
    file: "파일",
    new_task: "새로운 작업",
    new_terminal: "새로운 터미널",
    open_claude_md: "CLAUDE.md 열기…",
    open_config_dir: "설정 폴더 열기",
    close_terminal: "터미널 닫기",
    edit: "편집",
    find_in_terminal: "찾기",
    view: "보기",
    command_palette: "명령 팔레트",
    next_task: "다음 작업",
    prev_task: "이전 작업",
    split_horizontal: "가로로 분할",
    split_vertical: "세로로 분할",
    switch_theme: "테마 전환…",
    window: "윈도우",
    focus_main: "VibeTerm (메인)",
    floating_prefix: "플로팅",
    help: "도움말",
    open_shortcuts: "모든 단축키…",
    open_github: "GitHub 저장소",
    open_issues: "문제 신고",
    open_privacy: "개인정보 처리방침",
};

const LBL_VI: MenuLabels = MenuLabels {
    about_app: "Giới thiệu VibeTerm",
    check_update: "Kiểm tra bản cập nhật…",
    settings: "Tùy chọn…",
    services: "Dịch vụ",
    hide: "Ẩn VibeTerm",
    hide_others: "Ẩn mục khác",
    show_all: "Hiện tất cả",
    quit: "Thoát VibeTerm",
    undo: "Hoàn tác",
    redo: "Làm lại",
    cut: "Cắt",
    copy: "Sao chép",
    paste: "Dán",
    select_all: "Chọn tất cả",
    minimize: "Thu nhỏ",
    maximize: "Thu phóng",
    file: "Tệp",
    new_task: "Tạo task mới",
    new_terminal: "Terminal mới",
    open_claude_md: "Mở CLAUDE.md…",
    open_config_dir: "Mở thư mục cấu hình",
    close_terminal: "Đóng terminal",
    edit: "Sửa",
    find_in_terminal: "Tìm",
    view: "Hiển thị",
    command_palette: "Bảng lệnh",
    next_task: "Task kế tiếp",
    prev_task: "Task trước",
    split_horizontal: "Chia ngang",
    split_vertical: "Chia dọc",
    switch_theme: "Đổi theme…",
    window: "Cửa sổ",
    focus_main: "VibeTerm (Chính)",
    floating_prefix: "Cửa sổ nổi",
    help: "Trợ giúp",
    open_shortcuts: "Tất cả phím tắt…",
    open_github: "Kho GitHub",
    open_issues: "Báo lỗi",
    open_privacy: "Chính sách quyền riêng tư",
};

const LBL_ID: MenuLabels = MenuLabels {
    about_app: "Tentang VibeTerm",
    check_update: "Periksa Pembaruan…",
    settings: "Preferensi…",
    services: "Layanan",
    hide: "Sembunyikan VibeTerm",
    hide_others: "Sembunyikan Lainnya",
    show_all: "Tampilkan Semua",
    quit: "Keluar dari VibeTerm",
    undo: "Urungkan",
    redo: "Ulangi",
    cut: "Potong",
    copy: "Salin",
    paste: "Tempel",
    select_all: "Pilih Semua",
    minimize: "Perkecil",
    maximize: "Zum",
    file: "Berkas",
    new_task: "Tugas Baru",
    new_terminal: "Terminal Baru",
    open_claude_md: "Buka CLAUDE.md…",
    open_config_dir: "Buka Folder Konfigurasi",
    close_terminal: "Tutup Terminal",
    edit: "Edit",
    find_in_terminal: "Cari",
    view: "Tampilan",
    command_palette: "Palet Perintah",
    next_task: "Tugas Berikutnya",
    prev_task: "Tugas Sebelumnya",
    split_horizontal: "Bagi Mendatar",
    split_vertical: "Bagi Menegak",
    switch_theme: "Ganti Tema…",
    window: "Jendela",
    focus_main: "VibeTerm (Utama)",
    floating_prefix: "Mengambang",
    help: "Bantuan",
    open_shortcuts: "Semua Pintasan…",
    open_github: "Repositori GitHub",
    open_issues: "Laporkan Masalah",
    open_privacy: "Kebijakan Privasi",
};

const LBL_ES: MenuLabels = MenuLabels {
    about_app: "Acerca de VibeTerm",
    check_update: "Buscar actualizaciones…",
    settings: "Ajustes…",
    services: "Servicios",
    hide: "Ocultar VibeTerm",
    hide_others: "Ocultar los demás",
    show_all: "Mostrar todo",
    quit: "Salir de VibeTerm",
    undo: "Deshacer",
    redo: "Rehacer",
    cut: "Cortar",
    copy: "Copiar",
    paste: "Pegar",
    select_all: "Seleccionar todo",
    minimize: "Minimizar",
    maximize: "Zoom",
    file: "Archivo",
    new_task: "Nueva tarea",
    new_terminal: "Nueva terminal",
    open_claude_md: "Abrir CLAUDE.md…",
    open_config_dir: "Abrir la carpeta de configuración",
    close_terminal: "Cerrar terminal",
    edit: "Edición",
    find_in_terminal: "Buscar",
    view: "Visualización",
    command_palette: "Paleta de comandos",
    next_task: "Tarea siguiente",
    prev_task: "Tarea anterior",
    split_horizontal: "Dividir horizontalmente",
    split_vertical: "Dividir verticalmente",
    switch_theme: "Cambiar tema…",
    window: "Ventana",
    focus_main: "VibeTerm (principal)",
    floating_prefix: "Flotante",
    help: "Ayuda",
    open_shortcuts: "Todos los atajos…",
    open_github: "Repositorio de GitHub",
    open_issues: "Informar de un problema",
    open_privacy: "Política de privacidad",
};

const LBL_PT_BR: MenuLabels = MenuLabels {
    about_app: "Sobre o VibeTerm",
    check_update: "Buscar Atualizações…",
    settings: "Preferências…",
    services: "Serviços",
    hide: "Ocultar VibeTerm",
    hide_others: "Ocultar Outros",
    show_all: "Mostrar Tudo",
    quit: "Encerrar VibeTerm",
    undo: "Desfazer",
    redo: "Refazer",
    cut: "Cortar",
    copy: "Copiar",
    paste: "Colar",
    select_all: "Selecionar Tudo",
    minimize: "Minimizar",
    maximize: "Zoom",
    file: "Arquivo",
    new_task: "Nova Tarefa",
    new_terminal: "Novo Terminal",
    open_claude_md: "Abrir CLAUDE.md…",
    open_config_dir: "Abrir Pasta de Configuração",
    close_terminal: "Fechar Terminal",
    edit: "Editar",
    find_in_terminal: "Buscar",
    view: "Visualizar",
    command_palette: "Paleta de Comandos",
    next_task: "Próxima Tarefa",
    prev_task: "Tarefa Anterior",
    split_horizontal: "Dividir Horizontalmente",
    split_vertical: "Dividir Verticalmente",
    switch_theme: "Trocar Tema…",
    window: "Janela",
    focus_main: "VibeTerm (Principal)",
    floating_prefix: "Flutuante",
    help: "Ajuda",
    open_shortcuts: "Todos os Atalhos…",
    open_github: "Repositório no GitHub",
    open_issues: "Relatar Problema",
    open_privacy: "Política de Privacidade",
};

const LBL_DE: MenuLabels = MenuLabels {
    about_app: "Über VibeTerm",
    check_update: "Nach Updates suchen …",
    settings: "Einstellungen …",
    services: "Dienste",
    hide: "VibeTerm ausblenden",
    hide_others: "Andere ausblenden",
    show_all: "Alle einblenden",
    quit: "VibeTerm beenden",
    undo: "Widerrufen",
    redo: "Wiederholen",
    cut: "Ausschneiden",
    copy: "Kopieren",
    paste: "Einsetzen",
    select_all: "Alles auswählen",
    minimize: "Im Dock ablegen",
    maximize: "Zoomen",
    file: "Ablage",
    new_task: "Neue Aufgabe",
    new_terminal: "Neues Terminal",
    open_claude_md: "CLAUDE.md öffnen …",
    open_config_dir: "Konfigurationsordner öffnen",
    close_terminal: "Terminal schließen",
    edit: "Bearbeiten",
    find_in_terminal: "Suchen",
    view: "Darstellung",
    command_palette: "Befehlspalette",
    next_task: "Nächste Aufgabe",
    prev_task: "Vorherige Aufgabe",
    split_horizontal: "Horizontal teilen",
    split_vertical: "Vertikal teilen",
    switch_theme: "Theme wechseln …",
    window: "Fenster",
    focus_main: "VibeTerm (Hauptfenster)",
    floating_prefix: "Schwebend",
    help: "Hilfe",
    open_shortcuts: "Alle Tastenkürzel …",
    open_github: "GitHub-Repository",
    open_issues: "Problem melden",
    open_privacy: "Datenschutzerklärung",
};

const LBL_FR: MenuLabels = MenuLabels {
    about_app: "À propos de VibeTerm",
    check_update: "Rechercher les mises à jour…",
    settings: "Réglages…",
    services: "Services",
    hide: "Masquer VibeTerm",
    hide_others: "Masquer les autres",
    show_all: "Tout afficher",
    quit: "Quitter VibeTerm",
    undo: "Annuler",
    redo: "Rétablir",
    cut: "Couper",
    copy: "Copier",
    paste: "Coller",
    select_all: "Tout sélectionner",
    minimize: "Réduire",
    maximize: "Réduire/Agrandir",
    file: "Fichier",
    new_task: "Nouvelle tâche",
    new_terminal: "Nouveau terminal",
    open_claude_md: "Ouvrir CLAUDE.md…",
    open_config_dir: "Ouvrir le dossier de configuration",
    close_terminal: "Fermer le terminal",
    edit: "Édition",
    find_in_terminal: "Rechercher",
    view: "Présentation",
    command_palette: "Palette de commandes",
    next_task: "Tâche suivante",
    prev_task: "Tâche précédente",
    split_horizontal: "Diviser horizontalement",
    split_vertical: "Diviser verticalement",
    switch_theme: "Changer de thème…",
    window: "Fenêtre",
    focus_main: "VibeTerm (principale)",
    floating_prefix: "Flottante",
    help: "Aide",
    open_shortcuts: "Tous les raccourcis…",
    open_github: "Dépôt GitHub",
    open_issues: "Signaler un problème",
    open_privacy: "Politique de confidentialité",
};

const LBL_IT: MenuLabels = MenuLabels {
    about_app: "Informazioni su VibeTerm",
    check_update: "Verifica aggiornamenti…",
    settings: "Impostazioni…",
    services: "Servizi",
    hide: "Nascondi VibeTerm",
    hide_others: "Nascondi gli altri",
    show_all: "Mostra tutto",
    quit: "Esci da VibeTerm",
    undo: "Annulla",
    redo: "Ripristina",
    cut: "Taglia",
    copy: "Copia",
    paste: "Incolla",
    select_all: "Seleziona tutto",
    minimize: "Riduci a icona",
    maximize: "Zoom",
    file: "Archivio",
    new_task: "Nuovo task",
    new_terminal: "Nuovo terminale",
    open_claude_md: "Apri CLAUDE.md…",
    open_config_dir: "Apri la cartella di configurazione",
    close_terminal: "Chiudi terminale",
    edit: "Modifica",
    find_in_terminal: "Cerca",
    view: "Vista",
    command_palette: "Palette dei comandi",
    next_task: "Task successivo",
    prev_task: "Task precedente",
    split_horizontal: "Dividi orizzontalmente",
    split_vertical: "Dividi verticalmente",
    switch_theme: "Cambia tema…",
    window: "Finestra",
    focus_main: "VibeTerm (Principale)",
    floating_prefix: "Mobile",
    help: "Aiuto",
    open_shortcuts: "Tutte le scorciatoie…",
    open_github: "Repository GitHub",
    open_issues: "Segnala un problema",
    open_privacy: "Informativa sulla privacy",
};

const LBL_RU: MenuLabels = MenuLabels {
    about_app: "О приложении VibeTerm",
    check_update: "Проверить обновления…",
    settings: "Настройки…",
    services: "Службы",
    hide: "Скрыть VibeTerm",
    hide_others: "Скрыть остальные",
    show_all: "Показать все",
    quit: "Завершить VibeTerm",
    undo: "Отменить",
    redo: "Повторить",
    cut: "Вырезать",
    copy: "Скопировать",
    paste: "Вставить",
    select_all: "Выбрать все",
    minimize: "Убрать в Dock",
    maximize: "Изменить масштаб",
    file: "Файл",
    new_task: "Новая задача",
    new_terminal: "Новый терминал",
    open_claude_md: "Открыть CLAUDE.md…",
    open_config_dir: "Открыть папку конфигурации",
    close_terminal: "Закрыть терминал",
    edit: "Правка",
    find_in_terminal: "Найти",
    view: "Вид",
    command_palette: "Палитра команд",
    next_task: "Следующая задача",
    prev_task: "Предыдущая задача",
    split_horizontal: "Разделить по горизонтали",
    split_vertical: "Разделить по вертикали",
    switch_theme: "Сменить тему…",
    window: "Окно",
    focus_main: "VibeTerm (главное)",
    floating_prefix: "Плавающее",
    help: "Справка",
    open_shortcuts: "Все сочетания клавиш…",
    open_github: "Репозиторий на GitHub",
    open_issues: "Сообщить о проблеме",
    open_privacy: "Политика конфиденциальности",
};

const LBL_TR: MenuLabels = MenuLabels {
    about_app: "VibeTerm Hakkında",
    check_update: "Güncellemeleri Denetle…",
    settings: "Tercihler…",
    services: "Hizmetler",
    hide: "VibeTerm'i Gizle",
    hide_others: "Diğerlerini Gizle",
    show_all: "Tümünü Göster",
    quit: "VibeTerm'den Çık",
    undo: "Geri Al",
    redo: "Yinele",
    cut: "Kes",
    copy: "Kopyala",
    paste: "Yapıştır",
    select_all: "Tümünü Seç",
    minimize: "Simge Durumuna Küçült",
    maximize: "Yakınlaştır",
    file: "Dosya",
    new_task: "Yeni Görev",
    new_terminal: "Yeni Terminal",
    open_claude_md: "CLAUDE.md'yi Aç…",
    open_config_dir: "Yapılandırma Klasörünü Aç",
    close_terminal: "Terminali Kapat",
    edit: "Düzen",
    find_in_terminal: "Bul",
    view: "Görünüm",
    command_palette: "Komut Paleti",
    next_task: "Sonraki Görev",
    prev_task: "Önceki Görev",
    split_horizontal: "Yatay Böl",
    split_vertical: "Dikey Böl",
    switch_theme: "Temayı Değiştir…",
    window: "Pencere",
    focus_main: "VibeTerm (Ana)",
    floating_prefix: "Yüzen",
    help: "Yardım",
    open_shortcuts: "Tüm Kısayollar…",
    open_github: "GitHub Deposu",
    open_issues: "Sorun Bildir",
    open_privacy: "Gizlilik Politikası",
};

fn menu_labels(l: MenuLang) -> &'static MenuLabels {
    match l {
        MenuLang::ZhCN => &LBL_ZH,
        MenuLang::ZhHant => &LBL_ZH_HANT,
        MenuLang::En => &LBL_EN,
        MenuLang::Ja => &LBL_JA,
        MenuLang::Ko => &LBL_KO,
        MenuLang::Vi => &LBL_VI,
        MenuLang::Id => &LBL_ID,
        MenuLang::Es => &LBL_ES,
        MenuLang::PtBr => &LBL_PT_BR,
        MenuLang::De => &LBL_DE,
        MenuLang::Fr => &LBL_FR,
        MenuLang::It => &LBL_IT,
        MenuLang::Ru => &LBL_RU,
        MenuLang::Tr => &LBL_TR,
    }
}

// ---- helpers ----
pub(crate) fn emit_tasks_changed(app: &AppHandle, tasks: &TaskRegistry) {
    let Ok(mut list) = tasks.list() else { return };
    // 注入 last_output(终端末行,Prowl 风格状态行)。
    // 分屏时遍历所有 terminal,挑 last_update_ms 最大的那个 — 即"最近有输出"的那块屏。
    if let Some(state) = app.try_state::<AppState>() {
        for dto in &mut list {
            if !dto.terminal_ids.is_empty() {
                dto.last_output = state.terminals.most_recent_tail(&dto.terminal_ids);
            }
        }
    }
    let _ = app.emit("tasks_changed", &list);
}

/// 主窗口当前是否聚焦(用户正盯着 VibeTerm)。
fn main_window_focused(app: &AppHandle) -> bool {
    app.get_webview_window("main")
        .and_then(|w| w.is_focused().ok())
        .unwrap_or(false)
}

/// 通知投递方式 —— preflight 放行后告诉调用方走哪条路。
#[derive(Debug, Clone, Copy, PartialEq)]
enum NotifyRoute {
    /// 主窗口在后台:发系统通知 + 声音(完整通知)。
    Background,
    /// 主窗口在前台、但完成的不是当前选中任务:只前端轻提示音 + 任务列表行高亮,
    /// 不发系统横幅(macOS 前台横幅常被吞,且用户已在 app 里,无需强打扰)。
    ForegroundLight,
}

/// 通知预检 — 所有 task-level 守门集中一处, WaitingInput / agent_completed 共用.
/// 返回 Some((NotifyFile, route)) 表示放行, None 表示静默.
///
/// 守门顺序(命中任意一条即返回 None):
///   1. 非 agent task / per-task muted / 全局总开关 off / 免打扰时段 → 静默
///   2. 主窗口聚焦时:
///        - allow_foreground=false(如 waiting_input)→ 静默(维持前台不打扰)
///        - 完成的就是当前选中任务 → 静默(用户正看着,删除线/黄灯就在眼前)
///        - notify_focused_other_task 关 → 静默
///        - 否则 → ForegroundLight(前台轻提示音 + 列表高亮)
///   3. 主窗口失焦 → Background(完整系统通知 + 声音)
fn notify_preflight(
    app: &AppHandle,
    tasks: &TaskRegistry,
    task_id: vibeterm_ipc::TaskId,
    allow_foreground: bool,
) -> Option<(NotifyFile, NotifyRoute)> {
    let is_agent = tasks.agent_kind_of(task_id).ok().flatten().is_some();
    if !is_agent {
        return None;
    }
    if tasks.notify_muted_of(task_id).unwrap_or(false) {
        return None;
    }
    let prefs = NotifyFile::load();
    if !prefs.enabled {
        return None;
    }
    let now_hhmm = chrono::Local::now().format("%H:%M").to_string();
    if prefs.quiet_hours.contains(&now_hhmm) {
        return None;
    }
    if main_window_focused(app) {
        if !allow_foreground {
            return None;
        }
        if tasks.active_main() == Some(task_id) {
            return None;
        }
        if !prefs.notify_focused_other_task {
            return None;
        }
        return Some((prefs, NotifyRoute::ForegroundLight));
    }
    Some((prefs, NotifyRoute::Background))
}

/// 未看完成数 → Dock 角标(macOS dock 图标红色数字)。开关关或数为 0 时清除角标。
/// 复用 TaskRegistry::unseen_done_count(聚合状态 = Done 的任务数)。状态跃迁 /
/// 切换 active / 关闭任务后调用,保持角标与"未看完成"实时一致。
fn refresh_dock_badge(app: &AppHandle, tasks: &TaskRegistry) {
    let n = if NotifyFile::load().dock_badge_unseen {
        tasks.unseen_done_count()
    } else {
        0
    };
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.set_badge_count(if n > 0 { Some(n as i64) } else { None });
    }
}

/// agent 完成一轮(transcript 权威信号:claude `stop_reason=end_turn` / codex `task_complete`)
/// → 按 cwd 关联 task → 标 Done + Dock 角标 + 完成通知。比 PTY 输出超时可靠(agent 自己的结构
/// 化记录)。去重:per-task last_turn_id,同一轮多次 emit 只标一次。active 任务不标(用户正看着)。
/// agent transcript 轮状态更新(working/done)→ 关联 task → 设状态。
/// done 跃迁(working→done,刚答完一轮)时额外 Dock 角标 + 通知;working/done 都 emit 刷新圆点。
/// claude: turn_done = (stop_reason == "end_turn");codex: turn_done = task_completed(完成晚于开始)。
/// 这套用 transcript 驱动 agent 任务的完成显示,压过 PTY 输出嗅探(尤其 codex 状态栏刷新的假 Running)。
fn on_agent_turn_update(
    app: &AppHandle,
    agent: &str,
    match_cwd: &str,
    is_claude: bool,
    turn_done: bool,
    turn_id: Option<&str>,
) {
    let Some(state) = app.try_state::<AppState>() else {
        return;
    };
    let tasks: &TaskRegistry = &state.tasks;
    // 兜底实时窗口焦点 —— macOS 切到别的 app 时 WindowEvent::Focused 不一定可靠触发;每次 agent
    // 状态更新都按实时 is_focused() 校正,焦点变了立即刷新圆点 + 角标(失焦时当前 task 完成 → Done)。
    // 轮询每 3s 经此,故即使 Focused 事件完全不来,也最多 3s 校正到位。
    // 兜底实时焦点(Focused 事件 macOS 切 app 不一定可靠);set_window_focused 内已打日志,
    // 这里焦点变了就刷新圆点 + 角标。轮询每 3s 经此,即使事件不来也最多 3s 校正。
    if tasks
        .set_window_focused(main_window_focused(app))
        .unwrap_or(false)
    {
        emit_tasks_changed(app, tasks);
        refresh_dock_badge(app, tasks);
    }
    let pairs = match tasks.task_cwd_pairs() {
        Ok(p) => p,
        Err(_) => return,
    };
    // claude 用 cwd_to_project_dir 两边编码后比(规避 project_path 反解歧义);codex 精确比 cwd。
    let target = if is_claude {
        vibeterm_agent_watch::claude::project::cwd_to_project_dir(match_cwd)
    } else {
        match_cwd.to_string()
    };
    for (task_id, c) in pairs {
        let hit = if is_claude {
            vibeterm_agent_watch::claude::project::cwd_to_project_dir(&c) == target
        } else {
            c == match_cwd
        };
        if !hit {
            continue;
        }
        // set_agent_turn_done 返回 (changed, just_completed)。
        // changed=值真的变了才刷新 —— 轮询每 3s 兜底反复调,不变则不 emit,避免前端无谓刷新。
        // just_completed=working/未知 → done 跃迁,才发完成通知。Err(task 不存在)忽略。
        if let Ok((changed, just_completed)) = tasks.set_agent_turn_done(task_id, turn_done, turn_id)
        {
            // changed=turn_done 布尔变了(Running↔Done/Idle);just_completed=新一轮答完(快轮时
            // turn_done 没变但 seen 翻 false → 圆点也要刷成 Done)。任一成立都刷新圆点 + 角标。
            if changed || just_completed {
                tracing::info!(
                    agent,
                    task_id,
                    turn_done,
                    just_completed,
                    turn_id = ?turn_id,
                    "agent_turn_update: transcript 轮状态变化 → 刷新圆点"
                );
                emit_tasks_changed(app, tasks);
                refresh_dock_badge(app, tasks);
            }
            if just_completed {
                // 通知:交 fire 内 preflight 按窗口焦点决定 —— 后台都发(当前/非当前)、
                // 前台非当前任务轻提示、前台当前任务静默。
                let last = tasks
                    .task_dto(task_id)
                    .ok()
                    .flatten()
                    .and_then(|d| state.terminals.most_recent_tail(&d.terminal_ids))
                    .unwrap_or_default();
                fire_agent_completed_notification(
                    app,
                    tasks,
                    task_id,
                    agent,
                    &format!("task-{task_id}"),
                    last.trim(),
                );
            }
        }
    }
}

/// 3s 轮询兜底:按 task.cwd 主动读 transcript 完成状态 → on_agent_turn_update。
/// 文件监听(notify/FSEvents)对 codex 的 ~/.codex/sessions 写入不可靠(实测漏 task_complete),
/// 这里不依赖监听、直接读文件兜底。read_for_cwd 按 cwd 精确定位会话文件,比 watcher 全局
/// find_latest 更准;on_agent_turn_update 仅在状态变化时 emit,无谓轮询不刷屏。
fn poll_agent_turn_from_transcript(
    app: &AppHandle,
    tasks: &TaskRegistry,
    task_id: vibeterm_ipc::TaskId,
    kind: &str,
) {
    let Ok(Some(cwd)) = tasks.cwd(task_id) else {
        return;
    };
    if cwd.is_empty() {
        return;
    }
    let (is_claude, turn_done, turn_id) = match kind {
        "claude" => {
            let sess = vibeterm_agent_watch::claude::project::read_for_cwd(&cwd);
            let done = sess
                .as_ref()
                .map(|s| s.stop_reason.as_deref() == Some("end_turn"))
                .unwrap_or(false);
            // turn id = 末条 assistant 的 uuid;仅 done 时取,给完成判定去重(快轮也不漏)。
            let tid = if done { sess.and_then(|s| s.last_turn_id) } else { None };
            (true, done, tid)
        }
        "codex" => (
            false,
            vibeterm_agent_watch::codex::session::read_for_cwd(&cwd)
                .map(|s| s.task_completed)
                .unwrap_or(false),
            // codex 维持布尔跃迁判定 —— 现状每次都灵,不引入 turn_id 改动风险。
            None,
        ),
        _ => return,
    };
    // 诊断:每次轮询打出 read_for_cwd 读到的完成态 + turn_id。第二次完成若仍漏,看这里 ——
    // turn_id 变了却没 fire = 逻辑问题;turn_id 没变 = 选错会话文件(读到了旧/别的会话)。
    if is_claude {
        tracing::debug!(
            task_id,
            turn_done,
            turn_id = ?turn_id,
            "poll: claude transcript 完成态(read_for_cwd)"
        );
    }
    on_agent_turn_update(app, kind, &cwd, is_claude, turn_done, turn_id.as_deref());
}

/// 前台轻提示 / 持续提醒用的"前端可播声音名"。系统声音名(Glass 等)前端 `<audio>` 放不了,
/// 退到 bundled fallback,保证前台/持续场景一定有声(系统横幅那条路才用得了系统声音)。
fn frontend_sound_for(app: &AppHandle, configured: &str, fallback: &str) -> String {
    let (use_fe, _native, raw) = resolve_notify_sound(app, configured, fallback);
    if use_fe {
        raw
    } else {
        fallback.to_string()
    }
}

/// 间歇持续提醒(单路全局)。在 200ms tick 里调:有"未看完成"且主窗口失焦时,每隔
/// PERSISTENT_REMIND_INTERVAL 响 1 路声音催用户回来;未看数归零 / 主窗口聚焦 / 开关关
/// → reset(下次重新计时)。首次发现未看只记基准不响 —— "完成"通知本身已响过那一声,避免双响。
fn maybe_persistent_remind(app: &AppHandle, state: &AppState) {
    let reset = || {
        if let Ok(mut g) = state.last_persistent_remind.lock() {
            *g = None;
        }
    };
    let prefs = NotifyFile::load();
    if !prefs.enabled || !prefs.persistent_unseen_sound {
        reset();
        return;
    }
    if state.tasks.unseen_done_count() == 0 || main_window_focused(app) {
        // 看完了 / 人回到 app → 停止催促并重置计时
        reset();
        return;
    }
    let now_hhmm = chrono::Local::now().format("%H:%M").to_string();
    if prefs.quiet_hours.contains(&now_hhmm) {
        return; // 免打扰时段:不响,也不重置基准(出时段后接着按节奏来)
    }
    let now = std::time::Instant::now();
    {
        let mut g = match state.last_persistent_remind.lock() {
            Ok(g) => g,
            Err(_) => return,
        };
        match *g {
            // 首次发现"未看 + 失焦":只记基准不响(完成通知已响过那一声)
            None => {
                *g = Some(now);
                return;
            }
            Some(t) if now.duration_since(t) >= PERSISTENT_REMIND_INTERVAL => {
                *g = Some(now);
            }
            _ => return,
        }
    }
    let configured = prefs.events.done.sound.as_deref().unwrap_or("");
    let fe = frontend_sound_for(app, configured, "ringtone2");
    let _ = app.emit(
        "notification_play_sound",
        serde_json::json!({ "sound": fe }),
    );
}

/// 解析 sound 字段, 返回 (use_frontend_audio, native_sound, raw_sound).
/// raw_sound 用于 emit 给前端 <audio> 播自定义文件 / 自带库.
///
/// 三类 sound 字段:
///   - 绝对路径 / ~/ → 前端 <audio> 放 (native silent)
///   - 自带库名 (resource_dir/sounds/<name>.mp3) → 前端 <audio> 放 (OS 没这个名)
///   - macOS 系统声音名 (Glass/Tink/...) → 走 NSUserNotification.sound
fn resolve_notify_sound(
    app: &AppHandle,
    configured: &str,
    fallback: &str,
) -> (bool, String, String) {
    let cfg = configured.trim();
    if sound_is_file_path(cfg) {
        return (true, String::new(), cfg.to_string());
    }
    // 自带库:打包资源里的 sound id → 走前端音频, 跨平台一致
    if !cfg.is_empty() && is_bundled_sound(app, cfg) {
        return (true, String::new(), cfg.to_string());
    }
    let native = if !cfg.is_empty() { cfg } else { fallback };
    (false, native.to_string(), native.to_string())
}

fn is_bundled_sound(app: &AppHandle, name: &str) -> bool {
    let Ok(res_dir) = app.path().resource_dir() else {
        return false;
    };
    res_dir
        .join("resources/sounds")
        .join(format!("{name}.mp3"))
        .is_file()
}

/// 在聚合状态跃迁时弹系统通知。
///
/// 触发: ① 任意 → WaitingInput(等用户)。② agent 终端 Running→Idle 且 by_osc
/// (真完成 —— 标题 spinner→静态 / OSC D)→ "完成"通知。纯嗅探, 不依赖 hook。
///
/// Stalled 不弹通知 — 区分"agent 真挂了"vs"agent 完成等输入"在通用 TUI 协议层
/// 做不到, 视觉徽标 (任务列表呼吸动画) 已足够提示.
fn notify_status_transition(
    app: &AppHandle,
    tasks: &TaskRegistry,
    task_id: vibeterm_ipc::TaskId,
    prev: vibeterm_ipc::TaskStatus,
    new: vibeterm_ipc::TaskStatus,
    by_osc: bool,
) {
    use vibeterm_ipc::TaskStatus;

    if prev == new {
        return;
    }

    // agent 精确完成 → "完成"通知(纯嗅探, 替代原 hook 的 TurnComplete).
    // 触发条件: agent 终端 + Running→Idle + by_osc(真完成 —— OSC D 或标题 spinner→静态,
    // 非 800ms 超时误判). 授权等待会走 WaitingInput 分支而非 Idle, 不会误判完成.
    if matches!(new, TaskStatus::Idle) && matches!(prev, TaskStatus::Running) && by_osc {
        if let Some(agent) = tasks.agent_kind_of(task_id).ok().flatten() {
            let last = app
                .try_state::<AppState>()
                .and_then(|s| {
                    s.tasks
                        .task_dto(task_id)
                        .ok()
                        .flatten()
                        .and_then(|d| s.terminals.most_recent_tail(&d.terminal_ids))
                })
                .unwrap_or_default();
            fire_agent_completed_notification(
                app,
                tasks,
                task_id,
                &agent,
                &format!("task-{task_id}"),
                last.trim(),
            );
        }
        return;
    }

    if !matches!(new, TaskStatus::WaitingInput) {
        return;
    }
    // waiting_input 不放前台轻提示(allow_foreground=false → route 恒 Background,
    // 维持"前台一律静默";前台轻提示只用于"完成"通知,符合用户预期)。
    let Some((prefs, _route)) = notify_preflight(app, tasks, task_id, false) else {
        return;
    };
    let event_prefs = &prefs.events.waiting_input;
    if !event_prefs.enabled {
        return;
    }

    let last_output = app.try_state::<AppState>().and_then(|s| {
        s.tasks
            .task_dto(task_id)
            .ok()
            .flatten()
            .and_then(|d| s.terminals.most_recent_tail(&d.terminal_ids))
    });
    let agent_kind = tasks.agent_kind_of(task_id).ok().flatten();
    let label = task_label(tasks, task_id);
    let body = format_notify_body(&label, last_output.as_deref(), agent_kind.as_deref());
    let title = "VibeTerm — 等待你的输入".to_string();
    let configured = event_prefs.sound.as_deref().unwrap_or("");
    // 通知点击聚焦:把 task_id 编进通知 id(i32),前端 click listener 用它切到对应 task。
    // 声音由 send_notification 内部 afplay(绕开 webview),tone20 作 fallback。
    send_notification(app, task_id, title, body, configured, "tone20");
}

/// 实际发系统通知 + 自定义文件音效旁路 + 记录 last_notify (用于点击聚焦).
/// 把 builder 那块从 notify_status_transition 抽出来给 hook 路径复用.
fn send_notification(
    app: &AppHandle,
    task_id: vibeterm_ipc::TaskId,
    title: String,
    body: String,
    configured: &str,
    fallback: &str,
) {
    use tauri_plugin_notification::NotificationExt;
    let id: i32 = i32::try_from(task_id).unwrap_or(0);
    // 横幅:tauri-plugin-notification 走 Rust 端、不经 webview。不带 .sound —— 声音单独 afplay。
    match app
        .notification()
        .builder()
        .id(id)
        .title(title)
        .body(body)
        .show()
    {
        Ok(()) => {
            // 声音:Rust afplay 直接播文件,绕开 webview 的 autoplay/后台限制
            // (webview <audio> 在无 user gesture / 窗口后台时被拦 —— 这正是"有时没声音"的根因)。
            play_sound_native(app, configured, fallback);
            if let Some(s) = app.try_state::<AppState>() {
                if let Ok(mut g) = s.last_notify.lock() {
                    *g = Some((task_id, std::time::Instant::now()));
                }
            }
        }
        Err(e) => {
            tracing::debug!(err = %e, "notification show failed");
        }
    }
}

/// 进程内音频播放线程的句柄。afplay 每次 fork 新进程、开关默认音频输出设备,GUI app 子进程
/// 下连续播放第二次常哑(afplay 自身 exit 0,声音却没出来)。改用 rodio:常驻线程持有一个
/// OutputStream(设备一直开、不反复开关),每次播放只塞一个 Sink。
static AUDIO_TX: std::sync::OnceLock<std::sync::mpsc::Sender<std::path::PathBuf>> =
    std::sync::OnceLock::new();

/// 启动常驻音频线程(持有 rodio OutputStream)。app 启动时调用一次。
fn init_audio_thread() {
    let (tx, rx) = std::sync::mpsc::channel::<std::path::PathBuf>();
    let spawned = std::thread::Builder::new()
        .name("vibeterm-audio".into())
        .spawn(move || {
            let (_stream, handle) = match rodio::OutputStream::try_default() {
                Ok(s) => {
                    tracing::info!("audio: OutputStream 就绪(常驻)");
                    s
                }
                Err(e) => {
                    tracing::warn!(err = %e, "audio: 无默认输出设备,通知声音禁用");
                    return;
                }
            };
            // _stream 在本线程常驻 alive。串行播放:每条通知开一个 Sink,append 后
            // sleep_until_end 同步播到完再释放 —— 不用 detach。detach 是把 source 挂到 mixer
            // 异步播,连续播放时第二个 source 常不出声(用户实测:第二次弹了通知却没声音)。
            // 串行 = 每次独立 Sink、播完即释放,连续多次稳定。每步打 debug 便于诊断。
            while let Ok(path) = rx.recv() {
                tracing::info!(path = %path.display(), "audio: 收到播放请求");
                let src = match std::fs::File::open(&path) {
                    Ok(f) => match rodio::Decoder::new(std::io::BufReader::new(f)) {
                        Ok(s) => s,
                        Err(e) => {
                            tracing::warn!(err = %e, "audio: 解码失败");
                            continue;
                        }
                    },
                    Err(e) => {
                        tracing::warn!(err = %e, "audio: 打开文件失败");
                        continue;
                    }
                };
                match rodio::Sink::try_new(&handle) {
                    Ok(sink) => {
                        sink.append(src);
                        tracing::info!("audio: 开始播放(Sink)");
                        sink.sleep_until_end(); // 同步播到完,本线程串行
                        tracing::info!("audio: 播放结束");
                    }
                    Err(e) => tracing::warn!(err = %e, "audio: Sink 创建失败"),
                }
            }
            tracing::warn!("audio: 播放线程退出(channel 关闭)");
        });
    if spawned.is_ok() {
        let _ = AUDIO_TX.set(tx);
    } else {
        tracing::warn!("audio: 音频线程启动失败");
    }
}

/// 播放通知声音 —— 解析文件路径后发给常驻音频线程(rodio 进程内播放,绕开 afplay 反复 fork)。
/// `configured` 解析不到(空 / "default" / 无此文件)就退到 `fallback`;再不行则静默。
fn play_sound_native(app: &AppHandle, configured: &str, fallback: &str) {
    let path =
        resolve_sound_to_path(app, configured).or_else(|| resolve_sound_to_path(app, fallback));
    let Some(path) = path else {
        tracing::debug!(configured, fallback, "notify 声音:无可播文件,静默");
        return;
    };
    tracing::info!(path = %path.display(), "notify 声音 → rodio(send)");
    match AUDIO_TX.get() {
        Some(tx) => {
            let _ = tx.send(path);
        }
        None => tracing::warn!("notify 声音:音频线程未初始化"),
    }
}

/// agent hook 触发的"完成"通知 — 走完整守门 (preflight 通用),
/// 但事件 prefs 走 events.done (老 toml 字段, 语义重定义为 "agent_completed via hook").
/// throttle: 同 session_id 30s 内最多 1 发, 防 agent 来回对话连发.
pub fn fire_agent_completed_notification(
    app: &AppHandle,
    tasks: &TaskRegistry,
    task_id: vibeterm_ipc::TaskId,
    agent: &str,
    session_id: &str,
    last_message: &str,
) {
    let Some((prefs, route)) = notify_preflight(app, tasks, task_id, true) else {
        return;
    };
    let event_prefs = &prefs.events.done;
    if !event_prefs.enabled {
        return;
    }

    // session 级 throttle (来回对话 5~10s 一发 turn, 用 30s 间隔合并).
    if let Some(s) = app.try_state::<AppState>() {
        let mut g = match s.last_agent_completed.lock() {
            Ok(g) => g,
            Err(_) => return,
        };
        let now = std::time::Instant::now();
        if let Some(prev) = g.get(session_id) {
            if now.duration_since(*prev) < AGENT_COMPLETED_COOLDOWN {
                tracing::debug!(session_id, "agent_completed throttled");
                return;
            }
        }
        // 清理超出冷却窗口(取 2× 留余量)的旧 session 条目, 避免 map 无界增长.
        g.retain(|_, t| now.duration_since(*t) < AGENT_COMPLETED_COOLDOWN * 2);
        g.insert(session_id.to_string(), now);
    }

    let label = task_label(tasks, task_id);
    let title = format!("VibeTerm — {agent} 完成");
    let body = if last_message.is_empty() {
        label
    } else {
        format!("{label} · {last_message}")
    };
    let configured = event_prefs.sound.as_deref().unwrap_or("");
    match route {
        NotifyRoute::Background => {
            // 后台:系统横幅 + afplay 声音,都走 Rust 端、不经 webview。
            tracing::info!(configured, "fire 完成通知 → Background(横幅 + afplay)");
            send_notification(app, task_id, title, body, configured, "ringtone2");
        }
        NotifyRoute::ForegroundLight => {
            // 前台轻提示:不发横幅(macOS 前台横幅常被吞),只 afplay 一声 + 列表行高亮。
            // afplay 是独立进程,前台也不受 webview autoplay 限制,稳。
            tracing::info!(
                configured,
                "fire 完成通知 → ForegroundLight(afplay + 行高亮)"
            );
            play_sound_native(app, configured, "ringtone2");
            let _ = app.emit("task_flash", task_id);
        }
    }
}

fn task_label(tasks: &TaskRegistry, id: vibeterm_ipc::TaskId) -> String {
    tasks
        .name_of(id)
        .ok()
        .flatten()
        .unwrap_or_else(|| format!("task #{id}"))
}

/// 拼通知 body. 三段式 — task_label · [agent_kind] · last_output(截 60 字).
/// last_output / agent_kind 缺失时优雅降级.
fn format_notify_body(label: &str, last_output: Option<&str>, agent_kind: Option<&str>) -> String {
    let mut parts = vec![label.to_string()];
    if let Some(k) = agent_kind {
        parts.push(format!("[{k}]"));
    }
    if let Some(t) = last_output {
        // 60 字符截断 (按 char count, 中日韩 char 也算 1)
        const MAX_TAIL: usize = 60;
        let chars: Vec<char> = t.chars().collect();
        let tail = if chars.len() > MAX_TAIL {
            let mut s: String = chars.into_iter().take(MAX_TAIL).collect();
            s.push('…');
            s
        } else {
            t.to_string()
        };
        parts.push(tail);
    }
    parts.join(" · ")
}

/// 展开 cwd 字符串里的 `~` 和 `$VAR` / `${VAR}`,验证目录存在;
/// 路径不存在或无法 stat 时 fallback $HOME。
fn expand_and_validate_cwd(input: &str) -> String {
    let expanded = expand_path_str(input);
    if std::path::Path::new(&expanded).is_dir() {
        expanded
    } else {
        tracing::warn!(
            input,
            expanded,
            "cwd not a directory, falling back to $HOME"
        );
        std::env::var("HOME").unwrap_or_else(|_| ".".into())
    }
}

fn expand_path_str(input: &str) -> String {
    let trimmed = input.trim();
    // ~ / ~/...
    let after_tilde: String = if trimmed == "~" {
        std::env::var("HOME").unwrap_or_else(|_| trimmed.into())
    } else if let Some(rest) = trimmed.strip_prefix("~/") {
        let home = std::env::var("HOME").unwrap_or_default();
        if home.is_empty() {
            trimmed.into()
        } else {
            format!("{}/{}", home.trim_end_matches('/'), rest)
        }
    } else {
        trimmed.into()
    };
    // $VAR / ${VAR}
    expand_env_vars(&after_tilde)
}

fn expand_env_vars(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = String::with_capacity(s.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'$' && i + 1 < bytes.len() {
            // ${NAME}
            if bytes[i + 1] == b'{' {
                if let Some(end) = s[i + 2..].find('}') {
                    let name = &s[i + 2..i + 2 + end];
                    if let Ok(v) = std::env::var(name) {
                        out.push_str(&v);
                    }
                    i += 2 + end + 1;
                    continue;
                }
            }
            // $NAME — 取 [A-Za-z_][A-Za-z0-9_]*
            let start = i + 1;
            let mut j = start;
            while j < bytes.len() {
                let c = bytes[j];
                let valid = c.is_ascii_alphanumeric() || c == b'_';
                if j == start && !(c.is_ascii_alphabetic() || c == b'_') {
                    break;
                }
                if !valid {
                    break;
                }
                j += 1;
            }
            if j > start {
                let name = &s[start..j];
                if let Ok(v) = std::env::var(name) {
                    out.push_str(&v);
                }
                i = j;
                continue;
            }
        }
        // 非变量字节:按 UTF-8 char 边界整体推入,避免逐字节 `as char`
        // 把多字节序列(CJK 等)拆成 Latin-1 mojibake。
        match s[i..].chars().next() {
            Some(ch) => {
                out.push(ch);
                i += ch.len_utf8();
            }
            None => break,
        }
    }
    out
}

#[cfg(test)]
mod cwd_expand_tests {
    use super::*;

    #[test]
    fn expand_tilde_only() {
        std::env::set_var("HOME", "/Users/test");
        assert_eq!(expand_path_str("~"), "/Users/test");
    }

    #[test]
    fn expand_tilde_slash() {
        std::env::set_var("HOME", "/Users/test");
        assert_eq!(
            expand_path_str("~/projects/foo"),
            "/Users/test/projects/foo"
        );
    }

    #[test]
    fn expand_env_var_braced() {
        std::env::set_var("FOO_DIR", "/opt/foo");
        assert_eq!(expand_path_str("${FOO_DIR}/bar"), "/opt/foo/bar");
    }

    #[test]
    fn expand_env_var_plain() {
        std::env::set_var("HOME", "/Users/test");
        assert_eq!(expand_path_str("$HOME/x"), "/Users/test/x");
    }

    #[test]
    fn absolute_path_unchanged() {
        assert_eq!(expand_path_str("/usr/local/bin"), "/usr/local/bin");
    }

    #[test]
    fn cjk_path_preserved() {
        std::env::set_var("HOME", "/Users/test");
        assert_eq!(expand_path_str("/Users/test/项目"), "/Users/test/项目");
        assert_eq!(expand_path_str("~/日本語"), "/Users/test/日本語");
    }

    #[test]
    fn cjk_path_with_env_var() {
        std::env::set_var("HOME", "/Users/test");
        assert_eq!(expand_path_str("$HOME/中文目录"), "/Users/test/中文目录");
    }
}

// ============================
// IPC commands — Terminal
// ============================

#[tauri::command]
async fn start_pty(
    opts: SpawnPtyOpts,
    channel: Channel<Vec<u8>>,
    state: tauri::State<'_, AppState>,
    app: AppHandle,
) -> IpcResult<SpawnPtyResult> {
    spawn_inner(opts, channel, None, &state, &app)
}

// 在指定 task 下 spawn(可选 slot_id 做幂等)
#[tauri::command]
async fn spawn_terminal_in_task(
    task_id: vibeterm_ipc::TaskId,
    slot_id: Option<u32>,
    opts: SpawnPtyOpts,
    channel: Channel<Vec<u8>>,
    state: tauri::State<'_, AppState>,
    app: AppHandle,
) -> IpcResult<SpawnPtyResult> {
    // (task, slot) 幂等 — 用 slot_lock 序列化"查 + spawn + bind",避免并发 race。
    // 没 slot_id 走旧 spawn 路径(不做幂等)。
    let Some(sid) = slot_id else {
        return spawn_inner(opts, channel, Some(task_id), &state, &app);
    };

    let lock = state
        .tasks
        .slot_lock(task_id, sid)
        .map_err(|_| IpcError::Unknown {
            trace_id: "slot_locks poisoned".into(),
        })?;
    let _guard = lock.lock().map_err(|_| IpcError::Unknown {
        trace_id: "slot_lock poisoned".into(),
    })?;

    // 临界区:lock 拿到后,先查;还没绑 → spawn 后写回;已绑 → attach 共享 PTY
    if let Ok(Some(existing)) = state.tasks.terminal_for_slot(task_id, sid) {
        // PassThroughSink:只透传字节,不做 status 嗅探(主 sink 已在做)
        struct PassThroughSink {
            channel: Channel<Vec<u8>>,
        }
        impl vibeterm_pty::ChunkSink for PassThroughSink {
            fn push(&self, chunk: Vec<u8>) {
                let _ = self.channel.send(chunk);
            }
            fn finish(&self, _info: vibeterm_pty::ExitInfo) {}
        }
        state
            .terminals
            .attach_sink(existing, PassThroughSink { channel })
            .map_err(|e| IpcError::PtySpawnFailed {
                reason: e.to_string(),
            })?;
        return Ok(SpawnPtyResult {
            terminal_id: existing,
        });
    }

    let result = spawn_inner(opts, channel, Some(task_id), &state, &app)?;
    if let Err(e) = state.tasks.bind_slot(task_id, sid, result.terminal_id) {
        // 绑定失败会导致下次同 slot 幂等 spawn 无法复用,重复造 PTY — 至少留痕
        tracing::warn!(err = %e, task_id = %task_id, slot = %sid, terminal_id = %result.terminal_id, "bind_slot failed");
    }
    Ok(result)
}

/// command(shell 路径)的 basename 是否为 zsh。
fn is_zsh(command: &str) -> bool {
    std::path::Path::new(command)
        .file_name()
        .and_then(|n| n.to_str())
        .is_some_and(|n| n == "zsh")
}

/// 读 config.toml 的 shell_integration 开关(默认 true)。每次 spawn 读一次,
/// 设置改动下次开终端即生效,无需重启。
fn shell_integration_enabled() -> bool {
    Config::load().map(|c| c.shell_integration).unwrap_or(true)
}

/// 确保 zsh 集成的 ZDOTDIR 目录存在(config_dir/shell-integration/zsh),写入 4 个
/// wrapper 文件。app 自有目录,非用户 dotfiles。整个 app 生命周期写一次(OnceLock 缓存)。
fn ensure_zsh_zdotdir() -> Option<std::path::PathBuf> {
    static DIR: std::sync::OnceLock<Option<std::path::PathBuf>> = std::sync::OnceLock::new();
    DIR.get_or_init(|| {
        let dir = vibeterm_config::config_dir()
            .ok()?
            .join("shell-integration")
            .join("zsh");
        std::fs::create_dir_all(&dir).ok()?;
        let files = [
            (".zshenv", include_str!("shell-hooks/zdotdir/zshenv")),
            (".zprofile", include_str!("shell-hooks/zdotdir/zprofile")),
            (".zshrc", include_str!("shell-hooks/zdotdir/zshrc")),
            (".zlogin", include_str!("shell-hooks/zdotdir/zlogin")),
        ];
        for (name, content) in files {
            if let Err(e) = std::fs::write(dir.join(name), content) {
                tracing::warn!(err = %e, file = name, "write zsh zdotdir wrapper failed");
                return None;
            }
        }
        Some(dir)
    })
    .clone()
}

fn spawn_inner(
    opts: SpawnPtyOpts,
    channel: Channel<Vec<u8>>,
    task_id: Option<vibeterm_ipc::TaskId>,
    state: &AppState,
    app: &AppHandle,
) -> IpcResult<SpawnPtyResult> {
    // cwd 优先级:opts.cwd > task.cwd > $HOME。
    // 不变式:挂了 worktree 的 task,task.cwd 始终 = worktree_path
    //   (见 vibeterm-core::tasks::create / attach_worktree),所以本路径自动用对。
    let cwd_raw = opts
        .cwd
        .or_else(|| task_id.and_then(|t| state.tasks.cwd(t).ok().flatten()))
        .unwrap_or_else(|| std::env::var("HOME").unwrap_or_else(|_| ".".into()));
    // 用户可能填 `~/projects/foo` 或 `$HOME/x` — shell 展开 ~ 但 posix_spawn 不会,
    // 这里手工展开;展开后路径不存在 fallback $HOME,避免 PTY 静默 chdir 失败 → 进程 cwd。
    let cwd = expand_and_validate_cwd(&cwd_raw);
    let command = opts
        .command
        .unwrap_or_else(|| std::env::var("SHELL").unwrap_or_else(|_| default_shell().into()));
    let args = opts.args.unwrap_or_default();

    // 4 层 env 合并:
    //   1. 进程继承(自动,由 portable-pty 处理)
    //   2. env.toml 全局
    //   3. 任务级 env(待加 task.env 字段;当前 None)
    //   4. 命令行内联(opts.env)
    let mut merged: std::collections::HashMap<String, String> = std::collections::HashMap::new();
    if let Ok(envfile) = EnvFile::load() {
        for (k, v) in envfile.to_env_pairs() {
            merged.insert(k, v);
        }
    }
    for (k, v) in opts.env.unwrap_or_default() {
        merged.insert(k, v);
    }
    // 默认 TERM / COLORTERM / TERM_PROGRAM(后者让 shell integration hook 识别)
    merged
        .entry("TERM".into())
        .or_insert_with(|| "xterm-256color".into());
    merged
        .entry("COLORTERM".into())
        .or_insert_with(|| "truecolor".into());
    merged
        .entry("TERM_PROGRAM".into())
        .or_insert_with(|| "vibeterm".into());
    merged
        .entry("TERM_PROGRAM_VERSION".into())
        .or_insert_with(|| env!("CARGO_PKG_VERSION").into());

    // shell 集成自动注入(默认开,config 可关):为 zsh 设临时 ZDOTDIR,让现成的
    // OSC 133 parser 拿到 shell 权威的 prompt/command/exit-code 标记。VS Code/Ghostty
    // 式 ephemeral 注入 —— 只设临时 env + 写 app 自有目录的 wrapper,绝不碰用户 dotfiles。
    if is_zsh(&command) && shell_integration_enabled() {
        if let Some(dir) = ensure_zsh_zdotdir() {
            // 用户原始 ZDOTDIR(通常未设 → $HOME)交给 wrapper 链接回去
            let user_zdotdir = merged
                .get("ZDOTDIR")
                .cloned()
                .or_else(|| std::env::var("ZDOTDIR").ok())
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| std::env::var("HOME").unwrap_or_else(|_| "~".into()));
            merged.insert("VIBETERM_USER_ZDOTDIR".into(), user_zdotdir);
            merged.insert("VIBETERM_INJECTION".into(), "1".into());
            merged.insert("ZDOTDIR".into(), dir.to_string_lossy().into_owned());
        }
    }

    let env: Vec<(String, String)> = merged.into_iter().collect();

    // 预分配 sink 需要 terminal_id;先 spawn 占位,再回填 — 简化:用同一 mutex 加锁
    // 实际做法:先确定 terminal_id,在 spawn() 之前通过 sink 包装好
    // TerminalRegistry::spawn 内部分配 id;这里我们重构成两步:reserve_id + spawn_with_id
    // 简化:在 spawn 之后立即 attach,sink 的 terminal_id 在 spawn 前用 "next_id" 探测一次
    // 实际可接受:terminal_id 估计值与真值差 0(单线程顺序)— 但有 race。
    // 为正确性,改 sink 持有 Mutex<Option<TerminalId>>,spawn 完后 set。

    let term_id_holder: Arc<std::sync::Mutex<Option<TerminalId>>> =
        Arc::new(std::sync::Mutex::new(None));
    let status = Arc::new(std::sync::Mutex::new(StatusDetector::new(&command)));
    let sink = LazyChannelSink {
        channel,
        terminal_id_holder: term_id_holder.clone(),
        status: status.clone(),
        tasks: state.tasks.clone(),
        app: app.clone(),
    };
    let id = state
        .terminals
        .spawn(
            SpawnOpts {
                rows: opts.rows,
                cols: opts.cols,
                cwd,
                command,
                args,
                env,
            },
            sink,
        )
        .map_err(|e| IpcError::PtySpawnFailed {
            reason: e.to_string(),
        })?;
    *term_id_holder.lock().unwrap_or_else(|p| p.into_inner()) = Some(id);

    // 注册 status detector 到 AppState. 全局 status-tick 任务(见 setup)周期 tick
    // 这里所有 detector 做 stall/idle 时间态判定; close / PTY 退出时摘除条目.
    if let Ok(mut map) = state.status_detectors.lock() {
        map.insert(id, status.clone());
    }

    if let Some(task_id) = task_id {
        if let Err(e) = state.tasks.attach_terminal(task_id, id) {
            // 绑定失败 → task 的 terminal_ids 不含该 id,后续 resize/write/status 按 task 定位会落空
            tracing::warn!(err = %e, task_id = %task_id, terminal_id = id, "attach_terminal failed");
        }
        emit_tasks_changed(app, &state.tasks);
    }
    Ok(SpawnPtyResult { terminal_id: id })
}

struct LazyChannelSink {
    channel: Channel<Vec<u8>>,
    terminal_id_holder: Arc<std::sync::Mutex<Option<TerminalId>>>,
    status: Arc<std::sync::Mutex<StatusDetector>>,
    tasks: Arc<TaskRegistry>,
    app: AppHandle,
}

impl ChunkSink for LazyChannelSink {
    fn push(&self, chunk: Vec<u8>) {
        let id_opt = *self
            .terminal_id_holder
            .lock()
            .unwrap_or_else(|p| p.into_inner());
        // 状态嗅探(C1 修订:status 用 broadcast 模式,这里同步 feed 简化为单调用)
        // 一次锁取 (new_status, idle_by_osc),后者标记 Idle 是 OSC D 真完成
        // 还是 timeout 误判,通知层用它过滤 ping 之类的假完成.
        let (new_status, idle_by_osc, sniffed_effort) = {
            let mut d = self.status.lock().unwrap_or_else(|p| p.into_inner());
            let s = d.feed(&chunk);
            (s, d.idle_by_osc(), d.last_effort().map(|x| x.to_string()))
        };
        if let (Some(id), Some(s)) = (id_opt, new_status) {
            if let Ok(Some((task_id, prev_agg, new_agg))) =
                self.tasks.update_terminal_status(id, s, idle_by_osc)
            {
                let _ = self.app.emit(
                    "task_status_changed",
                    serde_json::json!({"task_id": task_id, "status": s}),
                );
                emit_tasks_changed(&self.app, &self.tasks);
                notify_status_transition(
                    &self.app,
                    &self.tasks,
                    task_id,
                    prev_agg,
                    new_agg,
                    idle_by_osc,
                );
                refresh_dock_badge(&self.app, &self.tasks);
            }
        }
        // effort: 嗅探到 "thinking with X effort" → 写 task.effort(widget 回退读它).
        // set_effort_for_terminal 仅在真变化时返回 Some, 故 emit 不会随每帧 spinner 刷屏.
        if let (Some(id), Some(eff)) = (id_opt, sniffed_effort) {
            if let Ok(Some(_)) = self.tasks.set_effort_for_terminal(id, Some(eff)) {
                emit_tasks_changed(&self.app, &self.tasks);
            }
        }
        if let Err(e) = self.channel.send(chunk) {
            tracing::warn!(error = %e, "channel send failed");
        }
    }
    fn finish(&self, info: ExitInfo) {
        if let Some(id) = *self
            .terminal_id_holder
            .lock()
            .unwrap_or_else(|p| p.into_inner())
        {
            // PTY 自然退出:从 status_detectors 摘除,全局 tick 任务随即不再 tick 它.
            if let Some(state) = self.app.try_state::<AppState>() {
                if let Ok(mut map) = state.status_detectors.lock() {
                    map.remove(&id);
                }
            }
            tracing::info!(?info, terminal_id = id, "pty exited");
            let _ = self.app.emit(
                "terminal_exited",
                serde_json::json!({"terminal_id": id, "exit_code": info.exit_code}),
            );
        }
    }
}

#[tauri::command]
async fn write_pty(
    id: TerminalId,
    data: Vec<u8>,
    state: tauri::State<'_, AppState>,
) -> IpcResult<()> {
    // 标记"用户在动" — Stalled 检测要求 last_user_input > last_chunk_at,
    // 排除 agent 跑完任务停在 prompt 长期空闲被误判为卡住的场景.
    if !data.is_empty() {
        if let Ok(detectors) = state.status_detectors.lock() {
            if let Some(d) = detectors.get(&id) {
                if let Ok(mut det) = d.lock() {
                    det.mark_user_input();
                }
            }
        }
    }
    state.terminals.write(id, &data).map_err(|e| match e {
        vibeterm_core::TerminalRegistryError::NotFound(id) => IpcError::NotFound {
            resource: "terminal".into(),
            id: id.to_string(),
        },
        other => IpcError::Unknown {
            trace_id: format!("write_pty:{other}"),
        },
    })
}

#[tauri::command]
async fn resize_pty(
    id: TerminalId,
    rows: u16,
    cols: u16,
    state: tauri::State<'_, AppState>,
) -> IpcResult<()> {
    state.terminals.resize(id, rows, cols).map_err(|e| match e {
        vibeterm_core::TerminalRegistryError::NotFound(id) => IpcError::NotFound {
            resource: "terminal".into(),
            id: id.to_string(),
        },
        other => IpcError::Unknown {
            trace_id: format!("resize_pty:{other}"),
        },
    })
}

// attach 已有 terminal 给新 sink(浮窗 reparent 用,共享 PTY 流)
#[tauri::command]
async fn attach_terminal(
    id: TerminalId,
    channel: Channel<Vec<u8>>,
    state: tauri::State<'_, AppState>,
    app: AppHandle,
) -> IpcResult<u64> {
    let term_id_holder = Arc::new(std::sync::Mutex::new(Some(id)));
    // 状态嗅探:复用主 sink 已注册的 detector(共享同一份),避免浮窗用独立
    // detector 因 agent_rules 不一致而对同一 term_id 产生状态覆盖/抖动.
    // 找不到(非 spawn 路径的终端)才退化为独立 detector,此时无主 detector 可冲突.
    let status = state
        .status_detectors
        .lock()
        .ok()
        .and_then(|map| map.get(&id).cloned())
        .unwrap_or_else(|| Arc::new(std::sync::Mutex::new(StatusDetector::new("zsh"))));
    let sink = LazyChannelSink {
        channel,
        terminal_id_holder: term_id_holder,
        status,
        tasks: state.tasks.clone(),
        app: app.clone(),
    };
    let sink_id = state.terminals.attach_sink(id, sink).map_err(|e| match e {
        vibeterm_core::TerminalRegistryError::NotFound(id) => IpcError::NotFound {
            resource: "terminal".into(),
            id: id.to_string(),
        },
        other => IpcError::Unknown {
            trace_id: format!("attach:{other}"),
        },
    })?;
    Ok(sink_id)
}

// 设置页用:返回当前生效的 clipboard images dir 绝对路径(便于 UI 显示)
#[tauri::command]
async fn get_clipboard_images_dir() -> IpcResult<String> {
    vibeterm_config::clipboard_images_dir()
        .map(|p| p.to_string_lossy().into_owned())
        .map_err(|e| IpcError::Unknown {
            trace_id: format!("get_clipboard_images_dir:{e}"),
        })
}

// 设置页"打开目录"按钮:在 Finder / Explorer / xdg-open 里 reveal
#[tauri::command]
async fn open_clipboard_images_dir() -> IpcResult<()> {
    let dir = vibeterm_config::clipboard_images_dir().map_err(|e| IpcError::Unknown {
        trace_id: format!("open_clipboard_images_dir:dir:{e}"),
    })?;
    #[cfg(target_os = "macos")]
    let r = std::process::Command::new("open").arg(&dir).status();
    #[cfg(target_os = "windows")]
    let r = std::process::Command::new("explorer").arg(&dir).status();
    #[cfg(all(unix, not(target_os = "macos")))]
    let r = std::process::Command::new("xdg-open").arg(&dir).status();
    r.map_err(|e| IpcError::Unknown {
        trace_id: format!("open_clipboard_images_dir:spawn:{e}"),
    })?;
    Ok(())
}

// 设置页"清空所有"按钮:删 dir 下所有 *.png
#[tauri::command]
async fn clear_clipboard_images() -> IpcResult<usize> {
    let dir = vibeterm_config::clipboard_images_dir().map_err(|e| IpcError::Unknown {
        trace_id: format!("clear_clipboard_images:dir:{e}"),
    })?;
    let mut removed = 0usize;
    let read = match std::fs::read_dir(&dir) {
        Ok(r) => r,
        Err(_) => return Ok(0),
    };
    for entry in read.flatten() {
        let p = entry.path();
        if p.extension().and_then(|s| s.to_str()) == Some("png") && std::fs::remove_file(&p).is_ok()
        {
            removed += 1;
        }
    }
    tracing::info!(removed, "clear_clipboard_images");
    Ok(removed)
}

// 把粘贴板里的 image bitmap 写到 clipboard-images/<ts>.png,返回绝对路径。
// 前端 paste 事件检测到 image MIME 时调用,然后把路径 writePty 进终端。
#[tauri::command]
async fn save_clipboard_image(bytes: Vec<u8>) -> IpcResult<String> {
    vibeterm_config::save_clipboard_image_default(&bytes)
        .map(|p| p.to_string_lossy().into_owned())
        .map_err(|e| IpcError::Unknown {
            trace_id: format!("save_clipboard_image:{e}"),
        })
}

// 一次性读剪贴板图片 → 编 PNG → 存盘,返回绝对路径(无图返回 None)。
// 把整条链放 Rust:
//   1) 走 tauri-plugin-clipboard-manager(包 arboard),直接 OS 级访问
//   2) image crate 编 PNG(剪贴板返回 RGBA raw)
//   3) vibeterm-config FIFO 落盘
// 前端 Cmd+V 主动调用,绕开 WebView paste 事件的 image content 兼容性问题。
#[derive(Debug, Clone, serde::Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum PasteResult {
    /// 优先:剪贴板里有文件 URL → 直接插路径(避开 Finder Cmd+C 图片
    /// 把缩略 icon 放进 bitmap 字段的陷阱)
    Files {
        paths: Vec<String>,
    },
    /// 剪贴板里是 image bitmap(截图工具) → Rust 后台编 PNG + 落盘
    Image {
        path: String,
    },
    /// 纯文本 → 走 xterm.paste(保留 bracketed paste 行为)
    Text {
        text: String,
    },
    Empty,
}

#[tauri::command]
async fn paste_clipboard_image(app: AppHandle) -> IpcResult<Option<String>> {
    // 兼容旧路径:仅返回 image 分支
    match paste_clipboard(app).await? {
        PasteResult::Image { path } => Ok(Some(path)),
        _ => Ok(None),
    }
}

// 一次 IPC 同时尝试 image + text,省一次往返。
// image 优先(screenshot 场景同时有 text 时仍应注入图);无图 fallback text。
//
// 命名策略:**内容 hash**(blake3 截前 16 hex 字符)— 同一截图反复粘贴
// 命中同一文件,二次秒返回 + 不占额外磁盘。也避免了"异步落盘"竞态
// (codex/claude-code 等 TUI 立即去 image::image_dimensions 读文件,
// 必须返回前文件已就位)。
#[tauri::command]
async fn paste_clipboard(app: AppHandle) -> IpcResult<PasteResult> {
    use std::time::Instant;

    let t0 = Instant::now();
    let clip = app.clipboard();

    // 优先 1:剪贴板含文件 URL(Finder Cmd+C 图片/文件 → 路径直插)
    // 这一步必须在 read_image 之前 —— 否则 Finder 的图标缩略会被当作 bitmap 落盘
    let files = clipboard_files::read_clipboard_files();
    if !files.is_empty() {
        tracing::debug!(
            n = files.len(),
            total_ms = t0.elapsed().as_millis() as u64,
            "paste_clipboard files"
        );
        return Ok(PasteResult::Files { paths: files });
    }

    if let Ok(img) = clip.read_image() {
        let (w, h) = (img.width(), img.height());
        let rgba = img.rgba();
        let t_read = t0.elapsed();

        // 内容 hash —— 同图反复粘贴命中同一路径(blake3 ~10GB/s,16MB → ~1.5ms)
        let hash = blake3::hash(rgba);
        let hex = hash.to_hex();
        let short = &hex.as_str()[..16];
        let dir = vibeterm_config::clipboard_images_dir().map_err(|e| IpcError::Unknown {
            trace_id: format!("paste_clipboard:dir:{e}"),
        })?;
        let target = dir.join(format!("{short}.png"));
        let target_str = target.to_string_lossy().into_owned();

        // 文件已存在 → 直接返回,跳过编码 + 写盘
        if target.exists() {
            // 刷新 mtime,让 FIFO 清理把它当"最近用过"留到最后
            if let Ok(f) = std::fs::OpenOptions::new().write(true).open(&target) {
                let _ = f.set_modified(std::time::SystemTime::now());
            }
            tracing::debug!(
                hash = short,
                total_ms = t0.elapsed().as_millis() as u64,
                "paste_clipboard image (hit cache)"
            );
            return Ok(PasteResult::Image { path: target_str });
        }

        // 首次见到此图 → 同步编 PNG + 落盘(codex 进程后续读盘必须命中)
        use image::codecs::png::{CompressionType, FilterType, PngEncoder};
        use image::{ExtendedColorType, ImageEncoder};
        let t_enc_start = Instant::now();
        let mut png_bytes: Vec<u8> = Vec::with_capacity((w as usize) * (h as usize));
        let encoder = PngEncoder::new_with_quality(
            &mut png_bytes,
            CompressionType::Fast,
            FilterType::NoFilter,
        );
        encoder
            .write_image(rgba, w, h, ExtendedColorType::Rgba8)
            .map_err(|e| IpcError::Unknown {
                trace_id: format!("paste_clipboard:encode:{e}"),
            })?;
        let t_encode = t_enc_start.elapsed();

        vibeterm_config::save_clipboard_image_at(&target, &png_bytes).map_err(|e| {
            IpcError::Unknown {
                trace_id: format!("paste_clipboard:save:{e}"),
            }
        })?;
        // FIFO 清理(用统一上限)
        let (max_count, max_bytes) = vibeterm_config::clipboard_images_caps();
        let _ = vibeterm_config::enforce_clipboard_images_caps(&dir, max_count, max_bytes);

        tracing::info!(
            w,
            h,
            hash = short,
            png_kb = png_bytes.len() / 1024,
            read_ms = t_read.as_millis() as u64,
            encode_ms = t_encode.as_millis() as u64,
            total_ms = t0.elapsed().as_millis() as u64,
            "paste_clipboard image (saved)"
        );
        return Ok(PasteResult::Image { path: target_str });
    }
    if let Ok(text) = clip.read_text() {
        if !text.is_empty() {
            tracing::debug!(
                len = text.len(),
                total_ms = t0.elapsed().as_millis() as u64,
                "paste_clipboard text"
            );
            return Ok(PasteResult::Text { text });
        }
    }
    Ok(PasteResult::Empty)
}

// 读 scrollback 快照(独立查询,不订阅;给搜索/导出/调试用)
#[tauri::command]
async fn get_scrollback(id: TerminalId, state: tauri::State<'_, AppState>) -> IpcResult<Vec<u8>> {
    state.terminals.scrollback(id).map_err(|e| match e {
        vibeterm_core::TerminalRegistryError::NotFound(id) => IpcError::NotFound {
            resource: "terminal".into(),
            id: id.to_string(),
        },
        other => IpcError::Unknown {
            trace_id: format!("get_scrollback:{other}"),
        },
    })
}

// 取消订阅(浮窗 Terminal 组件 onCleanup;**不** 关闭 PTY)
#[tauri::command]
async fn detach_terminal(
    id: TerminalId,
    sink_id: u64,
    state: tauri::State<'_, AppState>,
) -> IpcResult<()> {
    state
        .terminals
        .detach_sink(id, sink_id)
        .map_err(|e| match e {
            vibeterm_core::TerminalRegistryError::NotFound(id) => IpcError::NotFound {
                resource: "terminal".into(),
                id: id.to_string(),
            },
            other => IpcError::Unknown {
                trace_id: format!("detach:{other}"),
            },
        })
}

#[tauri::command]
async fn close_pty(
    id: TerminalId,
    state: tauri::State<'_, AppState>,
    app: AppHandle,
) -> IpcResult<()> {
    let _ = state.tasks.detach_terminal(id);
    // 幂等 slot 映射也要清,不然下次同 slot spawn 还会 attach 死 PTY
    let _ = state.tasks.unbind_terminal(id);
    // 清掉 status detector 注册 — 全局 tick 任务随即不再 tick 它
    if let Ok(mut map) = state.status_detectors.lock() {
        map.remove(&id);
    }
    let r = state.terminals.close(id).map_err(|e| match e {
        vibeterm_core::TerminalRegistryError::NotFound(id) => IpcError::NotFound {
            resource: "terminal".into(),
            id: id.to_string(),
        },
        other => IpcError::Unknown {
            trace_id: format!("close_pty:{other}"),
        },
    });
    emit_tasks_changed(&app, &state.tasks);
    r
}

// ============================
// IPC commands — Tasks
// ============================

#[tauri::command]
async fn list_tasks(state: tauri::State<'_, AppState>) -> IpcResult<Vec<TaskDto>> {
    state.tasks.list().map_err(map_task_err)
}

#[tauri::command]
async fn create_task(
    opts: CreateTaskOpts,
    state: tauri::State<'_, AppState>,
    app: AppHandle,
) -> IpcResult<TaskDto> {
    let id = state
        .tasks
        .create(opts.name, opts.cwd, opts.worktree)
        .map_err(map_task_err)?;
    emit_tasks_changed(&app, &state.tasks);
    state
        .tasks
        .task_dto(id)
        .map_err(map_task_err)?
        .ok_or(IpcError::NotFound {
            resource: "task".into(),
            id: id.to_string(),
        })
}

#[tauri::command]
async fn close_task(
    id: vibeterm_ipc::TaskId,
    state: tauri::State<'_, AppState>,
    app: AppHandle,
) -> IpcResult<()> {
    let term_ids = state.tasks.close(id).map_err(map_task_err)?;
    for tid in &term_ids {
        if let Err(e) = state.terminals.close(*tid) {
            tracing::warn!(err = %e, terminal_id = %tid, "close_task: terminal close failed");
        }
    }
    // 清掉相应 status detector 注册 — 全局 tick 任务随即不再 tick 它们
    if let Ok(mut map) = state.status_detectors.lock() {
        for tid in &term_ids {
            map.remove(tid);
        }
    }
    emit_tasks_changed(&app, &state.tasks);
    // 关掉的任务可能是 Done(未看)→ 刷新 Dock 角标
    refresh_dock_badge(&app, &state.tasks);
    Ok(())
}

#[tauri::command]
async fn rename_task(
    id: vibeterm_ipc::TaskId,
    name: String,
    state: tauri::State<'_, AppState>,
    app: AppHandle,
) -> IpcResult<()> {
    state.tasks.rename(id, name).map_err(map_task_err)?;
    emit_tasks_changed(&app, &state.tasks);
    Ok(())
}

#[tauri::command]
async fn pin_task(
    id: vibeterm_ipc::TaskId,
    pinned: bool,
    state: tauri::State<'_, AppState>,
    app: AppHandle,
) -> IpcResult<()> {
    state.tasks.pin(id, pinned).map_err(map_task_err)?;
    emit_tasks_changed(&app, &state.tasks);
    Ok(())
}

/// 切换 task 通知静音(持久化到 tasks.json).
#[tauri::command]
async fn set_task_notify_muted(
    id: vibeterm_ipc::TaskId,
    muted: bool,
    state: tauri::State<'_, AppState>,
    app: AppHandle,
) -> IpcResult<()> {
    state
        .tasks
        .set_notify_muted(id, muted)
        .map_err(map_task_err)?;
    emit_tasks_changed(&app, &state.tasks);
    Ok(())
}

/// 读 notify.toml.
#[tauri::command]
async fn get_notify_prefs() -> IpcResult<NotifyFile> {
    Ok(NotifyFile::load())
}

/// 整体覆盖写 notify.toml. atomic_write 保证 reload 安全.
#[tauri::command]
async fn save_notify_prefs(prefs: NotifyFile) -> IpcResult<()> {
    prefs.save().map_err(|e| {
        tracing::warn!(err = %e, "save_notify_prefs failed");
        IpcError::Unknown {
            trace_id: format!("save_notify_prefs:{e}"),
        }
    })
}

/// 把插件的 `PermissionState` 显式映射为 TS 契约的固定字符串字面量,
/// 不依赖 `Debug`/`Display` 格式(否则 `Prompt` 会序列化成契约外的 "prompt").
/// wire 值严格落在 TS `NotifyPermissionState = "granted" | "denied" | "default"`.
fn permission_state_str(s: tauri_plugin_notification::PermissionState) -> &'static str {
    use tauri_plugin_notification::PermissionState;
    match s {
        PermissionState::Granted => "granted",
        PermissionState::Denied => "denied",
        // Prompt / PromptWithRationale = "尚未授权", 对应契约的 "default"
        PermissionState::Prompt | PermissionState::PromptWithRationale => "default",
    }
}

/// 查询系统通知权限. macOS 首次需要授权.
/// 返回 "granted" | "denied" | "default" (未问过).
#[tauri::command]
async fn notify_permission(app: AppHandle) -> IpcResult<String> {
    use tauri_plugin_notification::NotificationExt;
    app.notification()
        .permission_state()
        .map(|s| permission_state_str(s).to_string())
        .map_err(|e| {
            tracing::warn!(err = %e, "notify_permission failed");
            IpcError::Unknown {
                trace_id: format!("notify_permission:{e}"),
            }
        })
}

/// 主动请求通知权限. macOS 第一次会弹系统授权对话框.
#[tauri::command]
async fn request_notify_permission(app: AppHandle) -> IpcResult<String> {
    use tauri_plugin_notification::NotificationExt;
    app.notification()
        .request_permission()
        .map(|s| permission_state_str(s).to_string())
        .map_err(|e| {
            tracing::warn!(err = %e, "request_notify_permission failed");
            IpcError::Unknown {
                trace_id: format!("request_notify_permission:{e}"),
            }
        })
}

/// 声音预览/播放数据. bytes 为 base64 (避免 Vec<u8> 走 JSON 数字数组的 6× 膨胀).
#[derive(Debug, Clone, serde::Serialize)]
struct NotifySoundData {
    /// 音频 MIME (audio/aiff, audio/wav, audio/mpeg, audio/ogg, audio/mp4, ...).
    mime: String,
    /// 原始音频字节的 base64.
    base64: String,
}

/// 判断声音字段是否为本地文件路径(而非 macOS 内建声音名).
/// 绝对路径 / `~/...` 前缀视为路径; 其它(空 / "default" / "Glass" 等)视为系统名.
fn sound_is_file_path(s: &str) -> bool {
    let s = s.trim();
    s.starts_with('/') || s.starts_with("~/")
}

/// 把 `~/...` 展开到 $HOME, 其它路径原样.
fn expand_tilde(s: &str) -> std::path::PathBuf {
    if let Some(rest) = s.strip_prefix("~/") {
        if let Ok(home) = std::env::var("HOME") {
            return std::path::PathBuf::from(home).join(rest);
        }
    }
    std::path::PathBuf::from(s)
}

/// 把 NotifyPrefs.sound 字段解析为可读的本地音频文件.
/// 返回 None 表示走系统默认(空字符串 / "default" / 没匹配到任何声音文件).
///
/// 查找顺序:
///   1. 绝对路径 / `~/` → 用户自选文件
///   2. VibeTerm 自带 (resource_dir/sounds/<name>.mp3) — 跨平台一致
///   3. macOS 系统声音 (/System/Library/Sounds/<name>.aiff)
///   4. macOS 用户声音 (~/Library/Sounds/<name>.aiff)
fn resolve_sound_to_path(app: &AppHandle, sound: &str) -> Option<std::path::PathBuf> {
    let s = sound.trim();
    if s.is_empty() || s.eq_ignore_ascii_case("default") {
        return None;
    }
    if sound_is_file_path(s) {
        let p = expand_tilde(s);
        if !p.is_file() {
            return None;
        }
        // 收敛任意文件读取:仅允许音频扩展名,且 canonicalize 后必须落在 $HOME 内.
        // (前端可控字符串 → 此前可读 /etc/passwd、~/.ssh/id_rsa 等任意 <10MB 文件)
        if mime_for_ext(&p) == "application/octet-stream" {
            tracing::warn!(path = %p.display(), "notify sound rejected: non-audio extension");
            return None;
        }
        let canon = match std::fs::canonicalize(&p) {
            Ok(c) => c,
            Err(e) => {
                tracing::warn!(path = %p.display(), err = %e, "notify sound canonicalize failed");
                return None;
            }
        };
        // HOME 也 canonicalize,避免 /var → /private/var 等符号链接导致合法路径被误拒.
        let home_ok = std::env::var("HOME").ok().and_then(|h| {
            let home_canon =
                std::fs::canonicalize(&h).unwrap_or_else(|_| std::path::PathBuf::from(h));
            canon.starts_with(&home_canon).then_some(())
        });
        if home_ok.is_some() {
            return Some(canon);
        }
        tracing::warn!(path = %canon.display(), "notify sound rejected: outside $HOME");
        return None;
    }
    // 自带:打包资源里 resources/sounds/<id>.mp3
    if let Ok(res_dir) = app.path().resource_dir() {
        let bundled = res_dir.join("resources/sounds").join(format!("{s}.mp3"));
        if bundled.is_file() {
            return Some(bundled);
        }
    }
    // macOS 系统名 fallback
    #[cfg(target_os = "macos")]
    {
        let sys = std::path::PathBuf::from("/System/Library/Sounds").join(format!("{s}.aiff"));
        if sys.is_file() {
            return Some(sys);
        }
        if let Ok(home) = std::env::var("HOME") {
            let user = std::path::PathBuf::from(home)
                .join("Library/Sounds")
                .join(format!("{s}.aiff"));
            if user.is_file() {
                return Some(user);
            }
        }
    }
    None
}

fn mime_for_ext(p: &std::path::Path) -> &'static str {
    match p
        .extension()
        .and_then(|e| e.to_str())
        .map(|s| s.to_lowercase())
    {
        Some(ref e) if e == "aiff" || e == "aif" || e == "aifc" => "audio/aiff",
        Some(ref e) if e == "wav" => "audio/wav",
        Some(ref e) if e == "mp3" => "audio/mpeg",
        Some(ref e) if e == "ogg" || e == "oga" => "audio/ogg",
        Some(ref e) if e == "m4a" || e == "mp4" || e == "aac" => "audio/mp4",
        Some(ref e) if e == "flac" => "audio/flac",
        _ => "application/octet-stream",
    }
}

/// 大文件保护. 音频通常 < 1MB; 上限 10MB 防意外塞超长 WAV.
const NOTIFY_SOUND_MAX_BYTES: u64 = 10 * 1024 * 1024;

/// 把声音字段解析后读字节回传, 前端用 <audio> 播放.
/// 既给设置面板"试听"按钮用, 也给"自定义文件路径"通知触发时实时播放用.
#[tauri::command]
async fn preview_notify_sound(app: AppHandle, sound: String) -> IpcResult<NotifySoundData> {
    use base64::Engine;
    let path = resolve_sound_to_path(&app, &sound).ok_or_else(|| IpcError::NotFound {
        resource: "notify_sound".into(),
        id: sound.clone(),
    })?;
    let meta = std::fs::metadata(&path).map_err(|e| IpcError::Unknown {
        trace_id: format!("notify_sound_meta:{e}"),
    })?;
    if meta.len() > NOTIFY_SOUND_MAX_BYTES {
        return Err(IpcError::PermissionDenied {
            reason: format!(
                "audio file too large ({} bytes, max {})",
                meta.len(),
                NOTIFY_SOUND_MAX_BYTES
            ),
        });
    }
    let bytes = std::fs::read(&path).map_err(|e| IpcError::Unknown {
        trace_id: format!("notify_sound_read:{e}"),
    })?;
    Ok(NotifySoundData {
        mime: mime_for_ext(&path).to_string(),
        base64: base64::engine::general_purpose::STANDARD.encode(&bytes),
    })
}

/// 自带声音库:从 bundle resources/sounds/sounds.json 读清单.
/// 前端下拉用 (按 category 分组). 找不到 manifest 返回空数组 (前端降级).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct BuiltinSound {
    id: String,
    name: String,
    category: String,
    #[allow(dead_code)]
    file: String,
}

#[derive(Debug, serde::Deserialize)]
struct SoundsManifest {
    sounds: Vec<BuiltinSound>,
}

#[tauri::command]
async fn list_builtin_sounds(app: AppHandle) -> IpcResult<Vec<BuiltinSound>> {
    let Ok(res_dir) = app.path().resource_dir() else {
        return Ok(vec![]);
    };
    let path = res_dir.join("resources/sounds/sounds.json");
    let Ok(s) = std::fs::read_to_string(&path) else {
        tracing::debug!(?path, "sounds.json not found");
        return Ok(vec![]);
    };
    let manifest: SoundsManifest = match serde_json::from_str(&s) {
        Ok(m) => m,
        Err(e) => {
            tracing::warn!(err = %e, "sounds.json parse failed");
            return Ok(vec![]);
        }
    };
    Ok(manifest.sounds)
}

#[tauri::command]
async fn reorder_tasks(
    order: Vec<vibeterm_ipc::TaskId>,
    state: tauri::State<'_, AppState>,
    app: AppHandle,
) -> IpcResult<()> {
    state.tasks.reorder(order).map_err(map_task_err)?;
    emit_tasks_changed(&app, &state.tasks);
    Ok(())
}

#[tauri::command]
async fn set_active_task(
    id: vibeterm_ipc::TaskId,
    state: tauri::State<'_, AppState>,
    app: AppHandle,
) -> IpcResult<()> {
    state.tasks.set_active_main(id).map_err(map_task_err)?;
    let _ = app.emit("active_task_changed", id);
    // 切换当前任务 → 重算各 task 聚合 status(切出的完成任务变 Done、切入的变 Idle)+ 刷新角标。
    emit_tasks_changed(&app, &state.tasks);
    refresh_dock_badge(&app, &state.tasks);
    Ok(())
}

// 写回任务的分屏布局,任意窗口可调,emit tasks_changed 同步另一窗
#[tauri::command]
async fn set_task_split_tree(
    id: vibeterm_ipc::TaskId,
    tree: vibeterm_ipc::SplitNode,
    state: tauri::State<'_, AppState>,
    app: AppHandle,
) -> IpcResult<()> {
    state.tasks.set_split_tree(id, tree).map_err(map_task_err)?;
    emit_tasks_changed(&app, &state.tasks);
    Ok(())
}

fn map_task_err(e: vibeterm_core::TaskError) -> IpcError {
    use vibeterm_core::TaskError::*;
    match e {
        NotFound(id) => IpcError::NotFound {
            resource: "task".into(),
            id: id.to_string(),
        },
        other => IpcError::Unknown {
            trace_id: format!("task:{other}"),
        },
    }
}

// ============================
// IPC commands — Git Worktree
// ============================
//
// 命名一律 git_* 前缀,前端 ipc/index.ts 一一对应。
// 错误统一映射 IpcError::Unknown { trace_id: "git:<detail>" }。

fn map_git_err(e: vibeterm_git::GitError) -> IpcError {
    IpcError::Unknown {
        trace_id: format!("git:{e}"),
    }
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// 把 vibeterm-git 解析出来的 entry + 实时 status 合成 IPC 层 WorktreeRef。
/// `branch` 字段:用 status.branch(短名)优于 entry.branch(refs/heads/ 前缀)。
async fn build_worktree_ref(
    repo_path: &std::path::Path,
    worktree_path: &std::path::Path,
) -> Result<vibeterm_ipc::WorktreeRef, vibeterm_git::GitError> {
    let st = vibeterm_git::worktree_status(worktree_path).await?;
    Ok(vibeterm_ipc::WorktreeRef {
        repo_path: repo_path.to_string_lossy().into_owned(),
        worktree_path: worktree_path.to_string_lossy().into_owned(),
        branch: st.branch.clone(),
        head: st.head,
        is_dirty: st.is_dirty,
        ahead: st.ahead,
        behind: st.behind,
        status_updated_at: now_ms(),
    })
}

#[tauri::command]
async fn git_is_repo(path: String) -> IpcResult<bool> {
    vibeterm_git::is_git_repo(std::path::Path::new(&path))
        .await
        .map_err(map_git_err)
}

#[tauri::command]
async fn git_repo_root(path: String) -> IpcResult<String> {
    vibeterm_git::repo_common_root(std::path::Path::new(&path))
        .await
        .map(|p| p.to_string_lossy().into_owned())
        .map_err(map_git_err)
}

#[tauri::command]
async fn git_list_worktrees(repo_path: String) -> IpcResult<Vec<vibeterm_git::WorktreeEntry>> {
    vibeterm_git::list_worktrees(std::path::Path::new(&repo_path))
        .await
        .map_err(map_git_err)
}

#[tauri::command]
async fn git_list_branches(repo_path: String) -> IpcResult<Vec<String>> {
    vibeterm_git::list_local_branches(std::path::Path::new(&repo_path))
        .await
        .map_err(map_git_err)
}

#[tauri::command]
async fn git_add_worktree(
    repo_path: String,
    new_path: String,
    spec: vibeterm_ipc::BranchSpecDto,
) -> IpcResult<vibeterm_ipc::WorktreeRef> {
    let repo = std::path::Path::new(&repo_path);
    let new = std::path::Path::new(&new_path);
    let bs = match spec {
        vibeterm_ipc::BranchSpecDto::Existing { branch } => {
            vibeterm_git::BranchSpec::Existing(branch)
        }
        vibeterm_ipc::BranchSpecDto::NewFromHead { branch } => {
            vibeterm_git::BranchSpec::NewFromHead(branch)
        }
        vibeterm_ipc::BranchSpecDto::NewFromRef {
            branch,
            start_point,
        } => vibeterm_git::BranchSpec::NewFromRef {
            name: branch,
            start_point,
        },
    };
    let _entry = vibeterm_git::add_worktree(repo, new, bs)
        .await
        .map_err(map_git_err)?;
    build_worktree_ref(repo, new).await.map_err(map_git_err)
}

#[tauri::command]
async fn git_remove_worktree(
    repo_path: String,
    worktree_path: String,
    force: bool,
) -> IpcResult<()> {
    vibeterm_git::remove_worktree(
        std::path::Path::new(&repo_path),
        std::path::Path::new(&worktree_path),
        force,
    )
    .await
    .map_err(map_git_err)
}

/// 把指定 worktree 挂到 task 上;同时把 task.cwd 改成 worktree_path。
#[tauri::command]
async fn attach_worktree_to_task(
    task_id: vibeterm_ipc::TaskId,
    worktree: vibeterm_ipc::WorktreeRef,
    state: tauri::State<'_, AppState>,
    app: AppHandle,
) -> IpcResult<TaskDto> {
    state
        .tasks
        .attach_worktree(task_id, worktree)
        .map_err(map_task_err)?;
    emit_tasks_changed(&app, &state.tasks);
    state
        .tasks
        .task_dto(task_id)
        .map_err(map_task_err)?
        .ok_or(IpcError::NotFound {
            resource: "task".into(),
            id: task_id.to_string(),
        })
}

#[tauri::command]
async fn detach_worktree_from_task(
    task_id: vibeterm_ipc::TaskId,
    state: tauri::State<'_, AppState>,
    app: AppHandle,
) -> IpcResult<TaskDto> {
    state.tasks.detach_worktree(task_id).map_err(map_task_err)?;
    emit_tasks_changed(&app, &state.tasks);
    state
        .tasks
        .task_dto(task_id)
        .map_err(map_task_err)?
        .ok_or(IpcError::NotFound {
            resource: "task".into(),
            id: task_id.to_string(),
        })
}

/// 把持久化的 worktree 路径(来自 tasks.json 反序列化)规范化后返回。
/// 防御性:canonicalize 消除 `../` 等,确保 git status 不在非预期目录执行;
/// 失败(路径不存在/非法)则返回 None,调用方跳过该条目并 warn。
fn validated_worktree_path(raw: &str) -> Option<std::path::PathBuf> {
    match std::fs::canonicalize(raw) {
        Ok(p) => Some(p),
        Err(e) => {
            tracing::warn!(path = raw, err = %e, "worktree path canonicalize failed, skipping");
            None
        }
    }
}

/// 主动刷新某 task 的 worktree 状态(UI 上拉刷新等场景)。
/// 后台另有定时轮询,这里只刷一次。
#[tauri::command]
async fn refresh_worktree_status(
    task_id: vibeterm_ipc::TaskId,
    state: tauri::State<'_, AppState>,
    app: AppHandle,
) -> IpcResult<Option<TaskDto>> {
    let wt = match state.tasks.worktree_of(task_id).map_err(map_task_err)? {
        Some(w) => w,
        None => return Ok(None),
    };
    let wt_path = match validated_worktree_path(&wt.worktree_path) {
        Some(p) => p,
        None => return Ok(None),
    };
    let st = vibeterm_git::worktree_status(&wt_path)
        .await
        .map_err(map_git_err)?;
    state
        .tasks
        .update_worktree_status(
            task_id,
            st.head,
            st.branch,
            st.is_dirty,
            st.ahead,
            st.behind,
            now_ms(),
        )
        .map_err(map_task_err)?;
    emit_tasks_changed(&app, &state.tasks);
    state.tasks.task_dto(task_id).map_err(map_task_err)
}

// ============================
// IPC commands — Theme / Config
// ============================

#[tauri::command]
async fn get_config() -> IpcResult<Config> {
    Config::load().map_err(|e| IpcError::Unknown {
        trace_id: format!("config:{e}"),
    })
}

#[tauri::command]
async fn set_shell_integration(enabled: bool) -> IpcResult<()> {
    let mut cfg = Config::load().map_err(|e| IpcError::Unknown {
        trace_id: format!("config:{e}"),
    })?;
    cfg.shell_integration = enabled;
    cfg.save().map_err(|e| IpcError::Unknown {
        trace_id: format!("save:{e}"),
    })?;
    // 下次 spawn 的终端生效(已开终端不动);无需 emit。
    Ok(())
}

#[tauri::command]
async fn set_active_theme(id: String, app: AppHandle) -> IpcResult<Theme> {
    let mut cfg = Config::load().map_err(|e| IpcError::Unknown {
        trace_id: format!("config:{e}"),
    })?;
    cfg.active_theme = id.clone();
    cfg.save().map_err(|e| IpcError::Unknown {
        trace_id: format!("save:{e}"),
    })?;
    let theme = vibeterm_config::get_theme(&id);
    let _ = app.emit("theme_changed", &theme);
    Ok(theme)
}

#[tauri::command]
async fn list_themes() -> IpcResult<Vec<Theme>> {
    Ok(vibeterm_config::load_all_themes())
}

#[tauri::command]
async fn get_theme(id: String) -> IpcResult<Theme> {
    Ok(vibeterm_config::get_theme(&id))
}

// env.toml 管理
#[tauri::command]
async fn get_env_file() -> IpcResult<EnvFile> {
    EnvFile::load().map_err(|e| IpcError::Unknown {
        trace_id: format!("env_load:{e}"),
    })
}

#[tauri::command]
async fn save_env_file(file: EnvFile, app: AppHandle) -> IpcResult<()> {
    file.save().map_err(|e| IpcError::Unknown {
        trace_id: format!("env_save:{e}"),
    })?;
    let _ = app.emit("env_changed", ());
    Ok(())
}

// keybindings.toml
#[tauri::command]
async fn get_keybindings() -> IpcResult<KeybindingsFile> {
    Ok(KeybindingsFile::load())
}

#[tauri::command]
async fn save_keybindings(file: KeybindingsFile, app: AppHandle) -> IpcResult<()> {
    file.save().map_err(|e| IpcError::Unknown {
        trace_id: format!("kb_save:{e}"),
    })?;
    let _ = app.emit("keybindings_changed", ());
    Ok(())
}

/// 重置所有快捷键为内置默认值. 删 keybindings.toml, 下次 load 返回 default.
/// 立即对指定 terminal 的 shell pid 做一次 agent 嗅探, 不等 3s 后台轮询.
/// PromptPicker 弹出时调一次, 确保 kind 与"用户当前焦点所在终端"一致.
/// 返回完整诊断信息: 命中的 agent + pid + pgid + 整个 process group cmdlines,
/// 前端 console 直接展示, 不需要后端日志.
#[derive(serde::Serialize)]
struct DetectAgentResult {
    agent_kind: Option<String>,
    shell_pid: Option<u32>,
    pgid: Option<u32>,
    cmdlines: Vec<String>,
    note: String,
}

#[tauri::command]
async fn detect_agent_for_terminal(
    terminal_id: TerminalId,
    state: tauri::State<'_, AppState>,
) -> IpcResult<DetectAgentResult> {
    let result = match state.terminals.pid_of(terminal_id) {
        Some(pid) => {
            let (kind, diag) = vibeterm_status::detect_agent_with_diagnostics(pid);
            tracing::info!(
                terminal_id, pid, pgid = ?diag.pgid, agent_kind = ?kind,
                cmdlines = ?diag.cmdlines,
                "detect_agent_for_terminal"
            );
            DetectAgentResult {
                agent_kind: kind.map(|k| k.as_str().to_string()),
                shell_pid: Some(diag.shell_pid),
                pgid: diag.pgid,
                cmdlines: diag.cmdlines,
                note: diag.note,
            }
        }
        None => DetectAgentResult {
            agent_kind: None,
            shell_pid: None,
            pgid: None,
            cmdlines: vec![],
            note: format!("terminal {terminal_id}: pid_of returned None"),
        },
    };
    // 诊断已通过上面的 tracing::info! 输出. 额外写固定 /tmp 文件方便排查,
    // 但 cmdlines 可能含敏感命令行参数 + /tmp 世界可读, 故仅限 debug 构建.
    #[cfg(debug_assertions)]
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open("/tmp/vibeterm-detect.log")
    {
        use std::io::Write;
        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let _ = writeln!(
            f,
            "ts={} terminal_id={} agent={:?} pid={:?} pgid={:?} cmdlines={:?} note={}",
            ts,
            terminal_id,
            result.agent_kind,
            result.shell_pid,
            result.pgid,
            result.cmdlines,
            result.note,
        );
    }
    Ok(result)
}

/// 重置所有 prompts 为内置默认值. 删 prompts.toml, 下次 load 返回 default.
#[tauri::command]
async fn reset_prompts(app: AppHandle) -> IpcResult<PromptsFile> {
    let p = vibeterm_config::prompts_toml_path().map_err(|e| IpcError::Unknown {
        trace_id: format!("prompts_path:{e}"),
    })?;
    if p.exists() {
        std::fs::remove_file(&p).map_err(|e| IpcError::Unknown {
            trace_id: format!("prompts_rm:{e}"),
        })?;
    }
    let _ = app.emit("prompts_changed", ());
    Ok(PromptsFile::load())
}

#[tauri::command]
async fn reset_keybindings(app: AppHandle) -> IpcResult<KeybindingsFile> {
    let p = vibeterm_config::keybindings_toml_path().map_err(|e| IpcError::Unknown {
        trace_id: format!("kb_path:{e}"),
    })?;
    if p.exists() {
        std::fs::remove_file(&p).map_err(|e| IpcError::Unknown {
            trace_id: format!("kb_rm:{e}"),
        })?;
    }
    let _ = app.emit("keybindings_changed", ());
    Ok(KeybindingsFile::load())
}

// prompts.toml
#[tauri::command]
async fn get_prompts() -> IpcResult<PromptsFile> {
    Ok(PromptsFile::load())
}

#[tauri::command]
async fn save_prompts(file: PromptsFile, app: AppHandle) -> IpcResult<()> {
    file.save().map_err(|e| IpcError::Unknown {
        trace_id: format!("prompts_save:{e}"),
    })?;
    let _ = app.emit("prompts_changed", ());
    Ok(())
}

// ---- Custom Actions ----

/// 启动时拿上次激活的 task id
#[tauri::command]
async fn get_active_task(
    state: tauri::State<'_, AppState>,
) -> IpcResult<Option<vibeterm_ipc::TaskId>> {
    Ok(state.tasks.active_main())
}

#[tauri::command]
async fn get_actions() -> IpcResult<ActionsFile> {
    Ok(ActionsFile::load())
}

#[tauri::command]
async fn save_actions(file: ActionsFile, app: AppHandle) -> IpcResult<()> {
    file.save().map_err(|e| IpcError::Unknown {
        trace_id: format!("actions_save:{e}"),
    })?;
    let _ = app.emit("actions_changed", ());
    Ok(())
}

/// 执行一个 action。
///
/// 模式:
///   - current_terminal: 写到指定 terminal_id(必传),自动追加 \n
///   - new_task: 创建新 task,命名 "<title>",cwd=$HOME,后台 spawn 由前端触发
///     (本命令只创建 task 并写回 command;前端拿 task_id 后 spawn + write)
///   - insert: 写到指定 terminal_id,不加 \n
///
/// 返回:
///   - current_terminal / insert → ExecuteActionResult::WrittenTo { terminal_id }
///   - new_task → ExecuteActionResult::NewTask { task_id, command }
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum ExecuteActionResult {
    WrittenTo {
        terminal_id: vibeterm_ipc::TerminalId,
    },
    NewTask {
        task_id: vibeterm_ipc::TaskId,
        command: String,
    },
}

#[tauri::command]
async fn execute_action(
    action_id: String,
    terminal_id: Option<vibeterm_ipc::TerminalId>,
    state: tauri::State<'_, AppState>,
    app: AppHandle,
) -> IpcResult<ExecuteActionResult> {
    let actions = ActionsFile::load();
    let action = actions
        .actions
        .into_iter()
        .find(|a| a.id == action_id)
        .ok_or_else(|| IpcError::NotFound {
            resource: "action".into(),
            id: action_id.clone(),
        })?;

    match action.mode {
        ActionMode::CurrentTerminal | ActionMode::Insert => {
            let tid = terminal_id.ok_or(IpcError::PermissionDenied {
                reason: "current_terminal/insert mode requires terminal_id".into(),
            })?;
            let mut payload = action.command.into_bytes();
            if matches!(action.mode, ActionMode::CurrentTerminal) {
                payload.push(b'\n');
            }
            state
                .terminals
                .write(tid, &payload)
                .map_err(|e| IpcError::Unknown {
                    trace_id: format!("write:{e}"),
                })?;
            Ok(ExecuteActionResult::WrittenTo { terminal_id: tid })
        }
        ActionMode::NewTask => {
            let id = state
                .tasks
                .create(action.title.clone(), None, None)
                .map_err(map_task_err)?;
            emit_tasks_changed(&app, &state.tasks);
            Ok(ExecuteActionResult::NewTask {
                task_id: id,
                command: action.command,
            })
        }
    }
}

// ============================
// IPC commands — Window
// ============================

#[tauri::command]
async fn open_floating(
    task_id: vibeterm_ipc::TaskId,
    app: AppHandle,
    state: tauri::State<'_, AppState>,
) -> IpcResult<String> {
    let label = format!("floating-{}", chrono_label());
    let builder = WebviewWindowBuilder::new(
        &app,
        &label,
        WebviewUrl::App(format!("floating.html?taskId={task_id}").into()),
    )
    .title(format!("VibeTerm — Task {task_id}"))
    .inner_size(800.0, 600.0)
    .background_color(tauri::window::Color(0x11, 0x11, 0x11, 0xff));
    #[cfg(target_os = "macos")]
    let builder = builder
        .title_bar_style(tauri::TitleBarStyle::Overlay)
        .hidden_title(true)
        .transparent(true);
    #[cfg(not(target_os = "macos"))]
    let builder = builder.decorations(false);
    let float_win = builder.build().map_err(|e| IpcError::Unknown {
        trace_id: format!("window:{e}"),
    })?;
    #[cfg(target_os = "macos")]
    apply_macos_vibrancy(&float_win);
    #[cfg(not(target_os = "macos"))]
    let _ = float_win;
    let _ = state
        .tasks
        .set_location(task_id, TaskLocation::Floating(label.clone()));
    emit_tasks_changed(&app, &state.tasks);
    let _ = app.emit(
        "floating_opened",
        serde_json::json!({"label": label, "task_id": task_id}),
    );
    // rebuild menu(windows submenu 含动态浮窗列表)
    #[cfg(target_os = "macos")]
    if let Ok(menu) = build_menu(&app, current_menu_lang(&state)) {
        let _ = app.set_menu(menu);
    }
    Ok(label)
}

#[tauri::command]
async fn close_floating(
    label: String,
    app: AppHandle,
    state: tauri::State<'_, AppState>,
) -> IpcResult<()> {
    if let Some(w) = app.get_webview_window(&label) {
        let _ = w.close();
    }
    // 找到对应 task 并改回 nowhere(主工作区不主动激活)
    if let Ok(tasks) = state.tasks.list() {
        for t in tasks {
            if let TaskLocation::Floating(ref l) = t.location {
                if l == &label {
                    let _ = state.tasks.set_location(t.id, TaskLocation::Nowhere);
                }
            }
        }
    }
    emit_tasks_changed(&app, &state.tasks);
    let _ = app.emit("floating_closed", &label);
    // rebuild menu
    #[cfg(target_os = "macos")]
    if let Ok(menu) = build_menu(&app, current_menu_lang(&state)) {
        let _ = app.set_menu(menu);
    }
    Ok(())
}

// 前端 setLang() 触发 — 切换顶栏菜单语言并重建。非 macOS 上是 noop。
#[tauri::command]
async fn set_menu_lang(
    lang: String,
    app: AppHandle,
    state: tauri::State<'_, AppState>,
) -> IpcResult<()> {
    let l = MenuLang::from_tag(&lang);
    if let Ok(mut g) = state.menu_lang.lock() {
        *g = l;
    }
    #[cfg(target_os = "macos")]
    {
        let _ = &app;
        if let Ok(menu) = build_menu(&app, l) {
            let _ = app.set_menu(menu);
        }
    }
    #[cfg(not(target_os = "macos"))]
    let _ = (app, l);
    Ok(())
}

#[tauri::command]
async fn focus_window(label: String, app: AppHandle) -> IpcResult<()> {
    if let Some(w) = app.get_webview_window(&label) {
        let _ = w.show();
        let _ = w.set_focus();
    }
    Ok(())
}

// 浮窗里按 Cmd+K 等全局快捷键时 → 通知主窗口 + 拉前台 + 触发该 action
// (浮窗内全局快捷键自动拉主窗口前台执行)
#[tauri::command]
async fn invoke_global_action(action: String, app: AppHandle) -> IpcResult<()> {
    if let Some(main) = app.get_webview_window("main") {
        let _ = main.show();
        let _ = main.set_focus();
        let _ = app.emit_to(
            tauri::EventTarget::WebviewWindow {
                label: "main".into(),
            },
            "global_action",
            action,
        );
    }
    Ok(())
}

// macOS 完整菜单栏;加浮窗动态列表
// PredefinedMenuItem 的 NSResponder 行为(Cut/Copy/Paste/Undo/Hide/Quit…)
// 由系统自动 dispatch;我们只通过 Some(text) 覆盖 label,跟随应用 lang。
#[cfg(target_os = "macos")]
fn build_menu(app: &AppHandle, lang: MenuLang) -> tauri::Result<Menu<tauri::Wry>> {
    let lbl = menu_labels(lang);
    // 收集当前浮窗 labels — 用于「窗口」子菜单动态项
    let floating_labels: Vec<String> = app
        .webview_windows()
        .keys()
        .filter_map(|label| {
            if label.starts_with("floating") {
                Some(label.clone())
            } else {
                None
            }
        })
        .collect();
    let app_submenu = Submenu::with_items(
        app,
        "VibeTerm",
        true,
        &[
            &PredefinedMenuItem::about(app, Some(lbl.about_app), None)?,
            &MenuItem::with_id(app, "check_update", lbl.check_update, true, None::<&str>)?,
            &PredefinedMenuItem::separator(app)?,
            &MenuItem::with_id(app, "open_settings", lbl.settings, true, Some("Cmd+,"))?,
            &PredefinedMenuItem::separator(app)?,
            &PredefinedMenuItem::services(app, Some(lbl.services))?,
            &PredefinedMenuItem::separator(app)?,
            &PredefinedMenuItem::hide(app, Some(lbl.hide))?,
            &PredefinedMenuItem::hide_others(app, Some(lbl.hide_others))?,
            &PredefinedMenuItem::show_all(app, Some(lbl.show_all))?,
            &PredefinedMenuItem::separator(app)?,
            &PredefinedMenuItem::quit(app, Some(lbl.quit))?,
        ],
    )?;

    let file_submenu = Submenu::with_items(
        app,
        lbl.file,
        true,
        &[
            &MenuItem::with_id(app, "new_task", lbl.new_task, true, Some("Cmd+N"))?,
            &MenuItem::with_id(app, "new_terminal", lbl.new_terminal, true, Some("Cmd+T"))?,
            &PredefinedMenuItem::separator(app)?,
            &MenuItem::with_id(
                app,
                "open_claude_md",
                lbl.open_claude_md,
                true,
                None::<&str>,
            )?,
            &MenuItem::with_id(
                app,
                "open_config_dir",
                lbl.open_config_dir,
                true,
                None::<&str>,
            )?,
            &PredefinedMenuItem::separator(app)?,
            &MenuItem::with_id(
                app,
                "close_terminal",
                lbl.close_terminal,
                true,
                Some("Cmd+W"),
            )?,
        ],
    )?;

    let edit_submenu = Submenu::with_items(
        app,
        lbl.edit,
        true,
        &[
            &PredefinedMenuItem::undo(app, Some(lbl.undo))?,
            &PredefinedMenuItem::redo(app, Some(lbl.redo))?,
            &PredefinedMenuItem::separator(app)?,
            &PredefinedMenuItem::cut(app, Some(lbl.cut))?,
            &PredefinedMenuItem::copy(app, Some(lbl.copy))?,
            &PredefinedMenuItem::paste(app, Some(lbl.paste))?,
            &PredefinedMenuItem::select_all(app, Some(lbl.select_all))?,
            &PredefinedMenuItem::separator(app)?,
            &MenuItem::with_id(
                app,
                "find_in_terminal",
                lbl.find_in_terminal,
                true,
                Some("Cmd+F"),
            )?,
        ],
    )?;

    let view_submenu = Submenu::with_items(
        app,
        lbl.view,
        true,
        &[
            &MenuItem::with_id(
                app,
                "command_palette",
                lbl.command_palette,
                true,
                Some("Cmd+K"),
            )?,
            &PredefinedMenuItem::separator(app)?,
            &MenuItem::with_id(app, "next_task", lbl.next_task, true, Some("Cmd+Shift+]"))?,
            &MenuItem::with_id(app, "prev_task", lbl.prev_task, true, Some("Cmd+Shift+["))?,
            &PredefinedMenuItem::separator(app)?,
            &MenuItem::with_id(
                app,
                "split_horizontal",
                lbl.split_horizontal,
                true,
                Some("Cmd+D"),
            )?,
            &MenuItem::with_id(
                app,
                "split_vertical",
                lbl.split_vertical,
                true,
                Some("Cmd+Shift+D"),
            )?,
            &PredefinedMenuItem::separator(app)?,
            &MenuItem::with_id(app, "switch_theme", lbl.switch_theme, true, None::<&str>)?,
        ],
    )?;

    // 浮窗列表动态注入(每打开 / 关闭一个浮窗就 rebuild 整套菜单 + set_menu)
    let mut window_items: Vec<Box<dyn tauri::menu::IsMenuItem<tauri::Wry>>> = vec![
        Box::new(PredefinedMenuItem::minimize(app, Some(lbl.minimize))?),
        Box::new(PredefinedMenuItem::maximize(app, Some(lbl.maximize))?),
        Box::new(PredefinedMenuItem::separator(app)?),
        Box::new(MenuItem::with_id(
            app,
            "focus_main",
            lbl.focus_main,
            true,
            None::<&str>,
        )?),
    ];
    if !floating_labels.is_empty() {
        window_items.push(Box::new(PredefinedMenuItem::separator(app)?));
        for label in &floating_labels {
            let item = MenuItem::with_id(
                app,
                format!("focus_floating:{label}"),
                format!("{} — {label}", lbl.floating_prefix),
                true,
                None::<&str>,
            )?;
            window_items.push(Box::new(item));
        }
    }
    let window_items_refs: Vec<&dyn tauri::menu::IsMenuItem<tauri::Wry>> =
        window_items.iter().map(|b| b.as_ref()).collect();
    let window_submenu = Submenu::with_items(app, lbl.window, true, &window_items_refs)?;

    let help_submenu = Submenu::with_items(
        app,
        lbl.help,
        true,
        &[
            &MenuItem::with_id(
                app,
                "open_shortcuts",
                lbl.open_shortcuts,
                true,
                None::<&str>,
            )?,
            &MenuItem::with_id(app, "open_github", lbl.open_github, true, None::<&str>)?,
            &MenuItem::with_id(app, "open_issues", lbl.open_issues, true, None::<&str>)?,
            &MenuItem::with_id(app, "open_privacy", lbl.open_privacy, true, None::<&str>)?,
        ],
    )?;

    Menu::with_items(
        app,
        &[
            &app_submenu,
            &file_submenu,
            &edit_submenu,
            &view_submenu,
            &window_submenu,
            &help_submenu,
        ],
    )
}

#[cfg(target_os = "macos")]
fn current_menu_lang(state: &AppState) -> MenuLang {
    state
        .menu_lang
        .lock()
        .ok()
        .map(|g| *g)
        .unwrap_or(MenuLang::En)
}

/// 判断是否为受信任的本地 http URL(精确 host, 防 `http://localhost.evil.com` 前缀绕过).
/// `http://localhost` / `http://127.0.0.1` 后必须紧跟 `/`、`:`(端口)或字符串结束.
fn is_trusted_local_http(url: &str) -> bool {
    for host in ["http://localhost", "http://127.0.0.1"] {
        if let Some(rest) = url.strip_prefix(host) {
            if rest.is_empty() || rest.starts_with('/') || rest.starts_with(':') {
                return true;
            }
        }
    }
    false
}

// Open URL via OS;white-list:仅 https:// + http://localhost*
#[cfg(target_os = "macos")]
fn open_url_safe(_app: &AppHandle, url: &str) {
    if url.starts_with("https://") || is_trusted_local_http(url) {
        if let Err(e) = std::process::Command::new("open").arg(url).spawn() {
            tracing::warn!(url, err = %e, "open_url_safe spawn failed");
        }
    } else {
        tracing::warn!(url, "rejected URL not in whitelist");
    }
}

fn chrono_label() -> String {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis().to_string())
        .unwrap_or_else(|_| "0".into())
}

// ---- shell defaults ----
#[cfg(target_os = "windows")]
fn default_shell() -> &'static str {
    if which::which("pwsh.exe").is_ok() {
        "pwsh.exe"
    } else if which::which("powershell.exe").is_ok() {
        "powershell.exe"
    } else {
        "cmd.exe"
    }
}

#[cfg(not(target_os = "windows"))]
fn default_shell() -> &'static str {
    "/bin/zsh"
}

// ============================
// IPC commands — AI CLI 检测
// ============================
#[derive(serde::Serialize, Clone)]
struct CliStatus {
    name: String,
    installed: bool,
    path: Option<String>,
}

#[tauri::command]
/// 从 login shell 读完整 PATH —— macOS GUI app(Dock/Launchpad 启动)的进程 PATH
/// 不含 ~/.zshrc/.zprofile 里加的目录(homebrew / npm global / nvm 等), 直接 which 会
/// 漏报 "未安装"。读 login shell 的 PATH 修正, 用唯一标记提取避免 rc 其它输出干扰。
fn login_shell_path() -> Option<String> {
    let shell = std::env::var("SHELL").ok()?;
    let out = std::process::Command::new(&shell)
        .args(["-lic", "echo __VTPATH__$PATH"])
        .output()
        .ok()?;
    let s = String::from_utf8_lossy(&out.stdout);
    let p = s
        .lines()
        .find_map(|l| l.trim().strip_prefix("__VTPATH__"))?;
    (!p.is_empty()).then(|| p.to_string())
}

#[tauri::command]
async fn detect_ai_clis() -> IpcResult<Vec<CliStatus>> {
    // 暂时只检测 claude / codex(其它 agent 的状态嗅探不依赖此检测, 需要时再加回)
    let targets = ["claude", "codex"];
    // login shell 的完整 PATH(GUI 启动时进程 PATH 不全);失败回退进程 PATH.
    let path = tokio::task::spawn_blocking(login_shell_path)
        .await
        .ok()
        .flatten();
    let cwd = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("/"));
    Ok(targets
        .iter()
        .map(|name| {
            let found = match &path {
                Some(p) => which::which_in(name, Some(p), &cwd).ok(),
                None => which::which(name).ok(),
            };
            let path = found.map(|p| p.to_string_lossy().into_owned());
            CliStatus {
                name: name.to_string(),
                installed: path.is_some(),
                path,
            }
        })
        .collect())
}

/// Claude usage_cache.json 当前快照 — 前端启动时拉一次, 之后靠
/// `claude_usage_changed` 事件增量更新.
#[tauri::command]
async fn get_claude_usage_cache() -> IpcResult<Option<vibeterm_agent_watch::UsageCache>> {
    Ok(vibeterm_agent_watch::claude::usage_cache::read_once())
}

/// Claude 当前活跃 session (mtime 最新的 jsonl). 前端启动拉一次, 之后靠
/// `claude_session_changed` 事件增量更新.
/// 注意 v2 实现是全局取最新 — v4 会改成按 cwd 过滤.
#[tauri::command]
async fn get_claude_session() -> IpcResult<Option<vibeterm_agent_watch::ClaudeSession>> {
    // 文件 I/O 重 (扫多个 project dir + 最大 64MB jsonl), 走 spawn_blocking 不阻塞 tokio runtime.
    Ok(
        tokio::task::spawn_blocking(vibeterm_agent_watch::claude::project::read_once)
            .await
            .unwrap_or(None),
    )
}

#[tauri::command]
async fn get_codex_session() -> IpcResult<Option<vibeterm_agent_watch::CodexSnapshot>> {
    Ok(vibeterm_agent_watch::codex::session::read_once())
}

/// 按 cwd 查 Claude session — 精确到当前活跃终端的 cwd 而非全局最新.
#[tauri::command]
async fn get_claude_session_by_cwd(
    cwd: String,
) -> IpcResult<Option<vibeterm_agent_watch::ClaudeSession>> {
    // 文件 I/O 重, 走 spawn_blocking 不阻塞 tokio runtime.
    Ok(tokio::task::spawn_blocking(move || {
        vibeterm_agent_watch::claude::project::read_for_cwd(&cwd)
    })
    .await
    .unwrap_or(None))
}

/// 按 cwd 查 Codex session — 同上.
#[tauri::command]
async fn get_codex_session_by_cwd(
    cwd: String,
) -> IpcResult<Option<vibeterm_agent_watch::CodexSnapshot>> {
    // 文件 I/O 重 (扫近 3 天 rollout), 走 spawn_blocking 不阻塞 tokio runtime.
    Ok(tokio::task::spawn_blocking(move || {
        vibeterm_agent_watch::codex::session::read_for_cwd(&cwd)
    })
    .await
    .unwrap_or(None))
}

/// 使用统计面板数据 — 全量扫 `~/.claude/projects` + `~/.codex/sessions`, 聚合最近 `days` 天
/// 的按天 / 按模型 / 按项目 token + cost. 纯只读, 不联网 (离线定价表).
/// 全量扫描可能慢, 走 spawn_blocking 不阻塞 tokio runtime; 失败降级为空统计.
#[tauri::command]
async fn get_usage_stats(days: Option<u32>) -> IpcResult<vibeterm_agent_watch::stats::UsageStats> {
    let d = days.unwrap_or(30);
    Ok(
        tokio::task::spawn_blocking(move || vibeterm_agent_watch::stats::collect(d))
            .await
            .unwrap_or_default(),
    )
}

/// 把统计面板导出的 PNG (base64) 写到用户在前端 save 对话框选定的路径。
/// 仅接受 .png + PNG 魔数校验, 防写入非图片 / 任意垃圾。路径由原生 save 对话框产生 (用户授权)。
#[tauri::command]
async fn save_png_file(path: String, base64_png: String) -> IpcResult<()> {
    use base64::Engine;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(base64_png.as_bytes())
        .map_err(|e| IpcError::Unknown {
            trace_id: format!("save_png_file:decode:{e}"),
        })?;
    // PNG 魔数 (\x89PNG\r\n\x1a\n) 校验, 拒绝非 PNG.
    const PNG_MAGIC: &[u8] = &[0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A];
    if !bytes.starts_with(PNG_MAGIC) || !path.to_ascii_lowercase().ends_with(".png") {
        return Err(IpcError::Unknown {
            trace_id: "save_png_file:not_png".into(),
        });
    }
    std::fs::write(&path, &bytes).map_err(|e| IpcError::Unknown {
        trace_id: format!("save_png_file:write:{e}"),
    })
}

// ===== 手动更新检查(软件版本 / 模型价格)=====
// 🔴 零侵入红线: 仅此处、仅用户点按钮时联网; 纯 GET 两个固定 HTTPS 端点;
// 无任何上传 / 遥测; 绝无后台轮询 / 启动自动检查. 价格 override 只落 VibeTerm config 目录.

// 价格源: LiteLLM 社区维护的公开价格表(权威、含 200k 分档、ccusage 同源).
const PRICING_URL: &str =
    "https://raw.githubusercontent.com/BerriAI/litellm/main/model_prices_and_context_window.json";
const GH_LATEST_RELEASE_URL: &str = "https://api.github.com/repos/fjlmcm/VibeTerm/releases/latest";

/// 同步 GET 一个 HTTPS 文本资源, 带超时 + UA. 仅供手动更新检查用(跑在 spawn_blocking 里).
fn http_get_text(url: &str) -> Result<String, String> {
    let agent = ureq::AgentBuilder::new()
        .timeout_connect(std::time::Duration::from_secs(6))
        .timeout_read(std::time::Duration::from_secs(12))
        .build();
    agent
        .get(url)
        .set("User-Agent", "VibeTerm")
        .call()
        .map_err(|e| format!("request failed: {e}"))?
        .into_string()
        .map_err(|e| format!("read body failed: {e}"))
}

/// 价格表 sanity 校验: 单价有限、非负、且 < 上限, 防脏数据污染成本估算.
fn validate_pricing(t: &vibeterm_agent_watch::claude::pricing::PricingTable) -> Result<(), String> {
    use vibeterm_agent_watch::claude::pricing::Pricing;
    let check = |p: &Pricing, name: &str| -> Result<(), String> {
        for v in [
            p.input_per_mtok,
            p.output_per_mtok,
            p.cache_creation_per_mtok,
            p.cache_read_per_mtok,
        ] {
            if !(v.is_finite() && (0.0..100_000.0).contains(&v)) {
                return Err(format!("{name}: price out of range: {v}"));
            }
        }
        Ok(())
    };
    check(&t.models.opus, "opus")?;
    check(&t.models.sonnet, "sonnet")?;
    check(&t.models.haiku, "haiku")?;
    if t.updated_at.is_empty() {
        return Err("missing updated_at".into());
    }
    Ok(())
}

/// 原子写: 同目录临时文件 + rename(tempfile 已在依赖).
fn atomic_write(path: &std::path::Path, bytes: &[u8]) -> std::io::Result<()> {
    let dir = path.parent().unwrap_or_else(|| std::path::Path::new("."));
    let mut tmp = tempfile::NamedTempFile::new_in(dir)?;
    std::io::Write::write_all(&mut tmp, bytes)?;
    tmp.persist(path).map_err(|e| e.error)?;
    Ok(())
}

/// 当前模型价格来源状态(内置快照 or 已手动更新的覆盖). 给设置·更新页显示.
#[tauri::command]
async fn get_pricing_status() -> IpcResult<vibeterm_agent_watch::claude::pricing::PricingStatus> {
    Ok(vibeterm_agent_watch::claude::pricing::pricing_status())
}

/// 当前日期 YYYY-MM-DD(价格快照时间戳, 本地时区).
fn pricing_today() -> String {
    chrono::Local::now().format("%Y-%m-%d").to_string()
}

/// 从 LiteLLM 价格表适配出 opus/sonnet/haiku 当代价格.
/// LiteLLM 单价是 per-token, 这里 ×1e6 转 per-Mtok 对齐 VibeTerm 的 Pricing.
/// 同家族不同版本价格一致 → 取 anthropic 原生、优先当代(4.x)的代表条目.
fn parse_litellm_pricing(
    body: &str,
) -> Result<vibeterm_agent_watch::claude::pricing::PricingTable, String> {
    use vibeterm_agent_watch::claude::pricing::{Pricing, PricingModels, PricingTable};
    let map: std::collections::HashMap<String, serde_json::Value> =
        serde_json::from_str(body).map_err(|e| format!("json: {e}"))?;
    let pick = |family: &str| -> Result<Pricing, String> {
        let mut chosen: Option<&serde_json::Value> = None;
        let mut chosen_modern = false;
        for (k, v) in &map {
            let kl = k.to_ascii_lowercase();
            if !kl.contains(family) {
                continue;
            }
            if v.get("litellm_provider").and_then(|x| x.as_str()) != Some("anthropic") {
                continue;
            }
            if v.get("input_cost_per_token")
                .and_then(|x| x.as_f64())
                .is_none()
            {
                continue;
            }
            // 优先 4.x 当代(claude-opus-4-x / claude-4-opus), 跳过 3.x 老价.
            let modern = kl.contains("-4") || kl.contains("4-");
            if chosen.is_none() || (modern && !chosen_modern) {
                chosen = Some(v);
                chosen_modern = modern;
            }
        }
        let v = chosen.ok_or_else(|| format!("no anthropic {family} entry"))?;
        let mtok = |key: &str| -> Option<f64> {
            v.get(key).and_then(|x| x.as_f64()).map(|n| n * 1_000_000.0)
        };
        let req = |key: &str| -> Result<f64, String> {
            mtok(key).ok_or_else(|| format!("{family}.{key} missing"))
        };
        Ok(Pricing {
            input_per_mtok: req("input_cost_per_token")?,
            output_per_mtok: req("output_cost_per_token")?,
            cache_creation_per_mtok: mtok("cache_creation_input_token_cost").unwrap_or(0.0),
            cache_read_per_mtok: mtok("cache_read_input_token_cost").unwrap_or(0.0),
            input_above_200k_per_mtok: mtok("input_cost_per_token_above_200k_tokens"),
            output_above_200k_per_mtok: mtok("output_cost_per_token_above_200k_tokens"),
            cache_creation_above_200k_per_mtok: mtok(
                "cache_creation_input_token_cost_above_200k_tokens",
            ),
            cache_read_above_200k_per_mtok: mtok("cache_read_input_token_cost_above_200k_tokens"),
        })
    };
    Ok(PricingTable {
        updated_at: pricing_today(),
        source: "LiteLLM (BerriAI/litellm)".to_string(),
        models: PricingModels {
            opus: pick("opus")?,
            sonnet: pick("sonnet")?,
            haiku: pick("haiku")?,
        },
    })
}

/// 手动更新模型价格: GET LiteLLM 价格表 → 适配 opus/sonnet/haiku → 校验 → 原子写 config → 注入覆盖.
/// 仅用户在设置·更新页点击时触发. 失败不影响内置快照.
#[tauri::command]
async fn update_model_pricing() -> IpcResult<vibeterm_agent_watch::claude::pricing::PricingStatus> {
    use vibeterm_agent_watch::claude::pricing::{pricing_status, set_pricing_override};
    let body = tokio::task::spawn_blocking(|| http_get_text(PRICING_URL))
        .await
        .map_err(|e| IpcError::Unknown {
            trace_id: format!("update_model_pricing:join:{e}"),
        })?
        .map_err(|e| IpcError::Unknown {
            trace_id: format!("update_model_pricing:net:{e}"),
        })?;
    let table = parse_litellm_pricing(&body).map_err(|e| IpcError::Unknown {
        trace_id: format!("update_model_pricing:adapt:{e}"),
    })?;
    validate_pricing(&table).map_err(|e| IpcError::Unknown {
        trace_id: format!("update_model_pricing:invalid:{e}"),
    })?;
    let path = vibeterm_config::pricing_json_path().map_err(|e| IpcError::Unknown {
        trace_id: format!("update_model_pricing:path:{e}"),
    })?;
    let pretty = serde_json::to_string_pretty(&table).map_err(|e| IpcError::Unknown {
        trace_id: format!("update_model_pricing:ser:{e}"),
    })?;
    atomic_write(&path, pretty.as_bytes()).map_err(|e| IpcError::Unknown {
        trace_id: format!("update_model_pricing:write:{e}"),
    })?;
    set_pricing_override(table);
    Ok(pricing_status())
}

/// 还原内置默认价格: 删 override 文件 + 清缓存.
#[tauri::command]
async fn reset_model_pricing() -> IpcResult<vibeterm_agent_watch::claude::pricing::PricingStatus> {
    if let Ok(path) = vibeterm_config::pricing_json_path() {
        let _ = std::fs::remove_file(&path);
    }
    vibeterm_agent_watch::claude::pricing::clear_pricing_override();
    Ok(vibeterm_agent_watch::claude::pricing::pricing_status())
}

/// 软件版本检查结果(仅展示 + 给下载链接, 不下载安装).
#[derive(serde::Serialize)]
struct AppUpdateInfo {
    current: String,
    latest: Option<String>,
    has_update: bool,
    release_url: Option<String>,
    notes: Option<String>,
    published_at: Option<String>,
}

/// release 拉取结果: 区分有 release / 仓库尚无 release(404)/ 网络错误.
enum ReleaseFetch {
    Body(String),
    NoRelease,
    Err(String),
}

/// GET GitHub latest release, 把 404(尚无任何 release)与真正的网络错误分开.
fn http_get_release(url: &str) -> ReleaseFetch {
    let agent = ureq::AgentBuilder::new()
        .timeout_connect(std::time::Duration::from_secs(6))
        .timeout_read(std::time::Duration::from_secs(12))
        .build();
    match agent.get(url).set("User-Agent", "VibeTerm").call() {
        Ok(r) => match r.into_string() {
            Ok(s) => ReleaseFetch::Body(s),
            Err(e) => ReleaseFetch::Err(format!("read body: {e}")),
        },
        Err(ureq::Error::Status(404, _)) => ReleaseFetch::NoRelease,
        Err(e) => ReleaseFetch::Err(format!("{e}")),
    }
}

/// 手动检查软件更新: GET GitHub latest release, 比较版本. 仅显示 + 给 release 链接.
/// 仓库尚无任何 release 时(404)视为"已是最新", 不报错.
#[tauri::command]
async fn check_app_update(app: AppHandle) -> IpcResult<AppUpdateInfo> {
    let current = app.package_info().version.to_string();
    let body = match tokio::task::spawn_blocking(|| http_get_release(GH_LATEST_RELEASE_URL))
        .await
        .map_err(|e| IpcError::Unknown {
            trace_id: format!("check_app_update:join:{e}"),
        })? {
        ReleaseFetch::Body(b) => b,
        ReleaseFetch::NoRelease => {
            return Ok(AppUpdateInfo {
                current,
                latest: None,
                has_update: false,
                release_url: None,
                notes: None,
                published_at: None,
            });
        }
        ReleaseFetch::Err(e) => {
            return Err(IpcError::Unknown {
                trace_id: format!("check_app_update:net:{e}"),
            });
        }
    };
    #[derive(serde::Deserialize)]
    struct GhRelease {
        tag_name: String,
        html_url: String,
        body: Option<String>,
        published_at: Option<String>,
    }
    let rel: GhRelease = serde_json::from_str(&body).map_err(|e| IpcError::Unknown {
        trace_id: format!("check_app_update:parse:{e}"),
    })?;
    let latest_ver = rel.tag_name.trim_start_matches('v').to_string();
    let has_update = version_gt(&latest_ver, &current);
    Ok(AppUpdateInfo {
        current,
        latest: Some(latest_ver),
        has_update,
        release_url: Some(rel.html_url),
        notes: rel.body,
        published_at: rel.published_at,
    })
}

/// semver-ish 比较 a > b: 按 '.' 分段取前导数字比较, 缺段记 0.
fn version_gt(a: &str, b: &str) -> bool {
    let parse = |s: &str| -> Vec<u64> {
        s.split('.')
            .map(|x| {
                x.trim()
                    .chars()
                    .take_while(|c| c.is_ascii_digit())
                    .collect::<String>()
                    .parse::<u64>()
                    .unwrap_or(0)
            })
            .collect()
    };
    let (va, vb) = (parse(a), parse(b));
    for i in 0..va.len().max(vb.len()) {
        let x = va.get(i).copied().unwrap_or(0);
        let y = vb.get(i).copied().unwrap_or(0);
        if x != y {
            return x > y;
        }
    }
    false
}

/// 统一 provider 解析 + 降级链诊断 — 给 /doctor / 多 agent 视图用。
/// 返回所有已注册 provider 在该 cwd 的统一用量(含 sources 诊断: 走了哪个源/为何降级)。
/// 现有 per-provider 命令保持不变, 此命令是 CodexBar 式 provider 抽象的统一入口。
#[tauri::command]
async fn agent_usage_by_cwd(
    cwd: String,
) -> IpcResult<Vec<vibeterm_agent_watch::provider::AgentUsage>> {
    Ok(tokio::task::spawn_blocking(move || {
        vibeterm_agent_watch::provider::providers()
            .into_iter()
            .filter_map(|p| p.resolve_by_cwd(&cwd))
            .collect()
    })
    .await
    .unwrap_or_default())
}

/// 当前 cwd 对应的 Claude 5h 滚动块 (移植 ccusage `blocks.rs`).
#[tauri::command]
async fn get_claude_block_by_cwd(
    cwd: String,
) -> IpcResult<Option<vibeterm_agent_watch::claude::blocks::ActiveBlock>> {
    // 文件 I/O 重 (read_to_string jsonl), 走 spawn_blocking 不阻塞 tokio runtime.
    Ok(tokio::task::spawn_blocking(move || {
        vibeterm_agent_watch::claude::blocks::active_block_for_cwd(&cwd)
    })
    .await
    .unwrap_or(None))
}

/// Codex 5h 滚动块 (本地按 token_count 事件算, 跟 Claude 同算法).
/// `cwd` 参数是 IPC 对称占位 — 实际 Codex 配额按账号算, 不按 cwd 过滤.
/// 文件 I/O 重 (扫多个 rollout), 走 `spawn_blocking` 不阻塞 tokio runtime.
#[tauri::command]
async fn get_codex_block_by_cwd(
    cwd: String,
) -> IpcResult<Option<vibeterm_agent_watch::claude::blocks::ActiveBlock>> {
    Ok(tokio::task::spawn_blocking(move || {
        vibeterm_agent_watch::codex::blocks::active_block_for_cwd(&cwd)
    })
    .await
    .unwrap_or(None))
}

/// 跨所有 Claude project 累加过去 24h 的 token 用量.
/// 文件 I/O + 行扫描重, 走 `spawn_blocking` 不阻塞 tokio runtime.
#[tauri::command]
async fn get_claude_tokens_today() -> IpcResult<u64> {
    Ok(
        tokio::task::spawn_blocking(vibeterm_agent_watch::claude::project::total_tokens_last_24h)
            .await
            .unwrap_or(0),
    )
}

/// Claude 当前订阅 plan (`Max 20x` / `Pro` / `Free` ...). 未登录返回 None.
/// 读 ~/.claude.json + 解析, 走 `spawn_blocking`.
#[tauri::command]
async fn get_claude_plan() -> IpcResult<Option<String>> {
    Ok(
        tokio::task::spawn_blocking(vibeterm_agent_watch::claude::claude_config::plan_label)
            .await
            .unwrap_or(None),
    )
}

// ---- statusline.toml IO ----

#[tauri::command]
async fn get_statusline_config() -> IpcResult<vibeterm_config::StatusLineFile> {
    Ok(vibeterm_config::StatusLineFile::load())
}

#[tauri::command]
async fn save_statusline_config(
    config: vibeterm_config::StatusLineFile,
    app: AppHandle,
) -> IpcResult<()> {
    config.save().map_err(|e| IpcError::Unknown {
        trace_id: format!("statusline save: {e}"),
    })?;
    let _ = app.emit("statusline_config_changed", ());
    Ok(())
}

/// macOS: 用 lsof 直接拿进程 cwd. 这是不需要 shell integration 的"内核回退路径",
/// 普通用户不配 OSC 7/633 也能拿到 cwd. 失败 (lsof 不可用 / 进程已死) 返回 None.
#[cfg(target_os = "macos")]
fn kernel_cwd_of(pid: u32) -> Option<String> {
    let out = std::process::Command::new("lsof")
        .args(["-a", "-d", "cwd", "-p", &pid.to_string(), "-F", "n"])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let stdout = String::from_utf8_lossy(&out.stdout);
    // 输出格式:
    //   p<pid>
    //   fcwd
    //   n<path>
    for line in stdout.lines() {
        if let Some(path) = line.strip_prefix('n') {
            if !path.is_empty() {
                return Some(path.to_string());
            }
        }
    }
    None
}

#[cfg(not(target_os = "macos"))]
fn kernel_cwd_of(_pid: u32) -> Option<String> {
    None
}

/// 拿某个 terminal 当前 cwd:
///   1. 优先用 StatusDetector 解析的 OSC 633 Cwd (要 shell integration, 最准)
///   2. 退到 lsof 拉 PTY 子进程 (或更深的后裔) 的内核 cwd — 无需 shell 配置
/// 双路径都失败返回 None.
#[tauri::command]
async fn get_terminal_cwd(
    terminal_id: TerminalId,
    state: tauri::State<'_, AppState>,
) -> IpcResult<Option<String>> {
    // 路径 1: OSC 633
    if let Ok(map) = state.status_detectors.lock() {
        if let Some(det) = map.get(&terminal_id) {
            if let Ok(det) = det.lock() {
                if let Some(cwd) = det.current_cwd() {
                    return Ok(Some(cwd.to_string()));
                }
            }
        }
    }
    // 路径 2: 内核 lsof — 找 PTY 进程的最深后裔 (跑着的命令), 没后裔就用 shell 自己
    let Some(shell_pid) = state.terminals.pid_of(terminal_id) else {
        return Ok(None);
    };
    // 嗅探的 cmdlines 副产物里只有命令字符串, 不带 pid; 这里简单点直接用 shell_pid 的 cwd.
    // shell 的 cwd 在用户 `cd` 后会更新, 通常就是 prompt 上下文.
    Ok(kernel_cwd_of(shell_pid))
}

/// 把前端传入的 cwd 字符串规范化为绝对 + canonicalize 后的目录路径.
/// 用于所有"按 cwd 拉某种状态"的 IPC: 防 symlink TOCTOU 跳到敏感目录,
/// 同时保证传给 Command::current_dir 的路径是稳定的真实路径.
/// 失败 (路径不存在 / 不是目录 / canonicalize 报错) 返回 None.
fn safe_cwd(cwd: &str) -> Option<std::path::PathBuf> {
    let p = std::path::PathBuf::from(cwd);
    let canon = std::fs::canonicalize(&p).ok()?;
    if !canon.is_dir() {
        return None;
    }
    Some(canon)
}

/// 拿 cwd 对应的 git 简略状态 (branch / dirty / ahead / behind / staged / unstaged / untracked).
/// 非 git 仓库或路径无效 → None.
#[tauri::command]
async fn git_status_brief(cwd: String) -> IpcResult<Option<vibeterm_git::WorktreeStatus>> {
    let Some(path) = safe_cwd(&cwd) else {
        return Ok(None);
    };
    Ok(vibeterm_git::worktree_status(&path).await.ok())
}

/// stash 数量. 没 stash 或非 git 仓库返回 0.
#[tauri::command]
async fn git_stash_count(cwd: String) -> IpcResult<u32> {
    let Some(path) = safe_cwd(&cwd) else {
        return Ok(0);
    };
    Ok(vibeterm_git::stash_count(&path).await.unwrap_or(0))
}

/// 当前分支的 PR 状态 (用 gh CLI). 没装 gh / 没仓库 / 没 PR 都返回 None.
/// 返回值: "open" / "draft" / "merged" / "closed" / None.
///
/// **超时 5s**: gh CLI 初次使用会触发 auth 提示, 或网络慢时可挂分钟级,
/// 必须有 hard timeout 防止状态栏 refresh tick 被卡死.
#[tauri::command]
async fn gh_pr_status(cwd: String) -> IpcResult<Option<String>> {
    let Some(path) = safe_cwd(&cwd) else {
        return Ok(None);
    };
    let fut = tokio::process::Command::new("gh")
        .args([
            "pr",
            "view",
            "--json",
            "state,isDraft",
            "-q",
            ".state,.isDraft",
        ])
        .current_dir(&path)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .output();
    let out = match tokio::time::timeout(std::time::Duration::from_secs(5), fut).await {
        Ok(r) => r,
        Err(_) => {
            tracing::debug!("gh_pr_status: 5s timeout, treat as no PR");
            return Ok(None);
        }
    };
    let Ok(out) = out else { return Ok(None) };
    if !out.status.success() {
        return Ok(None);
    }
    let s = String::from_utf8_lossy(&out.stdout);
    let mut lines = s.lines();
    let state = lines.next().unwrap_or("").trim();
    let is_draft = lines.next().unwrap_or("").trim() == "true";
    let label = match state {
        "OPEN" if is_draft => "draft",
        "OPEN" => "open",
        "MERGED" => "merged",
        "CLOSED" => "closed",
        _ => return Ok(None),
    };
    Ok(Some(label.to_string()))
}

/// 调试用 — 把前端 console 信息追加到 /tmp/vibeterm-tasklist-debug.log,
/// 方便从主机直接 tail 文件诊断 webview 行为。
/// 仅 debug 构建落盘:/tmp 世界可读 + 写任意前端内容, release 下为 no-op.
#[tauri::command]
async fn debug_log(msg: String) -> IpcResult<()> {
    #[cfg(debug_assertions)]
    {
        use std::io::Write;
        let path = "/tmp/vibeterm-tasklist-debug.log";
        let mut f = match std::fs::OpenOptions::new()
            .append(true)
            .create(true)
            .open(path)
        {
            Ok(f) => f,
            Err(_) => return Ok(()),
        };
        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0);
        let _ = writeln!(f, "{ts} {msg}");
    }
    #[cfg(not(debug_assertions))]
    let _ = msg;
    Ok(())
}

// ============================
// 打开外部资源(URL / 文件路径)
// ============================
//
// 单一 command:open_external,先判断是 URL 还是 fs path:
//   - URL 白名单:https:// / http://localhost / http://127.0.0.1 / file://
//   - 文件路径必须实际存在(防注入 + 防误触发)
// std::process::Command 是 execve 不走 shell,无需担心元字符注入。
#[tauri::command]
async fn open_external(target: String) -> IpcResult<()> {
    // localhost/127.0.0.1 用精确 host 匹配,防 `http://localhost.evil.com` 前缀绕过.
    let is_url = target.starts_with("https://")
        || is_trusted_local_http(&target)
        || target.starts_with("file://");
    // 非 URL 的 fs path:canonicalize 消除 `../` 穿越歧义,用真实绝对路径打开,
    // 拒绝无法规范化的目标(不存在或非法).
    let resolved_path = if is_url {
        None
    } else {
        std::fs::canonicalize(&target).ok()
    };
    if !is_url && resolved_path.is_none() {
        tracing::warn!(target, "rejected open_external — not in whitelist");
        return Err(IpcError::PermissionDenied {
            reason: "target not in whitelist (need https / localhost / existing fs path)".into(),
        });
    }
    // URL 用原始 target;fs path 用规范化后的绝对路径
    let open_target: &std::ffi::OsStr = match &resolved_path {
        Some(p) => p.as_os_str(),
        None => std::ffi::OsStr::new(&target),
    };
    let spawn_result = if cfg!(target_os = "macos") {
        std::process::Command::new("open").arg(open_target).spawn()
    } else if cfg!(target_os = "linux") {
        std::process::Command::new("xdg-open")
            .arg(open_target)
            .spawn()
    } else {
        // windows:cmd /c start "" "<target>" — "" 是 start 的 title 占位
        std::process::Command::new("cmd")
            .args([
                std::ffi::OsStr::new("/c"),
                std::ffi::OsStr::new("start"),
                std::ffi::OsStr::new(""),
                open_target,
            ])
            .spawn()
    };
    spawn_result.map(|_| ()).map_err(|e| IpcError::Unknown {
        trace_id: format!("open_external: {e}"),
    })
}

// ============================
// 日志
// ============================

/// 编译期默认日志 level。
///
/// - debug build: `info` — 开发期需看到 spawn/close 等生命周期事件
/// - release build: `warn` — 生产只保留错误与异常信号,降低噪声
///
/// 用户可用 `RUST_LOG` 覆盖,语义同 `tracing_subscriber::EnvFilter`:
///   - `RUST_LOG=debug` 全 crate 提到 debug
///   - `RUST_LOG=vibeterm_status=trace,info` 单 crate 详查
///   - `RUST_LOG=off` 完全静默
fn default_log_filter() -> &'static str {
    if cfg!(debug_assertions) {
        "info"
    } else {
        "warn"
    }
}

fn init_tracing() {
    let filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(default_log_filter()));
    tracing_subscriber::fmt().with_env_filter(filter).init();
}

/// macOS `.app` 从 Finder 启动时只继承系统 PATH (/usr/bin:/bin:...),
/// 看不到 /opt/homebrew/bin、~/.local/bin、~/.cargo/bin、nvm node 等用户安装路径,
/// 导致 which::which("claude") / PTY spawn 找不到 AI CLI.
/// 修法: 调起用户 login shell (interactive) 抓 PATH 写回当前进程.
/// (npm `fix-path` 包同款思路, VS Code/Cursor/Atom 也都这么做)
#[cfg(target_os = "macos")]
fn fix_path_for_gui_launch() {
    // 已经有完整 PATH (dev 模式 / 用户从 terminal 启动) → 跳过避免无谓 shell 启动
    let current = std::env::var("PATH").unwrap_or_default();
    let home = std::env::var("HOME").unwrap_or_default();
    let homebrew_present =
        current.contains("/opt/homebrew/bin") || current.contains("/usr/local/bin");
    let user_local_present = !home.is_empty() && current.contains(&format!("{home}/.local/bin"));
    if homebrew_present || user_local_present {
        return;
    }

    let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".to_string());
    // 用 sentinel 提取干净的 PATH; -ilc = interactive login command, 强制 source .zshrc/.bash_profile
    let cmd = "printf '__VT_PATH_START__%s__VT_PATH_END__' \"$PATH\"";
    let out = match std::process::Command::new(&shell)
        .args(["-ilc", cmd])
        .output()
    {
        Ok(o) => o,
        Err(e) => {
            tracing::warn!("fix_path: spawn {shell} failed: {e}");
            return;
        }
    };
    let stdout = String::from_utf8_lossy(&out.stdout);
    let path = match (
        stdout.find("__VT_PATH_START__"),
        stdout.find("__VT_PATH_END__"),
    ) {
        (Some(a), Some(b)) if a + "__VT_PATH_START__".len() <= b => {
            &stdout[a + "__VT_PATH_START__".len()..b]
        }
        _ => {
            tracing::warn!("fix_path: sentinel not found in shell output");
            return;
        }
    };
    if path.is_empty() {
        return;
    }
    tracing::info!("fix_path: PATH inherited from {shell} (len={})", path.len());
    std::env::set_var("PATH", path);
}

#[cfg(not(target_os = "macos"))]
fn fix_path_for_gui_launch() {}

// ============================
// main
// ============================
fn main() {
    init_tracing();
    // 常驻音频线程(通知声音用 rodio 进程内播放,替代反复 fork 的 afplay)。早启动,后续直接 send。
    init_audio_thread();
    // 必须早于任何 which::which / PTY spawn — 否则 GUI 启动的 .app 看不到用户 PATH
    fix_path_for_gui_launch();

    // 创建配置目录(首启动)
    if let Err(e) = vibeterm_config::config_dir() {
        eprintln!("config dir error: {e}");
    }

    let state = AppState {
        terminals: Arc::new(TerminalRegistry::new()),
        tasks: Arc::new(TaskRegistry::new()),
        menu_lang: std::sync::Mutex::new(MenuLang::from_env()),
        status_detectors: std::sync::Mutex::new(std::collections::HashMap::new()),
        last_notify: std::sync::Mutex::new(None),
        last_agent_completed: std::sync::Mutex::new(std::collections::HashMap::new()),
        last_persistent_remind: std::sync::Mutex::new(None),
    };

    // 首启动:若无任务则创建一个 Default
    if let Ok(list) = state.tasks.list() {
        if list.is_empty() {
            let name = match std::env::var("LANG")
                .unwrap_or_default()
                .to_lowercase()
                .as_str()
            {
                s if s.starts_with("zh") => "默认".to_string(),
                s if s.starts_with("ja") => "デフォルト".to_string(),
                _ => "Default".to_string(),
            };
            let _ = state.tasks.create(name, None, None);
        }
    }

    tauri::Builder::default()
        // 必须第一个注册: 第二次启动时聚焦已有主窗口而非起平行实例 —— 平行实例会与
        // 当前实例 last-writer-wins 抢 tasks.json, 把用户已删的任务覆盖回来.
        .plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| {
            if let Some(w) = app.get_webview_window("main") {
                let _ = w.unminimize();
                let _ = w.show();
                let _ = w.set_focus();
            }
        }))
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_dialog::init())
        .manage(state)
        .invoke_handler(tauri::generate_handler![
            // Terminal
            start_pty,
            write_pty,
            resize_pty,
            close_pty,
            spawn_terminal_in_task,
            attach_terminal,
            detach_terminal,
            get_scrollback,
            save_clipboard_image,
            paste_clipboard_image,
            paste_clipboard,
            get_clipboard_images_dir,
            open_clipboard_images_dir,
            clear_clipboard_images,
            // Tasks
            list_tasks,
            create_task,
            close_task,
            rename_task,
            pin_task,
            reorder_tasks,
            set_active_task,
            set_task_split_tree,
            set_task_notify_muted,
            // 通知偏好
            get_notify_prefs,
            save_notify_prefs,
            notify_permission,
            request_notify_permission,
            preview_notify_sound,
            list_builtin_sounds,
            // Git worktree
            git_is_repo,
            git_repo_root,
            git_list_worktrees,
            git_list_branches,
            git_add_worktree,
            git_remove_worktree,
            attach_worktree_to_task,
            detach_worktree_from_task,
            refresh_worktree_status,
            // Theme / Config
            get_config,
            set_shell_integration,
            set_active_theme,
            list_themes,
            get_theme,
            get_env_file,
            save_env_file,
            get_keybindings,
            save_keybindings,
            reset_keybindings,
            detect_agent_for_terminal,
            reset_prompts,
            get_prompts,
            save_prompts,
            // Custom Actions
            get_actions,
            save_actions,
            execute_action,
            // layout snapshot
            get_active_task,
            // Window
            open_floating,
            close_floating,
            focus_window,
            invoke_global_action,
            // i18n
            set_menu_lang,
            // AI CLI 检测
            detect_ai_clis,
            debug_log,
            // Agent watch (v1+v2+v3) — Claude usage_cache + Claude session + Codex session
            get_claude_usage_cache,
            get_claude_session,
            get_codex_session,
            // 按 cwd 精确查 session (per-active-terminal 语义)
            get_claude_session_by_cwd,
            get_codex_session_by_cwd,
            agent_usage_by_cwd,
            get_claude_block_by_cwd,
            get_codex_block_by_cwd,
            get_claude_tokens_today,
            get_usage_stats,
            save_png_file,
            get_claude_plan,
            // v4: cwd + git status (按需调, 非常驻 watcher)
            get_terminal_cwd,
            git_status_brief,
            git_stash_count,
            gh_pr_status,
            // 状态栏自定义配置
            get_statusline_config,
            save_statusline_config,
            // 打开外部 URL / 文件
            open_external,
            // 设置·更新页:软件版本检查 + 模型价格更新(手动, 仅点按钮时联网)
            check_app_update,
            get_pricing_status,
            update_model_pricing,
            reset_model_pricing,
        ])
        .setup(|app| {
            // agent 状态走纯嗅探(OSC 标题 spinner + 输出时序)+ 只读文件监听, 不再装/起任何
            // hook server, 零侵入: 默认不碰 ~/.claude / ~/.codex, 也不会被外部会话污染.

            // 启动加载已保存的模型价格覆盖(用户曾手动"更新模型价格"过). 纯读本地 config 文件, 不联网.
            if let Ok(path) = vibeterm_config::pricing_json_path() {
                if let Ok(bytes) = std::fs::read(&path) {
                    match serde_json::from_slice::<
                        vibeterm_agent_watch::claude::pricing::PricingTable,
                    >(&bytes)
                    {
                        Ok(table) => {
                            vibeterm_agent_watch::claude::pricing::set_pricing_override(table)
                        }
                        Err(e) => tracing::warn!("ignore corrupt pricing.json: {e}"),
                    }
                }
            }

            // 启动 config watcher(50ms debounce)
            // 同时 fan-out 到 statusline_config_changed — 用户改 statusline.toml 即时生效
            let app_h = app.handle().clone();
            if let Ok(w) = vibeterm_config::ConfigWatcher::start(move || {
                tracing::info!(
                    "config dir changed → emit config_changed + statusline_config_changed"
                );
                let _ = app_h.emit("config_changed", ());
                let _ = app_h.emit("statusline_config_changed", ());
            }) {
                // watcher 被 leak 让其活到 app 退出(简化)
                Box::leak(Box::new(w));
            }

            // Agent watch v1: Claude usage_cache.json 监听
            let app_for_usage = app.handle().clone();
            let (usage_tx, mut usage_rx) = tokio::sync::mpsc::unbounded_channel::<
                vibeterm_agent_watch::claude::usage_cache::UsageCacheUpdate,
            >();
            vibeterm_agent_watch::claude::usage_cache::spawn_watcher(usage_tx);
            tauri::async_runtime::spawn(async move {
                while let Some(update) = usage_rx.recv().await {
                    let _ = app_for_usage.emit("claude_usage_changed", &update.cache);
                }
            });

            // Agent watch v2: Claude project transcript 监听
            let app_for_session = app.handle().clone();
            let (sess_tx, mut sess_rx) = tokio::sync::mpsc::unbounded_channel::<
                Option<vibeterm_agent_watch::ClaudeSession>,
            >();
            vibeterm_agent_watch::claude::project::spawn_watcher(sess_tx);
            tauri::async_runtime::spawn(async move {
                while let Some(sess) = sess_rx.recv().await {
                    // watcher 只刷新显示(model/ctx/cost),不驱动完成检测。
                    // 为何:这里的 ClaudeSession 来自 find_active_session_file() —— 全局 mtime 最新
                    // 的会话,未必是本任务 agent 的。典型反例:同一仓库里 Claude Code 自身几百 MB 的
                    // transcript,每条消息都在写 → mtime 几乎永远最新,且超限只能读末尾 stop_reason。
                    // 若用它驱动完成,会把"另一个 claude 会话答完了"误判成本任务 agent 答完 → claude
                    // 完成漏报(只剩 claude 自己 hook 弹的无声通知)。完成检测一律走 3s 轮询的
                    // poll_agent_turn_from_transcript → read_for_cwd(按 task.cwd 精确定位 + 排除超限
                    // 巨型会话)。codex 因会话按日期分目录、snapshot 自带精确 cwd,无此碰撞,保留其
                    // watcher 完成路径。
                    let _ = app_for_session.emit("claude_session_changed", &sess);
                }
            });

            // Agent watch v3: Codex session 监听
            let app_for_codex = app.handle().clone();
            let (codex_tx, mut codex_rx) = tokio::sync::mpsc::unbounded_channel::<
                Option<vibeterm_agent_watch::CodexSnapshot>,
            >();
            vibeterm_agent_watch::codex::session::spawn_watcher(codex_tx);
            tauri::async_runtime::spawn(async move {
                while let Some(snap) = codex_rx.recv().await {
                    // transcript 轮状态:task_completed(完成晚于开始)→ done;否则(新 task_started 在干)→ working
                    if let Some(ref s) = snap {
                        on_agent_turn_update(
                            &app_for_codex,
                            "codex",
                            &s.cwd,
                            false,
                            s.task_completed,
                            None, // codex 维持布尔跃迁判定(不引入 turn_id 改动风险)
                        );
                    }
                    let _ = app_for_codex.emit("codex_session_changed", &snap);
                }
            });

            // agent 进程识别轮询(每 3s)
            //   扫每个 task 的 terminals 的 shell pid → detect_agent_for_shell。
            //   有变化 emit tasks_changed。pgrep 在前台 idle 时也会运行,~1ms 量级,可接受。
            let app_for_agent = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                let interval = std::time::Duration::from_secs(3);
                loop {
                    tokio::time::sleep(interval).await;
                    let state = match app_for_agent.try_state::<AppState>() {
                        Some(s) => s,
                        None => continue,
                    };
                    let pairs = match state.tasks.task_terminal_ids() {
                        Ok(v) => v,
                        Err(_) => continue,
                    };
                    let mut any_changed = false;
                    for (task_id, term_ids) in pairs {
                        // 逐 terminal 检测 agent — 关键修复:
                        // 旧逻辑给 task 里 *所有* terminal 都开 stall 检测,导致同 task 里
                        // 的 idle shell (split 出来的辅助窗口)被错误标记 Stalled.
                        // 新:per-terminal 标记,只对真跑 agent 的那个 terminal 开 stall.
                        let mut agent_per_term: Vec<(TerminalId, Option<String>)> = Vec::new();
                        let mut task_agent_kind: Option<String> = None;
                        for term_id in &term_ids {
                            let kind = state
                                .terminals
                                .pid_of(*term_id)
                                .and_then(vibeterm_status::detect_agent_for_shell)
                                .map(|k| k.as_str().to_string());
                            agent_per_term.push((*term_id, kind.clone()));
                            if task_agent_kind.is_none() {
                                if let Some(k) = kind {
                                    task_agent_kind = Some(k);
                                }
                            }
                        }
                        let kind_for_turn = task_agent_kind.clone();
                        if let Ok(true) = state.tasks.set_agent_kind(task_id, task_agent_kind) {
                            any_changed = true;
                        }
                        // 兜底:文件监听(notify/FSEvents)可能漏 agent 完成写入(codex 尤甚),主 app
                        // 收不到 → 圆点卡 Running。这里按 task.cwd 主动读一次 transcript 完成状态
                        // (不依赖监听,文件已写就读得到),最多 3s 圆点转 Done。
                        if let Some(kind) = kind_for_turn.as_deref() {
                            poll_agent_turn_from_transcript(
                                &app_for_agent,
                                &state.tasks,
                                task_id,
                                kind,
                            );
                        }
                        // 按 per-terminal 嗅探到的 agent kind:
                        //   - 装对应授权框正则(set_agent_rules) → body 正则识别 WaitingInput;
                        //   - 真跑 agent 的 terminal 才开 stall 检测(辅助 idle shell 不开)。
                        if let Ok(detectors) = state.status_detectors.lock() {
                            for (term_id, kind) in &agent_per_term {
                                if let Some(d) = detectors.get(term_id) {
                                    if let Ok(mut det) = d.lock() {
                                        det.set_agent_rules(kind.as_deref());
                                        if kind.is_some() {
                                            det.enable_stall_detection(0);
                                        } else {
                                            let _ = det.disable_stall_detection();
                                        }
                                    }
                                }
                            }
                        }
                    }
                    if any_changed {
                        emit_tasks_changed(&app_for_agent, &state.tasks);
                    }
                }
            });

            // last_output 节流轮询(750ms/轮)
            //   每个 task 取 terminal_ids.last() 的末行,与上轮快照比较,
            //   有差异才 emit_tasks_changed。避免 stdout 频繁刷新时 emit 风暴。
            let app_for_tail = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                let interval = std::time::Duration::from_millis(750);
                let mut prev: std::collections::HashMap<vibeterm_ipc::TaskId, Option<String>> =
                    std::collections::HashMap::new();
                loop {
                    tokio::time::sleep(interval).await;
                    let state = match app_for_tail.try_state::<AppState>() {
                        Some(s) => s,
                        None => continue,
                    };
                    let pairs = match state.tasks.task_terminal_ids() {
                        Ok(v) => v,
                        Err(_) => continue,
                    };
                    let mut any_changed = false;
                    let mut alive: std::collections::HashSet<vibeterm_ipc::TaskId> =
                        std::collections::HashSet::new();
                    for (task_id, term_ids) in pairs {
                        alive.insert(task_id);
                        let tail = if term_ids.is_empty() {
                            None
                        } else {
                            state.terminals.most_recent_tail(&term_ids)
                        };
                        let prev_tail = prev.get(&task_id).cloned().unwrap_or(None);
                        if prev_tail != tail {
                            prev.insert(task_id, tail);
                            any_changed = true;
                        }
                    }
                    // 清理已删任务的快照
                    prev.retain(|k, _| alive.contains(k));
                    if any_changed {
                        emit_tasks_changed(&app_for_tail, &state.tasks);
                    }
                }
            });

            // worktree 状态轮询(5s/轮)
            //   仅扫挂了 worktree 的 task,对每个跑 `git status --porcelain=v2 --branch`。
            //   有任意字段变化 → emit tasks_changed。
            //   只在内存更新(update_worktree_status 不写盘),IO 噪音低。
            let app_for_poll = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                let interval = std::time::Duration::from_secs(5);
                loop {
                    tokio::time::sleep(interval).await;
                    let state = match app_for_poll.try_state::<AppState>() {
                        Some(s) => s,
                        None => continue,
                    };
                    let pairs = match state.tasks.worktree_tasks() {
                        Ok(v) => v,
                        Err(_) => continue,
                    };
                    if pairs.is_empty() {
                        continue;
                    }
                    let mut any_changed = false;
                    for (task_id, wt) in pairs {
                        let wt_path = match validated_worktree_path(&wt.worktree_path) {
                            Some(p) => p,
                            None => continue,
                        };
                        let st = match vibeterm_git::worktree_status(&wt_path).await {
                            Ok(s) => s,
                            Err(e) => {
                                tracing::debug!(?task_id, err = %e, "worktree status poll failed");
                                continue;
                            }
                        };
                        let changed = st.head != wt.head
                            || st.branch != wt.branch
                            || st.is_dirty != wt.is_dirty
                            || st.ahead != wt.ahead
                            || st.behind != wt.behind;
                        if changed {
                            let _ = state.tasks.update_worktree_status(
                                task_id,
                                st.head,
                                st.branch,
                                st.is_dirty,
                                st.ahead,
                                st.behind,
                                now_ms(),
                            );
                            any_changed = true;
                        }
                    }
                    if any_changed {
                        emit_tasks_changed(&app_for_poll, &state.tasks);
                    }
                }
            });

            // 全局 status-tick 任务(200ms/轮):遍历所有活跃 StatusDetector 做 stall/idle
            // 时间态判定. 取代旧的"每终端一个 OS 线程"——单任务即可; detector 一旦从
            // status_detectors 摘除(close / PTY 退出)就自动不再被 tick,无线程泄漏可能.
            let app_for_status_tick = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                let interval = std::time::Duration::from_millis(200);
                loop {
                    tokio::time::sleep(interval).await;
                    let state = match app_for_status_tick.try_state::<AppState>() {
                        Some(s) => s,
                        None => continue,
                    };
                    // 快照 (tid, detector) 后立即释放 map 锁,避免 tick/emit 期间持锁.
                    let detectors: Vec<(TerminalId, Arc<std::sync::Mutex<StatusDetector>>)> =
                        match state.status_detectors.lock() {
                            Ok(m) => m.iter().map(|(k, v)| (*k, v.clone())).collect(),
                            Err(_) => continue,
                        };
                    for (tid, det) in detectors {
                        let (new_state, idle_by_osc) = {
                            let mut d = det.lock().unwrap_or_else(|p| p.into_inner());
                            (d.tick(), d.idle_by_osc())
                        };
                        let Some(s) = new_state else { continue };
                        if let Ok(Some((task_id, prev_agg, new_agg))) =
                            state.tasks.update_terminal_status(tid, s, idle_by_osc)
                        {
                            let _ = app_for_status_tick.emit(
                                "task_status_changed",
                                serde_json::json!({"task_id": task_id, "status": s}),
                            );
                            emit_tasks_changed(&app_for_status_tick, &state.tasks);
                            notify_status_transition(
                                &app_for_status_tick,
                                &state.tasks,
                                task_id,
                                prev_agg,
                                new_agg,
                                idle_by_osc,
                            );
                            refresh_dock_badge(&app_for_status_tick, &state.tasks);
                        }
                    }
                    // 每轮 tick 末尾:间歇持续提醒(单路全局声音)检查
                    maybe_persistent_remind(&app_for_status_tick, &state);
                }
            });

            // 主窗口:平台特异创建 — macOS 用 Overlay titleBar(原生 traffic lights),
            // 其它平台保持 decorations:false 走自渲 titleBar(Win 风格右上三按钮)
            // background_color #111 让原生窗口在 webview 加载前就是暗色,不闪白
            let main_builder =
                WebviewWindowBuilder::new(app, "main", WebviewUrl::App("index.html".into()))
                    .title("VibeTerm")
                    .inner_size(1100.0, 720.0)
                    .min_inner_size(800.0, 500.0)
                    .background_color(tauri::window::Color(0x11, 0x11, 0x11, 0xff));
            // 历史注释提到关 drag-drop handler 是为状态栏 widget 排序; 但 solid-dnd 用
            // pointer events (pointerdown/move/up), 与 HTML5 native DnD 无关. Tauri
            // native drag-drop 必须开 — 否则 WebView 自己处理 drop, 拖图片进终端
            // 会被当成浏览器导航直接打开文件 (terminal/index.tsx 的 onDragDropEvent 监听失效).
            #[cfg(target_os = "macos")]
            let main_builder = main_builder
                .title_bar_style(tauri::TitleBarStyle::Overlay)
                .hidden_title(true)
                // wry PR #1662:transparent(true) 触发 WKWebView drawsBackground=false,
                // 修 resize 时 WebView 渲染滞后露白底的问题(已用 NSWindow.bg #111 兜底显示)
                .transparent(true);
            #[cfg(not(target_os = "macos"))]
            let main_builder = main_builder.decorations(false);
            let main_win = main_builder
                .build()
                .map_err(|e| Box::<dyn std::error::Error>::from(e.to_string()))?;
            #[cfg(target_os = "macos")]
            apply_macos_vibrancy(&main_win);
            #[cfg(not(target_os = "macos"))]
            let _ = main_win;

            // macOS 完整菜单栏 + 派发到 web
            #[cfg(target_os = "macos")]
            {
                let lang = app
                    .try_state::<AppState>()
                    .map(|s| current_menu_lang(&s))
                    .unwrap_or(MenuLang::En);
                let menu = build_menu(app.handle(), lang)?;
                app.set_menu(menu)?;
                app.on_menu_event(|app_handle, ev| {
                    let id = ev.id().0.clone();
                    tracing::debug!(menu_id = %id, "menu event");
                    match id.as_str() {
                        "open_config_dir" => {
                            if let Ok(dir) = vibeterm_config::config_dir() {
                                #[cfg(target_os = "macos")]
                                let _ = std::process::Command::new("open").arg(dir).spawn();
                            }
                        }
                        "open_github" => open_url_safe(app_handle, "https://github.com"),
                        "open_issues" => open_url_safe(app_handle, "https://github.com"),
                        "open_privacy" => open_url_safe(app_handle, "https://github.com"),
                        "focus_main" => {
                            if let Some(w) = app_handle.get_webview_window("main") {
                                let _ = w.show();
                                let _ = w.set_focus();
                            }
                        }
                        // 浮窗 focus(id 格式:focus_floating:<label>)
                        id if id.starts_with("focus_floating:") => {
                            let label = &id["focus_floating:".len()..];
                            if let Some(w) = app_handle.get_webview_window(label) {
                                let _ = w.show();
                                let _ = w.set_focus();
                            }
                        }
                        // 其余:转给 main 窗口的 global_action listener
                        _ => {
                            if let Some(main) = app_handle.get_webview_window("main") {
                                let _ = main.show();
                                let _ = main.set_focus();
                                let _ = app_handle.emit_to(
                                    tauri::EventTarget::WebviewWindow {
                                        label: "main".into(),
                                    },
                                    "global_action",
                                    id,
                                );
                            }
                        }
                    }
                });
            }

            Ok(())
        })
        .on_window_event(|window, event| {
            // 主窗口焦点变化:① 同步 window_focused —— 失焦时当前选中 task 完成会显 Done(未看)
            // 并计入 Dock 角标(用户在别的 app 也能从 Dock 看到),聚焦时标当前 task 已读;
            // ② 聚焦 + NOTIFY_FOCUS_GRACE 内有 last_notify → 通知前端切 task(桌面通知无 click callback 的近似)。
            if let tauri::WindowEvent::Focused(focused) = event {
                if window.label() == "main" {
                    tracing::info!(
                        focused = *focused,
                        "WindowEvent::Focused(主窗口焦点事件触发)"
                    );
                    let app = window.app_handle().clone();
                    if let Some(state) = app.try_state::<AppState>() {
                        if state.tasks.set_window_focused(*focused).unwrap_or(false) {
                            emit_tasks_changed(&app, &state.tasks);
                            refresh_dock_badge(&app, &state.tasks);
                        }
                        if *focused {
                            let task = state.last_notify.lock().ok().and_then(|mut g| {
                                g.take().filter(|(_, t)| t.elapsed() < NOTIFY_FOCUS_GRACE)
                            });
                            if let Some((task_id, _)) = task {
                                let _ = app.emit(
                                    "notification_focus_target",
                                    serde_json::json!({"task_id": task_id}),
                                );
                            }
                        }
                    }
                }
            }
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                let label = window.label().to_string();
                if cfg!(target_os = "macos") && label == "main" {
                    // macOS:关主窗 = 隐藏而非真退,符合 Cocoa 惯例
                    tracing::info!("main close intercepted -> hide");
                    api.prevent_close();
                    let _ = window.hide();
                    return;
                }
                // 浮窗系统关 = 与右键"回到主窗口"同款流程 —
                // 把 task.location 改回 Nowhere + emit tasks_changed,
                // 让主窗 onTasksChanged 自动 setActive 召回到右侧。
                if label.starts_with("floating-") {
                    let app = window.app_handle().clone();
                    let label_for_async = label.clone();
                    tauri::async_runtime::spawn(async move {
                        if let Some(state) = app.try_state::<AppState>() {
                            if let Ok(tasks) = state.tasks.list() {
                                for t in tasks {
                                    if let TaskLocation::Floating(ref l) = t.location {
                                        if l == &label_for_async {
                                            let _ = state
                                                .tasks
                                                .set_location(t.id, TaskLocation::Nowhere);
                                        }
                                    }
                                }
                            }
                            emit_tasks_changed(&app, &state.tasks);
                            let _ = app.emit("floating_closed", &label_for_async);
                            #[cfg(target_os = "macos")]
                            if let Ok(menu) = build_menu(&app, current_menu_lang(&state)) {
                                let _ = app.set_menu(menu);
                            }
                        }
                    });
                }
            }
        })
        .build(tauri::generate_context!())
        .expect("error building tauri app")
        .run(|app, event| match event {
            #[cfg(target_os = "macos")]
            RunEvent::ExitRequested { code, api, .. } => {
                if code.is_none() {
                    api.prevent_exit();
                }
            }
            #[cfg(target_os = "macos")]
            RunEvent::Reopen { .. } => {
                if let Some(w) = app.get_webview_window("main") {
                    let _ = w.show();
                    let _ = w.set_focus();
                }
            }
            _ => {
                let _ = (app, Duration::from_secs(0));
            }
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 默认 filter 必须能被 EnvFilter 解析(否则 release 启动会 panic)
    #[test]
    fn default_log_filter_parses_cleanly() {
        let f = default_log_filter();
        EnvFilter::new(f);
    }

    /// debug build → info,release build → warn。
    /// `cargo test` 默认以 debug profile 编译,故此处必为 "info"。
    #[test]
    fn default_log_filter_is_info_in_debug_build() {
        assert_eq!(default_log_filter(), "info");
    }
}
