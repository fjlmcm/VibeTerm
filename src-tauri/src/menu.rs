//! macOS 菜单栏:14 语言标签表 + 菜单构建。
//! 从 main.rs 拆出(行为不变);MenuLang 由 set_menu_lang IPC(留 main.rs)驱动。

#[cfg(target_os = "macos")]
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem, Submenu};
use tauri::AppHandle;
#[cfg(target_os = "macos")]
use tauri::Manager;

#[cfg(target_os = "macos")]
use crate::AppState;

#[derive(Clone, Copy, Debug)]
pub(crate) enum MenuLang {
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
    pub(crate) fn from_tag(s: &str) -> Self {
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

    pub(crate) fn from_env() -> Self {
        Self::from_tag(&std::env::var("LANG").unwrap_or_default())
    }
}

pub(crate) struct MenuLabels {
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

pub(crate) const LBL_ZH: MenuLabels = MenuLabels {
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

pub(crate) const LBL_EN: MenuLabels = MenuLabels {
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

pub(crate) const LBL_JA: MenuLabels = MenuLabels {
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

pub(crate) const LBL_ZH_HANT: MenuLabels = MenuLabels {
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

pub(crate) const LBL_KO: MenuLabels = MenuLabels {
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

pub(crate) const LBL_VI: MenuLabels = MenuLabels {
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

pub(crate) const LBL_ID: MenuLabels = MenuLabels {
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

pub(crate) const LBL_ES: MenuLabels = MenuLabels {
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

pub(crate) const LBL_PT_BR: MenuLabels = MenuLabels {
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

pub(crate) const LBL_DE: MenuLabels = MenuLabels {
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

pub(crate) const LBL_FR: MenuLabels = MenuLabels {
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

pub(crate) const LBL_IT: MenuLabels = MenuLabels {
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

pub(crate) const LBL_RU: MenuLabels = MenuLabels {
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

pub(crate) const LBL_TR: MenuLabels = MenuLabels {
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

pub(crate) fn menu_labels(l: MenuLang) -> &'static MenuLabels {
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

// macOS 完整菜单栏;加浮窗动态列表
// PredefinedMenuItem 的 NSResponder 行为(Cut/Copy/Paste/Undo/Hide/Quit…)
// 由系统自动 dispatch;我们只通过 Some(text) 覆盖 label,跟随应用 lang。
#[cfg(target_os = "macos")]
pub(crate) fn build_menu(app: &AppHandle, lang: MenuLang) -> tauri::Result<Menu<tauri::Wry>> {
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
pub(crate) fn current_menu_lang(state: &AppState) -> MenuLang {
    state
        .menu_lang
        .lock()
        .ok()
        .map(|g| *g)
        .unwrap_or(MenuLang::En)
}
